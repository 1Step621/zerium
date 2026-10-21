//! Demand, cache selection and publication policy, independent of decoder threads.
use super::{
    DecodedFrame, MediaAsset, MediaError, MediaFrameRequest, MediaInputId, MediaKind,
    PreviewFrameStatus, RasterSize, VisualFrame,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FrameSequence {
    source: MediaAsset,
    size: RasterSize,
}

impl FrameSequence {
    fn from_request(request: &MediaFrameRequest) -> Self {
        Self {
            source: request.asset.clone(),
            size: request.size,
        }
    }
}

struct ReadyFrame {
    presentation_time: Duration,
    duration: Duration,
    frame: Arc<VisualFrame>,
}

impl ReadyFrame {
    fn covers(&self, time: Duration) -> bool {
        crate::engine::media::frame_covers(self.presentation_time, self.duration, time)
    }
}

struct PresentedFrame {
    sequence: FrameSequence,
    presentation_time: Duration,
    frame: Arc<VisualFrame>,
}

impl PresentedFrame {
    fn matches(
        &self,
        sequence: &FrameSequence,
        presentation_time: Duration,
        frame: &Arc<VisualFrame>,
    ) -> bool {
        self.sequence == *sequence
            && self.presentation_time == presentation_time
            && Arc::ptr_eq(&self.frame, frame)
    }
}

pub(super) struct FrameDemand {
    ready_frames: HashMap<FrameSequence, Vec<ReadyFrame>>,
    requested_frames: HashMap<MediaInputId, Vec<MediaFrameRequest>>,
    last_presented_frames: HashMap<(MediaInputId, Duration), PresentedFrame>,
    failed_frames: HashSet<(FrameSequence, Duration)>,
    notifications: Vec<String>,
    revision: u64,
    error: Option<String>,
}

impl FrameDemand {
    pub(super) fn new() -> Self {
        Self {
            ready_frames: HashMap::new(),
            requested_frames: HashMap::new(),
            last_presented_frames: HashMap::new(),
            failed_frames: HashSet::new(),
            notifications: Vec::new(),
            revision: 0,
            error: None,
        }
    }

    pub(super) fn begin_frame_demand(&mut self) {
        self.requested_frames.clear();
    }

    pub(super) fn request_frame(
        &mut self,
        request: &MediaFrameRequest,
    ) -> Option<Arc<VisualFrame>> {
        let input = &request.input;
        let requests = self.requested_frames.entry(input.clone()).or_default();
        if !requests.contains(request) {
            requests.push(request.clone());
        }
        let sequence = FrameSequence::from_request(request);
        let (sequence, ready) = self.frame_candidate(&sequence, request.presentation_time)?;
        let sequence = sequence.clone();
        let presentation_time = ready.presentation_time;
        let frame = ready.frame.clone();
        let key = (input.clone(), request.presentation_time);
        let changed = self
            .last_presented_frames
            .get(&key)
            .is_none_or(|presented| !presented.matches(&sequence, presentation_time, &frame));
        self.last_presented_frames.insert(
            key,
            PresentedFrame {
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

    pub(super) fn needs_redraw(&self) -> bool {
        self.requested_frames.iter().any(|(input, requests)| {
            requests.iter().any(|request| {
                let sequence = FrameSequence::from_request(request);
                let presented = self
                    .last_presented_frames
                    .get(&(input.clone(), request.presentation_time));
                match self.frame_candidate(&sequence, request.presentation_time) {
                    Some((candidate, ready)) => presented.is_none_or(|presented| {
                        !presented.matches(candidate, ready.presentation_time, &ready.frame)
                    }),
                    None => presented.is_some(),
                }
            })
        })
    }

    fn frame_for(&self, sequence: &FrameSequence, time: Duration) -> Option<&ReadyFrame> {
        self.ready_frames.get(sequence)?.iter().min_by_key(|ready| {
            (
                !ready.covers(time),
                ready.presentation_time.abs_diff(time),
                ready.presentation_time,
            )
        })
    }

    fn frame_candidate(
        &self,
        sequence: &FrameSequence,
        time: Duration,
    ) -> Option<(&FrameSequence, &ReadyFrame)> {
        if let Some((candidate, _)) = self.ready_frames.get_key_value(sequence)
            && let Some(ready) = self.frame_for(candidate, time)
        {
            return Some((candidate, ready));
        }
        let MediaKind::Image { .. } = sequence.source.kind else {
            return None;
        };
        // A still image remains usable while its new raster size is prepared.
        // Decode scheduling still requires an exact size through frame_for().
        self.ready_frames
            .iter()
            .filter(|(candidate, _)| candidate.source == sequence.source)
            .flat_map(|(candidate, frames)| frames.iter().map(move |ready| (candidate, ready)))
            .min_by_key(|(candidate, ready)| {
                (
                    !ready.covers(time),
                    ready.presentation_time.abs_diff(time),
                    u64::from(candidate.size.max_width.abs_diff(sequence.size.max_width))
                        + u64::from(candidate.size.max_height.abs_diff(sequence.size.max_height)),
                    candidate.size.max_width,
                    candidate.size.max_height,
                )
            })
    }

    fn retain_demanded_frames(&mut self) {
        // Keep only the candidate for each visible request, including a fallback
        // while decoding. Temporal effects can demand several times per input.
        let mut retained = HashMap::<FrameSequence, HashSet<Duration>>::new();
        for request in self.requested_frames.values().flatten() {
            let sequence = FrameSequence::from_request(request);
            if let Some((candidate, ready)) =
                self.frame_candidate(&sequence, request.presentation_time)
            {
                retained
                    .entry(candidate.clone())
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

    pub(super) fn finish(&mut self) -> HashSet<MediaInputId> {
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
                            && (presented.sequence.size == request.size
                                || matches!(request.asset.kind, MediaKind::Image { .. }))
                            && *time == request.presentation_time
                    })
                })
            });
        let wanted = self
            .requested_frames
            .values()
            .flatten()
            .map(|request| {
                (
                    FrameSequence::from_request(request),
                    request.presentation_time,
                )
            })
            .collect::<HashSet<_>>();
        self.failed_frames.retain(|key| wanted.contains(key));
        for requests in self.requested_frames.values_mut() {
            requests.sort_by_key(|request| request.presentation_time);
        }

        self.retain_demanded_frames();
        active_inputs
    }

    pub(super) fn snapshot(&mut self) -> PreviewFrameStatus {
        PreviewFrameStatus {
            revision: self.revision,
            error: self.error.clone(),
            notifications: std::mem::take(&mut self.notifications),
        }
    }

    pub(super) fn next_request(&self, input: &MediaInputId) -> Option<MediaFrameRequest> {
        self.requested_frames
            .get(input)
            .and_then(|requests| {
                requests.iter().find(|request| {
                    let sequence = FrameSequence::from_request(request);
                    !self
                        .frame_for(&sequence, request.presentation_time)
                        .is_some_and(|ready| ready.covers(request.presentation_time))
                        && !self
                            .failed_frames
                            .contains(&(sequence, request.presentation_time))
                })
            })
            .cloned()
    }

    pub(super) fn publish(
        &mut self,
        request: MediaFrameRequest,
        result: Result<DecodedFrame, MediaError>,
    ) {
        let sequence = FrameSequence::from_request(&request);
        let presentation_time = request.presentation_time;
        let current = self
            .requested_frames
            .get(&request.input)
            .is_some_and(|requests| requests.contains(&request));
        match result {
            Err(MediaError::Cancelled) => {}
            Ok(decoded) => {
                let covers = decoded.covers(presentation_time);
                let frames = self.ready_frames.entry(sequence.clone()).or_default();
                frames.retain(|ready| ready.presentation_time != decoded.presentation_time);
                frames.push(ReadyFrame {
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
    }

    pub(super) fn reset(&mut self) {
        self.ready_frames.clear();
        self.requested_frames.clear();
        self.last_presented_frames.clear();
        self.failed_frames.clear();
        self.notifications.clear();
        self.error = None;
        self.revision = self.revision.saturating_add(1);
    }
}
