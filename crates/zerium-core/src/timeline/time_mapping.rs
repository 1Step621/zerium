use super::{FrameDuration, FrameRate, TimelineEditError};
use crate::{
    plugin::TimeMappingProperties,
    property::{PropertyValue, PropertyValues},
};

/// A source interval and its rate, independent of media readers and EOF policy.
/// Stored settings are f32; time conversion uses f64 intermediates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeMapping {
    source_offset: f32,
    source_span: f32,
    speed: f32,
}

impl Default for TimeMapping {
    fn default() -> Self {
        Self {
            source_offset: 0.,
            source_span: f32::INFINITY,
            speed: 1.,
        }
    }
}

impl TimeMapping {
    pub const MIN_SPEED: f64 = 0.25;
    pub const MAX_SPEED: f64 = 4.;

    pub fn from_properties(
        ids: TimeMappingProperties<'_>,
        values: &PropertyValues,
    ) -> Result<Self, TimelineEditError> {
        let number = |id| values.property(id).and_then(PropertyValue::as_f32);
        Self::new(
            number(ids.source_start).ok_or(TimelineEditError::InvalidSourceRange)?,
            number(ids.source_duration).ok_or(TimelineEditError::InvalidSourceRange)?,
            number(ids.playback_speed).ok_or(TimelineEditError::InvalidPlaybackSpeed)?,
        )
    }

    pub(super) fn property_values(
        self,
        ids: TimeMappingProperties<'_>,
    ) -> [(&str, PropertyValue); 3] {
        [
            (ids.source_start, PropertyValue::F32(self.source_offset)),
            (ids.source_duration, PropertyValue::F32(self.source_span)),
            (ids.playback_speed, PropertyValue::F32(self.speed)),
        ]
    }

    pub fn new(
        source_offset: f32,
        source_span: f32,
        speed: f32,
    ) -> Result<Self, TimelineEditError> {
        if !speed.is_finite() || !(Self::MIN_SPEED..=Self::MAX_SPEED).contains(&f64::from(speed)) {
            return Err(TimelineEditError::InvalidPlaybackSpeed);
        }
        if !source_offset.is_finite()
            || source_offset < 0.
            || !source_span.is_finite()
            || source_span <= 0.
        {
            return Err(TimelineEditError::InvalidSourceRange);
        }
        Ok(Self {
            source_offset,
            source_span,
            speed,
        })
    }

    pub fn source_offset(self) -> f64 {
        f64::from(self.source_offset)
    }

    pub fn speed(self) -> f64 {
        f64::from(self.speed)
    }

    pub fn source_span(self) -> f64 {
        f64::from(self.source_span)
    }

    fn source_end(self) -> f64 {
        self.source_offset() + self.source_span()
    }

    pub(super) fn with_span(self, source_span: f64) -> Self {
        Self {
            source_span: source_span.max(0.) as f32,
            ..self
        }
    }

    pub(super) fn with_speed(self, speed: f64) -> Option<Self> {
        Self::new(self.source_offset, self.source_span, speed as f32).ok()
    }

    fn timeline_frames_for_span(self, source_span: f32, frame_rate: FrameRate) -> f64 {
        let frames = f64::from(source_span) / self.speed() * frame_rate.frames_per_second();
        let nearest = frames.round();
        // Compare a stored frame boundary at source precision, without an epsilon.
        let boundary_span = nearest / frame_rate.frames_per_second() * self.speed();
        if boundary_span as f32 == source_span {
            nearest
        } else {
            frames
        }
    }

    pub(super) fn timeline_duration(self, frame_rate: FrameRate) -> FrameDuration {
        FrameDuration::new_saturating(
            self.timeline_frames_for_span(self.source_span, frame_rate)
                .ceil() as u64,
        )
    }

    pub(super) fn extend_left_frames(self, frame_rate: FrameRate) -> u64 {
        self.timeline_frames_for_span(self.source_offset, frame_rate)
            .floor() as u64
    }

    pub(super) fn trim_start(self, seconds: f64) -> Self {
        let source_offset = (self.source_offset + (seconds * self.speed()) as f32).max(0.);
        Self {
            source_offset,
            source_span: (self.source_end() - f64::from(source_offset)).max(0.) as f32,
            ..self
        }
    }

    pub fn source_seconds(self, local_seconds: f64) -> f64 {
        self.source_offset() + local_seconds * self.speed()
    }
}
