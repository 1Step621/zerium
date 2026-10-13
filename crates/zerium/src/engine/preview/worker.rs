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

struct PendingDecode {
    generation: u64,
    cancel: Arc<AtomicBool>,
}

pub(super) struct DecodeRequest {
    pub(super) generation: u64,
    pub(super) frame: MediaFrameRequest,
    pub(super) cancel: Arc<AtomicBool>,
}

pub(super) struct DecodeWorker {
    requests: std::sync::mpsc::SyncSender<DecodeRequest>,
    active: Option<PendingDecode>,
}

impl DecodeWorker {
    pub(super) fn new(requests: std::sync::mpsc::SyncSender<DecodeRequest>) -> Self {
        Self {
            requests,
            active: None,
        }
    }

    pub(super) fn is_pending(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn decode(&mut self, request: DecodeRequest) -> bool {
        debug_assert!(self.active.is_none());
        self.active = Some(PendingDecode {
            generation: request.generation,
            cancel: request.cancel.clone(),
        });
        self.requests.try_send(request).is_ok()
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

pub(super) fn run_decode_worker(
    media_readers: Arc<MediaReaderRegistry>,
    renderer: Option<Arc<RenderSession>>,
    requests: std::sync::mpsc::Receiver<DecodeRequest>,
    events: UnboundedSender<PreviewFrameEvent>,
) {
    let mut decoder = VisualDecodeSession::default();
    while let Ok(request) = requests.recv() {
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
        if events
            .unbounded_send(PreviewFrameEvent {
                generation: request.generation,
                request: request.frame,
                result,
            })
            .is_err()
        {
            break;
        }
    }
}

fn prepare_video_frame(
    renderer: &RenderSession,
    frame: &VisualFrame,
    cancelled: &AtomicBool,
) -> Result<(), MediaError> {
    if !matches!(frame.data, FrameData::Video(_)) {
        return Ok(());
    }
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
