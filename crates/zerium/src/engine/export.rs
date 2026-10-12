use std::{
    collections::HashMap,
    fmt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use thiserror::Error;

use crate::engine::{
    atomic_file::AtomicFileTransaction,
    gstreamer::encoding::{
        ExportFrame, ExportFramePipeline, FileEncoder, VideoEncoderSettings, VideoEncoding,
        VideoOutputSpec,
    },
    media::{
        AudioFormat, AudioGainEvaluation, AudioTimelineGraph, MediaInputId, MediaReaderRegistry,
        VisualDecoder, item_readings, refresh_files, sample_boundary,
    },
    rendering::{FrameRenderer, RenderScene, RenderSize, TextFrameCache},
};
use zerium_core::{
    media::VideoFrameRate,
    timeline::{Frame, FrameRate, TimelineSnapshot, TimelineTime, TimelineView},
};

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum ExportError {
    #[error("Export cancelled")]
    Cancelled,
    #[error("{0}")]
    InvalidTimeline(String),
    #[error(transparent)]
    Media(#[from] crate::engine::media::MediaError),
    #[error(transparent)]
    Render(#[from] crate::engine::rendering::RenderError),
    #[error("{0}")]
    Encoding(String),
}

impl ExportError {
    pub(crate) fn encoding(message: impl fmt::Display) -> Self {
        Self::Encoding(message.to_string())
    }
}

impl From<crate::engine::gstreamer::encoding::EncoderError> for ExportError {
    fn from(error: crate::engine::gstreamer::encoding::EncoderError) -> Self {
        match error {
            crate::engine::gstreamer::encoding::EncoderError::Cancelled => Self::Cancelled,
            error => Self::encoding(error),
        }
    }
}

const EXPORT_AUDIO_FORMAT: AudioFormat = AudioFormat {
    sample_rate: 48_000,
    channels: 2,
};
const EXPORT_PIPELINE_DEPTH: usize = 3;
const DECODE_PREFETCH_FRAMES: usize = 3;

pub(crate) enum ExportProgress {
    Frame(u64),
    Finished(Result<(), ExportError>),
}

enum EncoderMessage {
    Frame(ExportFrame),
    Complete,
}

pub(crate) fn export_timeline(
    timeline: TimelineSnapshot,
    renderer: FrameRenderer,
    media_readers: Arc<MediaReaderRegistry>,
    output: PathBuf,
    cancelled: &AtomicBool,
    progress: futures::channel::mpsc::UnboundedSender<ExportProgress>,
) -> Result<(), ExportError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(ExportError::Cancelled);
    }
    let frame_count = timeline.end_frame_exclusive().get();
    if frame_count == 0 {
        return Err(ExportError::InvalidTimeline(
            "There are no items to export on the timeline".to_owned(),
        ));
    }
    let (files, errors) = refresh_files(
        &mut item_readings(&timeline.visible_items()),
        timeline.media_cache(),
        &media_readers,
    );
    if let Some(error) = errors.into_iter().next() {
        return Err(error.into());
    }
    let mut cache = timeline.media_cache().clone();
    for file in files {
        cache.record(&file);
    }
    let timeline = timeline.with_media_cache(cache);
    let frame_rate = timeline.frame_rate();
    let size = RenderSize::from(timeline.resolution());
    let output_spec = VideoOutputSpec {
        width: size.width,
        height: size.height,
    }
    .validate()
    .map_err(ExportError::encoding)?;
    let audio_graph = AudioTimelineGraph::new(
        &timeline.visible_items(),
        timeline.media_cache(),
        frame_rate,
        EXPORT_AUDIO_FORMAT,
        &media_readers,
        AudioGainEvaluation::TimelineAnimation,
    )
    .map_err(ExportError::encoding)?;
    let video_frame_rate = VideoFrameRate::new(frame_rate.numerator(), frame_rate.denominator())
        .ok_or_else(|| ExportError::encoding("Invalid output frame rate"))?;
    if cancelled.load(Ordering::Relaxed) {
        return Err(ExportError::Cancelled);
    }
    let video_settings = VideoEncoderSettings {
        output: output_spec,
        frame_rate: video_frame_rate,
        gop: frame_rate
            .frames_per_second()
            .round()
            .clamp(1., f64::from(u32::MAX)) as u32,
        audio: (!audio_graph.is_empty()).then_some(EXPORT_AUDIO_FORMAT),
    };
    let decode_device = renderer.gpu().0.clone();
    let (mut frames, encoding) =
        ExportFramePipeline::new(renderer, size, EXPORT_PIPELINE_DEPTH, video_settings)?;
    thread::scope(|scope| {
        let (frames_to_encode, rendered_frames) = mpsc::sync_channel(EXPORT_PIPELINE_DEPTH);
        let encoder_failed = frames.failure_signal();
        let encoder_worker = thread::Builder::new()
            .name("zerium-export-encoder".to_owned())
            .spawn_scoped(scope, move || {
                let result = encode_frames(
                    output,
                    video_settings,
                    rendered_frames,
                    audio_graph,
                    frame_rate,
                    encoding,
                    cancelled,
                );
                if result.is_err() {
                    encoder_failed.store(true, Ordering::Release);
                }
                result
            })
            .map_err(|error| {
                ExportError::encoding(format!("Failed to start video encoding thread: {error}"))
            })?;
        // Scene evaluation, video decoding, and text rasterization run ahead on a
        // worker so the render thread only waits on the GPU, never on the CPU.
        let (decoded_scenes_tx, decoded_scenes_rx) = mpsc::sync_channel(DECODE_PREFETCH_FRAMES);
        let decode_worker = thread::Builder::new()
            .name("zerium-export-decode".to_owned())
            .spawn_scoped(scope, move || {
                decode_scenes(
                    timeline,
                    media_readers,
                    &decode_device,
                    size,
                    frame_count,
                    decoded_scenes_tx,
                    cancelled,
                )
            })
            .map_err(|error| {
                ExportError::encoding(format!("Failed to start video decoding thread: {error}"))
            })?;

        let render_result: Result<(), ExportError> = (|| {
            for _ in 0..frame_count {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(ExportError::Cancelled);
                }
                let (frame_index, scene) = decoded_scenes_rx.recv().map_err(|_| {
                    ExportError::encoding("Video decoding thread terminated unexpectedly")
                })??;
                if frames.is_full()
                    && let Some(frame) = frames.finish_next(cancelled)?
                {
                    send_frame(&frames_to_encode, frame)?;
                }
                frames.submit(frame_index, &scene, cancelled)?;
                let _ =
                    progress.unbounded_send(ExportProgress::Frame(frame_index.saturating_add(1)));
            }
            while let Some(frame) = frames.finish_next(cancelled)? {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(ExportError::Cancelled);
                }
                send_frame(&frames_to_encode, frame)?;
            }
            frames.finish()?;
            frames_to_encode
                .send(EncoderMessage::Complete)
                .map_err(|_| ExportError::encoding("Video encoder terminated unexpectedly"))?;
            Ok(())
        })();
        drop(decoded_scenes_rx);
        let decode_result = decode_worker.join();
        drop(frames_to_encode);
        let encoding_result = encoder_worker.join().unwrap_or_else(|_| {
            Err(ExportError::encoding(
                "Video encoding thread terminated unexpectedly",
            ))
        });
        // Cancellation also closes pipeline channels; report its cause rather
        // than the resulting disconnect errors from the other workers.
        if let Err(ExportError::Cancelled) = encoding_result {
            return Err(ExportError::Cancelled);
        }
        if let Err(error) = &encoding_result
            && !matches!(error, ExportError::Encoding(message) if message == "Export input closed before rendering completed")
        {
            return encoding_result;
        }
        render_result?;
        decode_result
            .map_err(|_| ExportError::encoding("Video decoding thread terminated unexpectedly"))?;
        encoding_result
    })
}

fn decode_scenes(
    timeline: TimelineSnapshot,
    media_readers: Arc<MediaReaderRegistry>,
    device: &wgpu::Device,
    size: RenderSize,
    frame_count: u64,
    scenes: mpsc::SyncSender<Result<(u64, RenderScene), ExportError>>,
    cancelled: &AtomicBool,
) {
    let composition_size = RenderSize::from(timeline.resolution());
    let mut decoders = HashMap::<MediaInputId, VisualDecoder>::new();
    let mut text_frames = TextFrameCache::new();
    for frame_index in 0..frame_count {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let frame = Frame::new(frame_index);
        let render_time = TimelineTime::from_frame(frame);
        let active_items = timeline.active_items_at(frame);
        text_frames.retain_active(active_items.iter().map(|(_, item)| item));
        let result = RenderScene::from_timeline(
            &timeline,
            render_time,
            size,
            |request| {
                let decoder = decoders.entry(request.input.clone()).or_default();
                let decoded = decoder.decode(&media_readers, &request, Some(device), cancelled)?;
                Ok::<_, ExportError>(Some(Arc::new(decoded.frame)))
            },
            |request| text_frames.frame_for(request, composition_size),
        )
        .map(|scene| (frame_index, scene));
        let failed = result.is_err();
        if scenes.send(result).is_err() || failed {
            break;
        }
    }
}

fn send_frame(
    sender: &mpsc::SyncSender<EncoderMessage>,
    frame: ExportFrame,
) -> Result<(), ExportError> {
    sender
        .send(EncoderMessage::Frame(frame))
        .map_err(|_| ExportError::encoding("Video encoder terminated unexpectedly"))
}

fn encode_frames(
    output: PathBuf,
    video_settings: VideoEncoderSettings,
    frames: mpsc::Receiver<EncoderMessage>,
    mut audio_graph: AudioTimelineGraph,
    frame_rate: FrameRate,
    encoding: VideoEncoding,
    cancelled: &AtomicBool,
) -> Result<(), ExportError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(ExportError::Cancelled);
    }
    let transaction = AtomicFileTransaction::new(&output).map_err(|error| {
        ExportError::encoding(format!(
            "Failed to create output temporary file '{}': {error}",
            output.display()
        ))
    })?;
    let has_audio = !audio_graph.is_empty();
    let mut encoder = FileEncoder::create(
        transaction.temporary_path(),
        video_settings,
        encoding,
        cancelled,
    )
    .map_err(ExportError::from)?;
    for message in frames {
        if cancelled.load(Ordering::Relaxed) {
            return Err(ExportError::Cancelled);
        }
        let EncoderMessage::Frame(frame) = message else {
            encoder.finish().map_err(ExportError::from)?;
            if cancelled.load(Ordering::Relaxed) {
                return Err(ExportError::Cancelled);
            }
            transaction.commit().map_err(|error| {
                ExportError::encoding(format!(
                    "Failed to finalize output '{}': {error}",
                    output.display()
                ))
            })?;
            return Ok(());
        };
        let index = frame.index;
        encoder.encode_frame(frame).map_err(ExportError::from)?;
        if has_audio {
            let start = sample_boundary(index, frame_rate, EXPORT_AUDIO_FORMAT.sample_rate);
            let end = sample_boundary(
                index.saturating_add(1),
                frame_rate,
                EXPORT_AUDIO_FORMAT.sample_rate,
            );
            let frame_count = usize::try_from(end.saturating_sub(start))
                .map_err(|_| ExportError::encoding("Export audio range is too large"))?;
            let audio = audio_graph
                .render(start, frame_count)
                .map_err(ExportError::encoding)?;
            encoder.encode_audio(&audio).map_err(ExportError::from)?;
        }
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err(ExportError::Cancelled);
    }
    Err(ExportError::encoding(
        "Export input closed before rendering completed",
    ))
}
