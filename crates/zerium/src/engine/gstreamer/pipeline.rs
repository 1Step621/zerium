//! Shared pipeline lifetime, URI handling, and bounded reads.
use crate::engine::media::MediaError;
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app::AppSink;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub(super) fn file_uri(path: &Path) -> Result<url::Url, MediaError> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(MediaError::external)?
            .join(path)
    };
    url::Url::from_file_path(path).map_err(|_| MediaError::invalid_input("Invalid media path"))
}

/// Always stop streaming threads, including when setup or seeking fails.
pub(super) struct Pipeline(gst::Pipeline);

impl Pipeline {
    pub(super) fn new(pipeline: gst::Pipeline) -> Self {
        pipeline.connect_deep_element_added(|_, _, element| {
            if element.factory().is_some_and(|factory| {
                factory
                    .metadata("klass")
                    .is_some_and(|klass| klass.contains("Decoder/Video"))
            }) && element.find_property("max-threads").is_some()
            {
                element.set_property("max-threads", 4i32);
            }
        });
        Self(pipeline)
    }
}

impl std::ops::Deref for Pipeline {
    type Target = gst::Pipeline;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        let _ = self.set_state(gst::State::Null);
    }
}

pub(super) struct SinkPipeline {
    pub(super) pipeline: Pipeline,
    pub(super) sink: AppSink,
}

impl SinkPipeline {
    pub(super) fn new() -> Self {
        Self {
            pipeline: Pipeline::new(gst::Pipeline::new()),
            sink: AppSink::builder()
                .sync(false)
                .max_buffers(2)
                .enable_last_sample(false)
                .wait_on_eos(false)
                .build(),
        }
    }

    pub(super) fn pull(
        &self,
        preroll: bool,
        cancelled: Option<&AtomicBool>,
    ) -> Result<Option<gst::Sample>, MediaError> {
        pull(&self.pipeline, &self.sink, preroll, cancelled)
    }
}

fn pull(
    pipeline: &gst::Pipeline,
    sink: &AppSink,
    preroll: bool,
    cancelled: Option<&AtomicBool>,
) -> Result<Option<gst::Sample>, MediaError> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Relaxed)) {
            return Err(MediaError::Cancelled);
        }
        let wait = gst::ClockTime::from_mseconds(10);
        let sample = if preroll {
            sink.try_pull_preroll(wait)
        } else {
            sink.try_pull_sample(wait)
        };
        if sample.is_some() {
            return Ok(sample);
        }
        if let Some(bus) = pipeline.bus() {
            while let Some(message) = bus.pop() {
                if let gst::MessageView::Error(error) = message.view() {
                    return Err(MediaError::external(error.error()));
                }
            }
        }
        if sink.is_eos() {
            return Ok(None);
        }
        if Instant::now() >= deadline {
            return Err(MediaError::external("Timed out waiting for a media sample"));
        }
    }
}
