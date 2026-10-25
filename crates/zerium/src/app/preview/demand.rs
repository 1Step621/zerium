//! Demand, cache selection and publication policy, independent of decoder threads.
use super::PreviewFrameStatus;
use crate::engine::input::{
    DecodeStreamKey, DecodedFrame, MediaError, MediaFrameRequest, RasterSize, VisualFrame,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use zerium_core::media::{MediaAsset, MediaKind};

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

enum FrameFailure {
    /// A completed decode returned no interval covering the requested time.
    NoCoverage,
    Decode(MediaError),
}

pub(super) struct FrameDemand {
    ready_frames: HashMap<FrameSequence, Vec<DecodedFrame>>,
    requested_frames: HashMap<DecodeStreamKey, Vec<MediaFrameRequest>>,
    last_presented_frames: HashMap<MediaFrameRequest, Arc<VisualFrame>>,
    failed_frames: HashMap<(FrameSequence, Duration), FrameFailure>,
    notifications: Vec<String>,
    revision: u64,
}

impl FrameDemand {
    pub(super) fn new() -> Self {
        Self {
            ready_frames: HashMap::new(),
            requested_frames: HashMap::new(),
            last_presented_frames: HashMap::new(),
            failed_frames: HashMap::new(),
            notifications: Vec::new(),
            revision: 0,
        }
    }

    pub(super) fn begin_frame_demand(&mut self) {
        self.requested_frames.clear();
    }

    pub(super) fn request_frame(
        &mut self,
        request: &MediaFrameRequest,
    ) -> Option<Arc<VisualFrame>> {
        let stream = &request.stream;
        let requests = self.requested_frames.entry(stream.clone()).or_default();
        if !requests.contains(request) {
            requests.push(request.clone());
        }
        let sequence = FrameSequence::from_request(request);
        let Some((_, ready)) = self.frame_candidate(&sequence, request.presentation_time) else {
            if self.last_presented_frames.remove(request).is_some() {
                self.revision = self.revision.saturating_add(1);
            }
            return None;
        };
        let frame = ready.frame.clone();
        let key = request.clone();
        let changed = self
            .last_presented_frames
            .get(&key)
            .is_none_or(|presented| !Arc::ptr_eq(presented, &frame));
        self.last_presented_frames.insert(key, frame.clone());
        if changed {
            self.revision = self.revision.saturating_add(1);
        }
        Some(frame)
    }

    pub(super) fn needs_redraw(&self) -> bool {
        self.requested_frames.values().any(|requests| {
            requests.iter().any(|request| {
                let sequence = FrameSequence::from_request(request);
                let presented = self.last_presented_frames.get(request);
                match self.frame_candidate(&sequence, request.presentation_time) {
                    Some((_, ready)) => {
                        presented.is_none_or(|presented| !Arc::ptr_eq(presented, &ready.frame))
                    }
                    None => presented.is_some(),
                }
            })
        })
    }

    fn frame_for(&self, sequence: &FrameSequence, time: Duration) -> Option<&DecodedFrame> {
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
    ) -> Option<(&FrameSequence, &DecodedFrame)> {
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
        // while decoding. Temporal effects can demand several times per stream.
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

    pub(super) fn finish(&mut self) -> HashSet<DecodeStreamKey> {
        let active_streams = self
            .requested_frames
            .keys()
            .cloned()
            .collect::<HashSet<_>>();
        self.last_presented_frames.retain(|request, _| {
            self.requested_frames
                .get(&request.stream)
                .is_some_and(|requests| requests.contains(request))
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
        self.failed_frames.retain(|key, _| wanted.contains(key));
        for requests in self.requested_frames.values_mut() {
            requests.sort_by_key(|request| request.presentation_time);
        }

        self.retain_demanded_frames();
        active_streams
    }

    pub(super) fn snapshot(&mut self) -> PreviewFrameStatus {
        // Derive status from current demand. Another source succeeding must not
        // clear this source's failure, and unused failures must leave the UI.
        let error = self
            .requested_frames
            .values()
            .flatten()
            .filter_map(|request| {
                match self.failed_frames.get(&(
                    FrameSequence::from_request(request),
                    request.presentation_time,
                )) {
                    Some(FrameFailure::Decode(error)) => Some(error.to_string()),
                    _ => None,
                }
            })
            // Keep the displayed message deterministic when multiple sources fail.
            .min();
        PreviewFrameStatus {
            revision: self.revision,
            error,
            notifications: std::mem::take(&mut self.notifications),
        }
    }

    pub(super) fn next_request(&self, stream: &DecodeStreamKey) -> Option<MediaFrameRequest> {
        self.requested_frames
            .get(stream)
            .and_then(|requests| {
                requests.iter().find(|request| {
                    let sequence = FrameSequence::from_request(request);
                    !self
                        .frame_for(&sequence, request.presentation_time)
                        .is_some_and(|ready| ready.covers(request.presentation_time))
                        && !self
                            .failed_frames
                            .contains_key(&(sequence, request.presentation_time))
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
            .get(&request.stream)
            .is_some_and(|requests| requests.contains(&request));
        match result {
            Err(MediaError::Cancelled) => {}
            Ok(decoded) => {
                let covers = decoded.covers(presentation_time);
                // Streams share ready pixels. One decoded interval can satisfy
                // several failed source times, not just this worker's request.
                self.failed_frames.retain(|(failed_sequence, time), _| {
                    *failed_sequence != sequence || !decoded.covers(*time)
                });
                let frames = self.ready_frames.entry(sequence.clone()).or_default();
                frames.retain(|ready| ready.presentation_time != decoded.presentation_time);
                frames.push(decoded);
                self.retain_demanded_frames();
                if !covers {
                    self.failed_frames
                        .insert((sequence, presentation_time), FrameFailure::NoCoverage);
                }
            }
            Err(error) => {
                // Another stream may already have supplied the requested image
                // while this worker was decoding. A late failure cannot undo it.
                if self
                    .frame_for(&sequence, presentation_time)
                    .is_some_and(|ready| ready.covers(presentation_time))
                {
                    return;
                }
                if current {
                    self.notifications
                        .push(format!("Failed to load media frame: {error}"));
                }
                self.failed_frames
                    .insert((sequence, presentation_time), FrameFailure::Decode(error));
            }
        }
    }

    pub(super) fn reset(&mut self) {
        self.ready_frames.clear();
        self.requested_frames.clear();
        self.last_presented_frames.clear();
        self.failed_frames.clear();
        self.notifications.clear();
        self.revision = self.revision.saturating_add(1);
    }
}
