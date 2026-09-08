use std::{num::NonZeroU64, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Frame(pub(super) u64);

impl Frame {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

/// Continuous position on the timeline, measured in frames.
///
/// Editing remains frame-based, while rendering can evaluate animation between
/// frame boundaries for temporal effects such as motion blur.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub(crate) struct TimelineTime(f64);

impl TimelineTime {
    pub(crate) fn from_frames(frames: f64) -> Self {
        Self(if frames.is_finite() { frames } else { 0. })
    }

    pub(crate) fn from_frame(frame: Frame) -> Self {
        Self(frame.get() as f64)
    }

    pub(crate) fn from_seconds(seconds: f64, frame_rate: FrameRate) -> Option<Self> {
        if !seconds.is_finite() || seconds < 0. {
            return None;
        }
        Some(Self::from_frames(seconds * frame_rate.frames_per_second()))
    }

    pub(crate) fn frames(self) -> f64 {
        self.0
    }

    pub(crate) fn seconds(self, frame_rate: FrameRate) -> f64 {
        self.0 / frame_rate.frames_per_second()
    }

    pub(crate) fn nearest_frame(self) -> Frame {
        Frame::new(self.0.round().clamp(0., u64::MAX as f64) as u64)
    }

    pub(crate) fn offset(self, frames: f64) -> Self {
        Self::from_frames(self.0 + frames)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FrameDuration(NonZeroU64);

impl FrameDuration {
    pub(crate) const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub(super) const fn new_saturating(value: u64) -> Self {
        match Self::new(value) {
            Some(value) => value,
            None => Self(NonZeroU64::MIN),
        }
    }

    pub(crate) const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Positive rational timeline frame rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameRate {
    numerator: u32,
    denominator: u32,
}

impl FrameRate {
    pub(crate) const FPS_30: Self = Self {
        numerator: 30,
        denominator: 1,
    };

    pub(crate) const fn new(numerator: u32, denominator: u32) -> Option<Self> {
        if numerator == 0 || denominator == 0 {
            return None;
        }
        Some(Self {
            numerator,
            denominator,
        })
    }

    pub(crate) fn seconds_to_frame(self, seconds: f64) -> Frame {
        let frames = seconds.max(0.) * f64::from(self.numerator) / f64::from(self.denominator);
        Frame::new(frames.round().clamp(0., u64::MAX as f64) as u64)
    }

    pub(crate) fn frame_to_seconds(self, frame: Frame) -> f64 {
        frame.get() as f64 * f64::from(self.denominator) / f64::from(self.numerator)
    }

    pub(crate) fn seconds_delta_to_frames(self, seconds: f64) -> i64 {
        let frames = seconds * f64::from(self.numerator) / f64::from(self.denominator);
        frames.round().clamp(i64::MIN as f64, i64::MAX as f64) as i64
    }

    pub(crate) fn frame_duration(self) -> Duration {
        let scaled_nanos = u64::from(self.denominator) * 1_000_000_000;
        let nanos = scaled_nanos.div_ceil(u64::from(self.numerator)).max(1);
        Duration::from_nanos(nanos)
    }

    pub(crate) fn frames_per_second(self) -> f64 {
        f64::from(self.numerator) / f64::from(self.denominator)
    }

    pub(crate) const fn numerator(self) -> u32 {
        self.numerator
    }

    pub(crate) const fn denominator(self) -> u32 {
        self.denominator
    }

    pub(crate) fn format_timecode(self, frame: Frame) -> String {
        let frame = u128::from(frame.get());
        let numerator = u128::from(self.numerator);
        let denominator = u128::from(self.denominator);
        let total_seconds = frame.saturating_mul(denominator) / numerator;
        let first_frame_in_second = total_seconds
            .saturating_mul(numerator)
            .div_ceil(denominator);
        let frames = frame.saturating_sub(first_frame_in_second);
        let seconds = total_seconds % 60;
        let total_minutes = total_seconds / 60;
        let minutes = total_minutes % 60;
        let hours = total_minutes / 60;

        format!("{hours:02}:{minutes:02}:{seconds:02}:{frames:02}")
    }
}
