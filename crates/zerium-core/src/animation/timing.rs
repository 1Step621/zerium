//! Integer authored clocks and continuous playback sampling.
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

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct AnimationRepeat {
    mode: RepeatMode,
    period: FrameDuration,
    phase_frames: Frame,
}

impl<'de> Deserialize<'de> for AnimationRepeat {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Settings {
            mode: RepeatMode,
            period: FrameDuration,
            phase_frames: Frame,
        }
        let settings = Settings::deserialize(deserializer)?;
        let repeat = Self::new(settings.mode, settings.period, settings.phase_frames);
        if repeat.period != settings.period || repeat.phase_frames != settings.phase_frames {
            return Err(serde::de::Error::custom(
                "invalid animation repeat frame settings",
            ));
        }
        Ok(repeat)
    }
}

impl AnimationRepeat {
    /// Phase is an integer frame offset. Ping-pong periods round up to an
    /// even frame count so both turnaround keys occupy whole frames.
    pub fn new(mode: RepeatMode, period: FrameDuration, phase: Frame) -> Self {
        let mut period = period.get();
        if mode == RepeatMode::PingPong && !period.is_multiple_of(2) {
            period = period.checked_add(1).unwrap_or(period - 1);
        }
        Self {
            mode,
            period: FrameDuration::new(period).expect("positive period"),
            phase_frames: Frame::new(phase.get() % period),
        }
    }

    pub fn with_mode(self, mode: RepeatMode) -> Self {
        Self::new(mode, self.period, self.phase_frames)
    }

    pub fn mode(self) -> RepeatMode {
        self.mode
    }

    pub fn period(self) -> FrameDuration {
        self.period
    }

    pub fn phase(self) -> Frame {
        self.phase_frames
    }

    pub(crate) fn pattern_span(self, duration: FrameDuration) -> FrameDuration {
        match self.mode {
            RepeatMode::None => duration.animation_span(),
            RepeatMode::Loop => self.period,
            RepeatMode::PingPong => FrameDuration::new(self.period.get() / 2).expect("even period"),
        }
    }

    pub(super) fn shifted(self, frames: i128) -> Self {
        let phase = (i128::from(self.phase_frames.get()) + frames)
            .rem_euclid(i128::from(self.period.get())) as u64;
        Self::new(self.mode, self.period, Frame::new(phase))
    }

    pub(super) fn scaled(self, numerator: u64, denominator: u64) -> Self {
        let period =
            FrameDuration::new_saturating(scale_frames(self.period.get(), numerator, denominator));
        let repeat = Self::new(self.mode, period, Frame::new(0));
        let phase = scale_frames(
            self.phase_frames.get(),
            repeat.period.get(),
            self.period.get(),
        );
        Self::new(self.mode, repeat.period, Frame::new(phase))
    }
}

/// Round a rational frame conversion without passing authored times through floats.
pub(super) fn scale_frames(value: u64, numerator: u64, denominator: u64) -> u64 {
    ((u128::from(value) * u128::from(numerator) + u128::from(denominator) / 2)
        / u128::from(denominator))
    .min(u128::from(u64::MAX)) as u64
}

/// The authored forward pattern uses integer frames. Playback can sample any
/// continuous local frame, including the return leg of a ping-pong cycle.
#[derive(Clone, Copy, Debug)]
pub struct AnimationClock {
    start: i128,
    span: FrameDuration,
    repeat: AnimationRepeat,
}

impl AnimationClock {
    pub(crate) fn new(start: Frame, track: &super::ScalarTrack) -> Self {
        let span = track.span();
        let repeat = track.repeat();
        let offset = if repeat.mode == RepeatMode::None {
            0
        } else {
            repeat.phase_frames.get()
        };
        Self {
            start: i128::from(start.get()) - i128::from(offset),
            span,
            repeat,
        }
    }

    pub fn start_time(self) -> TimelineTime {
        TimelineTime::from_frames(self.start as f64)
    }

    pub fn span_frames(self) -> f64 {
        self.span.get() as f64
    }

    /// Quantize a pointer/input position once at the editing boundary.
    pub fn pattern_frame_at(self, time: TimelineTime) -> Frame {
        Frame::new(
            (time.frames() - self.start as f64)
                .round()
                .clamp(0., self.span_frames()) as u64,
        )
    }

    pub fn frame_at_progress(self, progress: f64) -> Frame {
        Frame::new((progress.clamp(0., 1.) * self.span_frames()).round() as u64)
    }

    pub fn local_frame_at(self, time: TimelineTime) -> f64 {
        let local = time.frames() - self.start as f64;
        let span = self.span_frames();
        match self.repeat.mode {
            RepeatMode::None => local.clamp(0., span),
            RepeatMode::Loop => local.rem_euclid(span),
            RepeatMode::PingPong => {
                let local = local.rem_euclid(self.repeat.period.get() as f64);
                local.min(span * 2. - local)
            }
        }
    }

    fn occurrence_origins(self, frame: Frame) -> impl Iterator<Item = i128> {
        let forward = self.start + i128::from(frame.get());
        let reverse = (self.repeat.mode == RepeatMode::PingPong
            && frame.get() > 0
            && frame.get() < self.span.get())
        .then(|| self.start + i128::from(self.repeat.period.get()) - i128::from(frame.get()));
        std::iter::once(forward).chain(reverse)
    }

    fn occurrence_series(
        self,
        frame: Frame,
        start: TimelineTime,
        end: TimelineTime,
    ) -> impl Iterator<Item = (TimelineTime, f64, usize)> {
        let period =
            (self.repeat.mode != RepeatMode::None).then_some(self.repeat.period.get() as f64);
        self.occurrence_origins(frame).filter_map(move |origin| {
            if start > end {
                return None;
            }
            let origin = origin as f64;
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

    pub fn occurrences(
        self,
        frame: Frame,
        start: TimelineTime,
        end: TimelineTime,
    ) -> impl Iterator<Item = TimelineTime> {
        self.occurrence_series(frame, start, end)
            .flat_map(|(first, period, count)| {
                (0..count).map(move |cycle| first.offset(cycle as f64 * period))
            })
    }

    pub fn first_occurrence(
        self,
        frame: Frame,
        start: TimelineTime,
        end: TimelineTime,
    ) -> Option<TimelineTime> {
        self.occurrence_series(frame, start, end)
            .map(|(first, _, _)| first)
            .min_by(|left, right| left.frames().total_cmp(&right.frames()))
    }

    pub fn key_time_at(self, frame: Frame) -> TimelineTime {
        TimelineTime::from_frames((self.start + i128::from(frame.get())) as f64)
    }
}
