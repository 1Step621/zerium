//! Preview demand and decode-worker scheduling for the application.
mod demand;
mod worker;

use self::{demand::FrameDemand, worker::DecodeWorker};

use std::{collections::HashMap, sync::Arc};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::engine::{
    input::{DecodeStreamKey, DecodedFrame, MediaError, MediaFrameRequest, VisualFrame},
    processing::visual::VideoFrameUploader,
};

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
    uploader: Option<Arc<VideoFrameUploader>>,
    demand: FrameDemand,
    workers: HashMap<DecodeStreamKey, DecodeWorker>,
    next_generation: u64,
    events: UnboundedSender<PreviewFrameEvent>,
}

impl PreviewFrameScheduler {
    pub(crate) fn new(
        uploader: Option<VideoFrameUploader>,
    ) -> (Self, UnboundedReceiver<PreviewFrameEvent>) {
        let (events, event_receiver) = unbounded();
        (
            Self {
                uploader: uploader.map(Arc::new),
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
        self.workers.retain(|stream, _| active.contains(stream));
        let stopped = self
            .workers
            .iter()
            .filter(|(_, worker)| worker.is_finished())
            .map(|(stream, _)| stream.clone())
            .collect::<Vec<_>>();
        for stream in stopped {
            let mut worker = self.workers.remove(&stream).expect("stopped worker");
            if let Some(request) = worker.finish_failure() {
                self.demand.publish(request, Err(MediaError::WorkerStopped));
            }
        }
        for stream in active {
            self.decode_stream_if_needed(&stream);
        }
        self.demand.snapshot()
    }

    pub(crate) fn handle_event(&mut self, event: PreviewFrameEvent) -> PreviewFrameStatus {
        let stream = event.request.stream.clone();
        if self.finish_decode(event) {
            self.decode_stream_if_needed(&stream);
        }
        self.demand.snapshot()
    }

    fn worker_for(&mut self, stream: &DecodeStreamKey) -> Result<&mut DecodeWorker, MediaError> {
        use std::collections::hash_map::Entry;
        match self.workers.entry(stream.clone()) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => Ok(entry.insert(DecodeWorker::spawn(
                self.uploader.clone(),
                self.events.clone(),
            )?)),
        }
    }

    fn decode_stream_if_needed(&mut self, stream: &DecodeStreamKey) {
        // Keep one outstanding decode. Publish its result before scheduling from
        // the latest visible demand, without cancelling work on every movement.
        if self
            .workers
            .get(stream)
            .is_some_and(|worker| worker.is_pending())
        {
            return;
        }
        let Some(requested) = self.demand.next_request(stream) else {
            return;
        };
        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = self.next_generation;
        let worker = match self.worker_for(stream) {
            Ok(worker) => worker,
            Err(error) => {
                self.demand.publish(requested, Err(error));
                return;
            }
        };
        if !worker.decode(generation, requested.clone()) {
            self.workers.remove(stream);
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
        let stream = &request.stream;
        let Some(worker) = self.workers.get_mut(stream) else {
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

    pub(crate) fn set_uploader(&mut self, uploader: Option<VideoFrameUploader>) {
        self.reset();
        self.uploader = uploader.map(Arc::new);
    }
}
