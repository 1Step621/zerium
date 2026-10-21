mod demand;
mod worker;

use demand::FrameDemand;
use worker::{DecodeRequest, DecodeWorker, run_decode_worker};

use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicBool},
};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::engine::{
    frame::VisualFrame,
    media::{
        DecodedFrame, MediaError, MediaFrameRequest, MediaInputId, MediaReaderRegistry, RasterSize,
    },
    rendering::RenderSession,
};
use zerium_core::media::{MediaAsset, MediaKind};

pub(crate) struct PreviewFrameEvent {
    generation: u64,
    request: MediaFrameRequest,
    result: Result<DecodedFrame, MediaError>,
}

pub(crate) struct PreviewFrameStatus {
    pub(crate) revision: u64,
    pub(crate) error: Option<String>,
    pub(crate) notifications: Vec<String>,
}

pub(crate) struct PreviewFrameScheduler {
    media_readers: Arc<MediaReaderRegistry>,
    upload_renderer: Option<Arc<RenderSession>>,
    demand: FrameDemand,
    workers: HashMap<MediaInputId, DecodeWorker>,
    next_generation: u64,
    events: UnboundedSender<PreviewFrameEvent>,
}

impl PreviewFrameScheduler {
    pub(crate) fn new(
        media_readers: Arc<MediaReaderRegistry>,
        renderer: Option<Arc<RenderSession>>,
    ) -> (Self, UnboundedReceiver<PreviewFrameEvent>) {
        let (events, event_receiver) = unbounded();
        (
            Self {
                media_readers,
                // Upload scratch resources and locks are separate from UI rendering.
                upload_renderer: renderer.map(|renderer| Arc::new(renderer.fork())),
                demand: FrameDemand::new(),
                workers: HashMap::new(),
                next_generation: 0,
                events,
            },
            event_receiver,
        )
    }

    pub(crate) fn begin_frame_demand(&mut self) {
        self.demand.begin_frame_demand();
    }

    pub(crate) fn request_frame(
        &mut self,
        request: &MediaFrameRequest,
    ) -> Option<Arc<VisualFrame>> {
        self.demand.request_frame(request)
    }

    pub(crate) fn needs_redraw(&self) -> bool {
        self.demand.needs_redraw()
    }

    pub(crate) fn finish_frame_demand(&mut self) -> PreviewFrameStatus {
        let active = self.demand.finish();
        self.workers.retain(|input, _| active.contains(input));
        let stopped = self
            .workers
            .iter()
            .filter(|(_, worker)| worker.is_finished())
            .map(|(input, _)| input.clone())
            .collect::<Vec<_>>();
        for input in stopped {
            let mut worker = self.workers.remove(&input).expect("stopped worker");
            if let Some(request) = worker.finish_failure() {
                self.demand.publish(request, Err(MediaError::WorkerStopped));
            }
        }
        for input in active {
            self.decode_input_if_needed(&input);
        }
        self.demand.snapshot()
    }

    pub(crate) fn handle_event(&mut self, event: PreviewFrameEvent) -> PreviewFrameStatus {
        let input = event.request.input.clone();
        if self.finish_decode(event) {
            self.decode_input_if_needed(&input);
        }
        self.demand.snapshot()
    }

    fn worker_for(&mut self, input: &MediaInputId) -> Result<&mut DecodeWorker, MediaError> {
        use std::collections::hash_map::Entry;
        match self.workers.entry(input.clone()) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => {
                let (request_tx, request_rx) = std::sync::mpsc::sync_channel(1);
                let media_readers = self.media_readers.clone();
                let renderer = self.upload_renderer.clone();
                let events = self.events.clone();
                let thread = std::thread::Builder::new()
                    .name("zerium-video-worker".to_owned())
                    .spawn(move || run_decode_worker(media_readers, renderer, request_rx, events))
                    .map_err(|error| {
                        MediaError::External(format!("Failed to start video decoder: {error}"))
                    })?;
                Ok(entry.insert(DecodeWorker::new(request_tx, thread)))
            }
        }
    }

    fn decode_input_if_needed(&mut self, input: &MediaInputId) {
        // Keep one outstanding decode. Publish its result before scheduling from
        // the latest visible demand, without cancelling work on every movement.
        if self
            .workers
            .get(input)
            .is_some_and(|worker| worker.is_pending())
        {
            return;
        }
        let Some(requested) = self.demand.next_request(input) else {
            return;
        };
        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = self.next_generation;
        let worker = match self.worker_for(input) {
            Ok(worker) => worker,
            Err(error) => {
                self.demand.publish(requested, Err(error));
                return;
            }
        };
        if !worker.decode(DecodeRequest {
            generation,
            frame: requested.clone(),
            cancel: Arc::new(AtomicBool::new(false)),
        }) {
            self.workers.remove(input);
            self.demand
                .publish(requested, Err(MediaError::WorkerStopped));
        }
    }

    fn finish_decode(&mut self, event: PreviewFrameEvent) -> bool {
        let PreviewFrameEvent {
            generation,
            request,
            result,
        } = event;
        let input = &request.input;
        let Some(worker) = self.workers.get_mut(input) else {
            return false;
        };
        if !worker.complete(generation) {
            return false;
        }
        self.demand.publish(request, result);
        true
    }

    pub(crate) fn reset(&mut self) {
        self.workers.clear();
        self.demand.reset();
    }
}

impl Drop for PreviewFrameScheduler {
    fn drop(&mut self) {
        // Workers own their decoder and upload renderer. Cancellation closes the
        // request channel; they release those resources without blocking the UI.
        self.workers.clear();
    }
}
