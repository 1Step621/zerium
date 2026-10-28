//! One persistent media decode worker per consumer stream, driven by scene demand.
use super::PreviewFrameEvent;
use crate::engine::{
    input::{MediaError, MediaFrameRequest, VisualDecodeSession},
    processing::visual::VideoFrameUploader,
};
use futures::channel::mpsc::UnboundedSender;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
struct DecodeRequest {
    generation: u64,
    frame: MediaFrameRequest,
    cancel: Arc<AtomicBool>,
}

pub(super) struct DecodeWorker {
    requests: std::sync::mpsc::SyncSender<DecodeRequest>,
    active: Option<DecodeRequest>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl DecodeWorker {
    pub(super) fn spawn(
        uploader: Option<Arc<VideoFrameUploader>>,
        events: UnboundedSender<PreviewFrameEvent>,
    ) -> Result<Self, MediaError> {
        let (requests, receiver) = std::sync::mpsc::sync_channel(1);
        let handle = std::thread::Builder::new()
            .name("zerium-media-worker".to_owned())
            .spawn(move || run_decode_worker(uploader, receiver, events))
            .map_err(|error| {
                MediaError::external(format!("Failed to start media decoder: {error}"))
            })?;
        Ok(Self {
            requests,
            active: None,
            handle: Some(handle),
        })
    }

    pub(super) fn is_pending(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn decode(&mut self, generation: u64, frame: MediaFrameRequest) -> bool {
        debug_assert!(self.active.is_none());
        let request = DecodeRequest {
            generation,
            frame,
            cancel: Arc::new(AtomicBool::new(false)),
        };
        self.active = Some(request.clone());
        self.requests.try_send(request).is_ok()
    }

    pub(super) fn is_finished(&self) -> bool {
        self.handle
            .as_ref()
            .is_some_and(|handle| handle.is_finished())
    }

    pub(super) fn finish_failure(&mut self) -> Option<MediaFrameRequest> {
        debug_assert!(self.is_finished());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join(); // Already finished; never wait on the UI thread.
        }
        self.active.take().map(|active| active.frame)
    }

    pub(super) fn complete(&mut self, generation: u64) -> bool {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.generation == generation)
        {
            self.active = None;
            true
        } else {
            false
        }
    }
}

impl Drop for DecodeWorker {
    fn drop(&mut self) {
        // Closing the request channel releases the decoder on its own thread.
        // Cancellation never joins a live worker on the UI thread.
        if let Some(active) = &self.active {
            active.cancel.store(true, Ordering::Release);
        }
    }
}

/// Publish exactly once, even when a decoder panics. The terminal event wakes
/// an unchanged preview so it cannot remain permanently marked as pending.
struct DecodeReply<'a> {
    pending: Option<&'a DecodeRequest>,
    events: &'a UnboundedSender<PreviewFrameEvent>,
}

impl DecodeReply<'_> {
    fn publish(&mut self, result: Result<crate::engine::input::DecodedFrame, MediaError>) {
        if let Some(request) = self.pending.take() {
            let _ = self.events.unbounded_send(PreviewFrameEvent {
                generation: request.generation,
                request: request.frame.clone(),
                result,
            });
        }
    }
}

impl Drop for DecodeReply<'_> {
    fn drop(&mut self) {
        self.publish(Err(MediaError::WorkerStopped));
    }
}

fn run_decode_worker(
    uploader: Option<Arc<VideoFrameUploader>>,
    requests: std::sync::mpsc::Receiver<DecodeRequest>,
    events: UnboundedSender<PreviewFrameEvent>,
) {
    let mut decoder = VisualDecodeSession::default();
    while let Ok(request) = requests.recv() {
        let mut reply = DecodeReply {
            pending: Some(&request),
            events: &events,
        };
        let result = decoder
            .decode(
                &request.frame,
                uploader.as_ref().map(|uploader| uploader.gpu()),
                &request.cancel,
            )
            .and_then(|decoded| {
                if request.cancel.load(Ordering::Acquire) {
                    return Err(MediaError::Cancelled);
                }
                if let Some(uploader) = &uploader {
                    uploader
                        .prepare(&decoded.frame)
                        .map_err(MediaError::external)?;
                }
                Ok(decoded)
            });
        reply.publish(result);
        if events.is_closed() {
            break;
        }
    }
}
