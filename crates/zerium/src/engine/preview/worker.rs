//! One persistent media decode worker per input, driven by latest scene demand.
use super::PreviewFrameEvent;
use crate::engine::{
    frame::{FrameData, VisualFrame},
    media::{MediaError, MediaFrameRequest, MediaReaderRegistry, VisualDecodeSession},
    rendering::RenderSession,
};
use futures::channel::mpsc::UnboundedSender;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) struct DecodeRequest {
    pub(super) generation: u64,
    pub(super) frame: MediaFrameRequest,
    pub(super) cancel: Arc<AtomicBool>,
}

pub(super) struct DecodeWorker {
    requests: std::sync::mpsc::SyncSender<DecodeRequest>,
    active: Option<DecodeRequest>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl DecodeWorker {
    pub(super) fn new(
        requests: std::sync::mpsc::SyncSender<DecodeRequest>,
        handle: std::thread::JoinHandle<()>,
    ) -> Self {
        Self {
            requests,
            active: None,
            handle: Some(handle),
        }
    }

    pub(super) fn is_pending(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn decode(&mut self, request: DecodeRequest) -> bool {
        debug_assert!(self.active.is_none());
        self.active = Some(DecodeRequest {
            generation: request.generation,
            frame: request.frame.clone(),
            cancel: request.cancel.clone(),
        });
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
        if let Some(active) = &self.active {
            active.cancel.store(true, Ordering::Release);
        }
    }
}

/// Publish exactly once, even when a decoder panics. The terminal event wakes
/// an unchanged preview so it cannot remain permanently marked as pending.
struct DecodeReply<'a> {
    pending: Option<PreviewFrameEvent>,
    events: &'a UnboundedSender<PreviewFrameEvent>,
}

impl DecodeReply<'_> {
    fn publish(&mut self, result: Result<crate::engine::media::DecodedFrame, MediaError>) {
        if let Some(mut event) = self.pending.take() {
            event.result = result;
            let _ = self.events.unbounded_send(event);
        }
    }
}

impl Drop for DecodeReply<'_> {
    fn drop(&mut self) {
        self.publish(Err(MediaError::WorkerStopped));
    }
}

pub(super) fn run_decode_worker(
    media_readers: Arc<MediaReaderRegistry>,
    renderer: Option<Arc<RenderSession>>,
    requests: std::sync::mpsc::Receiver<DecodeRequest>,
    events: UnboundedSender<PreviewFrameEvent>,
) {
    let mut decoder = VisualDecodeSession::default();
    while let Ok(request) = requests.recv() {
        let mut reply = DecodeReply {
            pending: Some(PreviewFrameEvent {
                generation: request.generation,
                request: request.frame.clone(),
                result: Err(MediaError::WorkerStopped),
            }),
            events: &events,
        };
        let result = decoder
            .decode(
                &media_readers,
                &request.frame,
                renderer.as_ref().map(|renderer| renderer.gpu()),
                &request.cancel,
            )
            .and_then(|decoded| {
                if let Some(renderer) = &renderer {
                    prepare_video_frame(renderer, &decoded.frame, &request.cancel)?;
                }
                Ok(decoded)
            });
        reply.publish(result);
        if events.is_closed() {
            break;
        }
    }
}

fn prepare_video_frame(
    renderer: &RenderSession,
    frame: &VisualFrame,
    cancelled: &AtomicBool,
) -> Result<(), MediaError> {
    let FrameData::Video(_) = frame.data else {
        return Ok(());
    };
    if cancelled.load(Ordering::Acquire) {
        return Err(MediaError::Cancelled);
    }
    renderer
        .prepare_video_frame(frame)
        .map_err(MediaError::external)?;
    let gpu = renderer.gpu();
    let device = &gpu.device;
    let queue = &gpu.queue;
    // Conversion and presentation use the same queue, so its ordering makes
    // the owned texture safe to publish before the GPU has finished writing it.
    queue.submit([]);
    device
        .poll(wgpu::PollType::Poll)
        .map_err(MediaError::external)?;
    Ok(())
}
