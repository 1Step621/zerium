use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::engine::{
    frame::{FrameData, PixelFrame},
    media::{
        DecodedVideoFrame, MediaError, MediaFrameRequest, MediaInputId, MediaReaderRegistry,
        VideoDecodeSize, VisualDecoder,
    },
    rendering::FrameRenderer,
};
use zerium_core::media::MediaAsset;

struct InFlightVideoDecode {
    generation: u64,
    cancel: Arc<AtomicBool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct VideoFrameSequence {
    source: MediaAsset,
    size: VideoDecodeSize,
}

impl VideoFrameSequence {
    fn from_request(request: &MediaFrameRequest) -> Self {
        Self {
            source: request.asset.clone(),
            size: request.size,
        }
    }
}

struct ReadyVideoFrame {
    presentation_time: Duration,
    duration: Duration,
    frame: Arc<PixelFrame>,
}

impl ReadyVideoFrame {
    fn covers(&self, time: Duration) -> bool {
        let rounded = time.saturating_add(Duration::from_nanos(1));
        (self.presentation_time <= rounded
            && rounded < self.presentation_time.saturating_add(self.duration))
            || (self.duration.is_zero() && time == self.presentation_time)
    }
}

struct PresentedVideoFrame {
    sequence: VideoFrameSequence,
    presentation_time: Duration,
    frame: Arc<PixelFrame>,
}

impl PresentedVideoFrame {
    fn matches(
        &self,
        sequence: &VideoFrameSequence,
        presentation_time: Duration,
        frame: &Arc<PixelFrame>,
    ) -> bool {
        self.sequence == *sequence
            && self.presentation_time == presentation_time
            && Arc::ptr_eq(&self.frame, frame)
    }
}

struct VideoWorkerRequest {
    generation: u64,
    frame: MediaFrameRequest,
    cancel: Arc<AtomicBool>,
}

struct VideoWorker {
    requests: std::sync::mpsc::SyncSender<VideoWorkerRequest>,
    active: Option<InFlightVideoDecode>,
}

impl VideoWorker {
    fn decode(&mut self, request: VideoWorkerRequest) -> bool {
        debug_assert!(self.active.is_none());
        self.active = Some(InFlightVideoDecode {
            generation: request.generation,
            cancel: request.cancel.clone(),
        });
        self.requests.try_send(request).is_ok()
    }

    fn complete(&mut self, generation: u64) -> bool {
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

impl Drop for VideoWorker {
    fn drop(&mut self) {
        if let Some(active) = &self.active {
            active.cancel.store(true, Ordering::Release);
        }
    }
}

pub(crate) struct VideoPlaybackEvent {
    generation: u64,
    request: MediaFrameRequest,
    result: Result<DecodedVideoFrame, MediaError>,
}

fn video_worker_main(
    media_readers: Arc<MediaReaderRegistry>,
    renderer: Option<Arc<FrameRenderer>>,
    requests: std::sync::mpsc::Receiver<VideoWorkerRequest>,
    events: UnboundedSender<VideoPlaybackEvent>,
) {
    let mut decoder = VisualDecoder::default();
    while let Ok(request) = requests.recv() {
        let result = decoder
            .decode(
                &media_readers,
                &request.frame,
                renderer.as_ref().map(|renderer| renderer.gpu().0),
                &request.cancel,
            )
            .and_then(|decoded| {
                if let Some(renderer) = &renderer {
                    prepare_video_frame(renderer, &decoded.frame, &request.cancel)?;
                }
                Ok(decoded)
            });
        if events
            .unbounded_send(VideoPlaybackEvent {
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
    renderer: &FrameRenderer,
    frame: &PixelFrame,
    cancelled: &AtomicBool,
) -> Result<(), MediaError> {
    if !matches!(frame.data, FrameData::Video(_)) {
        return Ok(());
    }
    if cancelled.load(Ordering::Acquire) {
        return Err(MediaError::Cancelled);
    }
    renderer
        .retain_video_frame(frame)
        .map_err(MediaError::external)?;
    let (device, queue) = renderer.gpu();
    // Flush uploads and release decoder surfaces on this worker, even when the
    // window is not rendering or polling its device.
    let submission = queue.submit([]);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(MediaError::Cancelled);
        }
        match device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission.clone()),
            timeout: Some(Duration::from_millis(10)),
        }) {
            Ok(_) => return Ok(()),
            Err(wgpu::PollError::Timeout) if Instant::now() < deadline => {}
            Err(error) => return Err(MediaError::external(error)),
        }
    }
}

pub(crate) struct VideoPlaybackSnapshot {
    pub(crate) revision: u64,
    pub(crate) error: Option<String>,
    pub(crate) notifications: Vec<String>,
}

pub(crate) struct VideoPlaybackEngine {
    media_readers: Arc<MediaReaderRegistry>,
    upload_renderer: Option<Arc<FrameRenderer>>,
    ready_frames: HashMap<VideoFrameSequence, Vec<ReadyVideoFrame>>,
    requested_frames: HashMap<MediaInputId, Vec<MediaFrameRequest>>,
    last_presented_frames: HashMap<(MediaInputId, Duration), PresentedVideoFrame>,
    workers: HashMap<MediaInputId, VideoWorker>,
    worker_threads: Vec<std::thread::JoinHandle<()>>,
    next_generation: u64,
    events: UnboundedSender<VideoPlaybackEvent>,
    failed_frames: HashSet<(VideoFrameSequence, Duration)>,
    notifications: Vec<String>,
    revision: u64,
    error: Option<String>,
}

impl VideoPlaybackEngine {
    pub(crate) fn new(
        media_readers: Arc<MediaReaderRegistry>,
        renderer: Option<Arc<FrameRenderer>>,
    ) -> (Self, UnboundedReceiver<VideoPlaybackEvent>) {
        let (events, event_receiver) = unbounded();
        (
            Self {
                media_readers,
                // Upload scratch resources and locks are separate from UI rendering.
                upload_renderer: renderer.map(|renderer| Arc::new(renderer.fork())),
                ready_frames: HashMap::new(),
                requested_frames: HashMap::new(),
                last_presented_frames: HashMap::new(),
                workers: HashMap::new(),
                worker_threads: Vec::new(),
                next_generation: 0,
                events,
                failed_frames: HashSet::new(),
                notifications: Vec::new(),
                revision: 0,
                error: None,
            },
            event_receiver,
        )
    }

    pub(crate) fn begin_frame_demand(&mut self) {
        self.requested_frames.clear();
    }

    pub(crate) fn request_frame(&mut self, request: &MediaFrameRequest) -> Option<Arc<PixelFrame>> {
        let input = &request.input;
        let requests = self.requested_frames.entry(input.clone()).or_default();
        if !requests.contains(request) {
            requests.push(request.clone());
        }
        let sequence = VideoFrameSequence::from_request(request);
        let ready = self.frame_for(&sequence, request.presentation_time)?;
        let presentation_time = ready.presentation_time;
        let frame = ready.frame.clone();
        let key = (input.clone(), request.presentation_time);
        let changed = self
            .last_presented_frames
            .get(&key)
            .is_none_or(|presented| !presented.matches(&sequence, presentation_time, &frame));
        self.last_presented_frames.insert(
            key,
            PresentedVideoFrame {
                sequence,
                presentation_time,
                frame: frame.clone(),
            },
        );
        if changed {
            self.revision = self.revision.saturating_add(1);
        }
        Some(frame)
    }

    pub(crate) fn needs_redraw(&self) -> bool {
        self.requested_frames.iter().any(|(input, requests)| {
            requests.iter().any(|request| {
                let sequence = VideoFrameSequence::from_request(request);
                let presented = self
                    .last_presented_frames
                    .get(&(input.clone(), request.presentation_time));
                match self.frame_for(&sequence, request.presentation_time) {
                    Some(ready) => presented.is_none_or(|presented| {
                        !presented.matches(&sequence, ready.presentation_time, &ready.frame)
                    }),
                    None => presented.is_some(),
                }
            })
        })
    }

    fn frame_for(&self, sequence: &VideoFrameSequence, time: Duration) -> Option<&ReadyVideoFrame> {
        self.ready_frames.get(sequence)?.iter().min_by_key(|ready| {
            (
                !ready.covers(time),
                ready.presentation_time.abs_diff(time),
                ready.presentation_time,
            )
        })
    }

    fn retain_demanded_frames(&mut self) {
        // Keep only the candidate for each visible request, including a fallback
        // while decoding. Temporal effects can demand several times per input.
        let mut retained = HashMap::<VideoFrameSequence, HashSet<Duration>>::new();
        for request in self.requested_frames.values().flatten() {
            let sequence = VideoFrameSequence::from_request(request);
            if let Some(ready) = self.frame_for(&sequence, request.presentation_time) {
                retained
                    .entry(sequence)
                    .or_default()
                    .insert(ready.presentation_time);
            }
        }
        self.ready_frames.retain(|sequence, frames| {
            let Some(times) = retained.get(sequence) else {
                return false;
            };
            frames.retain(|ready| times.contains(&ready.presentation_time));
            !frames.is_empty()
        });
    }

    pub(crate) fn finish_frame_demand(&mut self) -> VideoPlaybackSnapshot {
        let active_inputs = self
            .requested_frames
            .keys()
            .cloned()
            .collect::<HashSet<_>>();
        self.last_presented_frames
            .retain(|(input, time), presented| {
                self.requested_frames.get(input).is_some_and(|requests| {
                    requests.iter().any(|request| {
                        presented.sequence.source == request.asset
                            && presented.sequence.size == request.size
                            && *time == request.presentation_time
                    })
                })
            });
        self.workers
            .retain(|input, _| active_inputs.contains(input));
        self.worker_threads.retain(|thread| !thread.is_finished());
        let wanted = self
            .requested_frames
            .values()
            .flatten()
            .map(|request| {
                (
                    VideoFrameSequence::from_request(request),
                    request.presentation_time,
                )
            })
            .collect::<HashSet<_>>();
        self.failed_frames.retain(|key| wanted.contains(key));
        for requests in self.requested_frames.values_mut() {
            requests.sort_by_key(|request| request.presentation_time);
        }

        self.retain_demanded_frames();
        for input in active_inputs {
            self.decode_input_if_needed(&input);
        }
        self.snapshot()
    }

    pub(crate) fn handle_event(&mut self, event: VideoPlaybackEvent) -> VideoPlaybackSnapshot {
        let input = event.request.input.clone();
        if self.finish_decode(event) {
            self.decode_input_if_needed(&input);
        }
        self.snapshot()
    }

    fn snapshot(&mut self) -> VideoPlaybackSnapshot {
        VideoPlaybackSnapshot {
            revision: self.revision,
            error: self.error.clone(),
            notifications: std::mem::take(&mut self.notifications),
        }
    }

    fn worker_for(&mut self, input: &MediaInputId) -> Option<&mut VideoWorker> {
        use std::collections::hash_map::Entry;
        match self.workers.entry(input.clone()) {
            Entry::Occupied(entry) => Some(entry.into_mut()),
            Entry::Vacant(entry) => {
                let (request_tx, request_rx) = std::sync::mpsc::sync_channel(1);
                let media_readers = self.media_readers.clone();
                let renderer = self.upload_renderer.clone();
                let events = self.events.clone();
                let thread = std::thread::Builder::new()
                    .name("zerium-video-worker".to_owned())
                    .spawn(move || video_worker_main(media_readers, renderer, request_rx, events))
                    .ok()?;
                self.worker_threads.push(thread);
                Some(entry.insert(VideoWorker {
                    requests: request_tx,
                    active: None,
                }))
            }
        }
    }

    fn decode_input_if_needed(&mut self, input: &MediaInputId) {
        // Keep one outstanding decode. Publish its result before scheduling from
        // the latest visible demand, without cancelling work on every movement.
        if self
            .workers
            .get(input)
            .is_some_and(|worker| worker.active.is_some())
        {
            return;
        }
        let Some(requested) = self
            .requested_frames
            .get(input)
            .and_then(|requests| {
                requests.iter().find(|request| {
                    let sequence = VideoFrameSequence::from_request(request);
                    !self
                        .frame_for(&sequence, request.presentation_time)
                        .is_some_and(|ready| ready.covers(request.presentation_time))
                        && !self
                            .failed_frames
                            .contains(&(sequence, request.presentation_time))
                })
            })
            .cloned()
        else {
            return;
        };
        let sequence = VideoFrameSequence::from_request(&requested);
        let presentation_time = requested.presentation_time;
        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = self.next_generation;
        let Some(worker) = self.worker_for(input) else {
            self.failed_frames.insert((sequence, presentation_time));
            return;
        };
        if !worker.decode(VideoWorkerRequest {
            generation,
            frame: requested,
            cancel: Arc::new(AtomicBool::new(false)),
        }) {
            self.workers.remove(input);
        }
    }

    fn finish_decode(&mut self, event: VideoPlaybackEvent) -> bool {
        let VideoPlaybackEvent {
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
        let sequence = VideoFrameSequence::from_request(&request);
        let presentation_time = request.presentation_time;
        let current = self
            .requested_frames
            .get(input)
            .is_some_and(|requests| requests.contains(&request));
        match result {
            Err(MediaError::Cancelled) => {}
            Ok(decoded) => {
                let covers = decoded.covers(presentation_time);
                let frames = self.ready_frames.entry(sequence.clone()).or_default();
                frames.retain(|ready| ready.presentation_time != decoded.presentation_time);
                frames.push(ReadyVideoFrame {
                    presentation_time: decoded.presentation_time,
                    duration: decoded.duration,
                    frame: Arc::new(decoded.frame),
                });
                self.retain_demanded_frames();
                if covers {
                    self.failed_frames
                        .remove(&(sequence.clone(), presentation_time));
                } else {
                    self.failed_frames
                        .insert((sequence.clone(), presentation_time));
                }
                if current {
                    self.error = None;
                }
            }
            Err(error) => {
                self.failed_frames
                    .insert((sequence.clone(), presentation_time));
                if current {
                    self.error = Some(error.to_string());
                    self.notifications
                        .push(format!("Failed to load video frame: {error}"));
                }
            }
        }
        true
    }

    pub(crate) fn reset(&mut self) {
        self.workers.clear();
        self.ready_frames.clear();
        self.requested_frames.clear();
        self.last_presented_frames.clear();
        self.failed_frames.clear();
        self.notifications.clear();
        self.error = None;
        self.revision = self.revision.saturating_add(1);
    }
}

impl Drop for VideoPlaybackEngine {
    fn drop(&mut self) {
        // Close demand channels and cancel lifecycle work before the preview
        // GPU is destroyed. Input changes/reset still retire workers asynchronously.
        self.workers.clear();
        for thread in self.worker_threads.drain(..) {
            let _ = thread.join();
        }
    }
}
