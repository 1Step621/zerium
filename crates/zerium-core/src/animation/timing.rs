//! Pattern time and playback time share one clock, independent of rendering.
use crate::timeline::{Frame, FrameDuration, TimelineTime};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    #[default]
    None,
    Loop,
    PingPong,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AnimationRepeat {
    mode: RepeatMode,
    period: f32,
    phase: f32,
}

impl Default for AnimationRepeat {
    fn default() -> Self {
        Self {
            mode: RepeatMode::None,
            period: 30.,
            phase: 0.,
        }
    }
}

impl AnimationRepeat {
    /// Period is one complete cycle in frames; phase is measured in cycles.
    pub fn new(mode: RepeatMode, period: f32, phase: f32) -> Option<Self> {
        if !period.is_finite() || period < 1. || !phase.is_finite() {
            return None;
        }
        Some(Self {
            mode,
            period,
            phase: phase.rem_euclid(1.),
        })
    }

    pub fn with_mode(self, mode: RepeatMode) -> Self {
        Self { mode, ..self }
    }

    pub fn mode(self) -> RepeatMode {
        self.mode
    }

    pub fn period(self) -> f32 {
        self.period
    }

    pub fn phase(self) -> f32 {
        self.phase
    }

    pub(super) fn shifted(self, frames: f64) -> Self {
        Self::new(
            self.mode,
            self.period,
            ((f64::from(self.phase) + frames / f64::from(self.period)).rem_euclid(1.)) as f32,
        )
        .expect("a finite time offset preserves valid repeat settings")
    }

    pub(super) fn scaled(self, factor: f32) -> Self {
        Self::new(
            self.mode,
            (f64::from(self.period) * f64::from(factor)).clamp(1., f64::from(f32::MAX)) as f32,
            self.phase,
        )
        .expect("a positive finite scale preserves valid repeat settings")
    }

    pub(super) fn is_valid(self) -> bool {
        Self::new(self.mode, self.period, self.phase) == Some(self)
    }
}

/// The editable forward pattern has an affine time axis. Playback folds that
/// axis for looping or ping-pong, without duplicating any stops.
#[derive(Clone, Copy, Debug)]
pub struct AnimationClock {
    start: f64,
    span_frames: f64,
    repeat: RepeatMode,
}

impl AnimationClock {
    pub fn new(start: Frame, duration: FrameDuration, repeat: AnimationRepeat) -> Self {
        let period = f64::from(repeat.period);
        let phase_offset = f64::from(repeat.phase) * period;
        let (span_frames, offset) = match repeat.mode {
            RepeatMode::None => (duration.get().saturating_sub(1).max(1) as f64, 0.),
            RepeatMode::Loop => (period, phase_offset),
            RepeatMode::PingPong => (period / 2., phase_offset),
        };
        Self {
            start: start.get() as f64 - offset,
            span_frames,
            repeat: repeat.mode,
        }
    }

    pub fn start_time(self) -> TimelineTime {
        TimelineTime::from_frames(self.start)
    }

    pub fn span_frames(self) -> f64 {
        self.span_frames
    }

    pub fn pattern_progress_at(self, time: TimelineTime) -> f32 {
        ((time.frames() - self.start) / self.span_frames).clamp(0., 1.) as f32
    }

    pub fn progress_at(self, time: TimelineTime) -> f32 {
        let progress = (time.frames() - self.start) / self.span_frames;
        (match self.repeat {
            RepeatMode::None => progress.clamp(0., 1.),
            RepeatMode::Loop => progress.rem_euclid(1.),
            RepeatMode::PingPong => {
                let progress = progress.rem_euclid(2.);
                progress.min(2. - progress)
            }
        }) as f32
    }

    fn occurrence_origins(self, progress: f32) -> impl Iterator<Item = f64> {
        let reverse = (self.repeat == RepeatMode::PingPong && progress > 0. && progress < 1.)
            .then(|| self.start + (2. - f64::from(progress)) * self.span_frames);
        std::iter::once(self.time_at(progress).frames()).chain(reverse)
    }

    fn cycle_length(self) -> Option<f64> {
        match self.repeat {
            RepeatMode::None => None,
            RepeatMode::Loop => Some(self.span_frames),
            RepeatMode::PingPong => Some(self.span_frames * 2.),
        }
    }

    fn occurrence_series(
        self,
        progress: f32,
        start: TimelineTime,
        end: TimelineTime,
    ) -> impl Iterator<Item = (TimelineTime, f64, usize)> {
        let period = self.cycle_length();
        self.occurrence_origins(progress).filter_map(move |origin| {
            if start > end {
                return None;
            }
            match period {
                Some(period) => {
                    let first = ((start.frames() - origin) / period).ceil();
                    let last = ((end.frames() - origin) / period).floor();
                    (first <= last).then(|| {
                        (
                            TimelineTime::from_frames(origin + first * period),
                            period,
                            (last - first + 1.) as usize,
                        )
                    })
                }
                None => (origin >= start.frames() && origin <= end.frames()).then_some((
                    TimelineTime::from_frames(origin),
                    0.,
                    1,
                )),
            }
        })
    }

    /// All occurrences of a pattern point in a visible timeline interval.
    pub fn occurrences(
        self,
        progress: f32,
        start: TimelineTime,
        end: TimelineTime,
    ) -> impl Iterator<Item = TimelineTime> {
        self.occurrence_series(progress, start, end)
            .flat_map(|(first, period, count)| {
                (0..count).map(move |cycle| first.offset(cycle as f64 * period))
            })
    }

    /// The first visible occurrence of a pattern position, including the
    /// return leg of a ping-pong cycle.
    pub fn first_occurrence(
        self,
        progress: f32,
        start: TimelineTime,
        end: TimelineTime,
    ) -> Option<TimelineTime> {
        self.occurrence_series(progress, start, end)
            .map(|(first, _, _)| first)
            .min_by(|left, right| left.frames().total_cmp(&right.frames()))
    }

    pub fn time_at(self, progress: f32) -> TimelineTime {
        TimelineTime::from_frames(self.start + f64::from(progress.clamp(0., 1.)) * self.span_frames)
    }
}
