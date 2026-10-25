use super::TimelineEditError;

/// A source interval and its rate, independent of media readers and EOF policy.
/// Resolved source values are f32; sampling uses f64 intermediates.
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

    pub fn source_seconds(self, local_seconds: f64) -> f64 {
        self.source_offset() + local_seconds * self.speed()
    }
}

impl TimeMapping {
    fn frames_for_span(self, span: f32, rate: super::FrameRate) -> f64 {
        let frames = f64::from(span) / self.speed() * rate.frames_per_second();
        let nearest = frames.round();
        if (nearest / rate.frames_per_second() * self.speed()) as f32 == span {
            nearest
        } else {
            frames
        }
    }

    pub(crate) fn timeline_duration(self, rate: super::FrameRate) -> super::FrameDuration {
        super::FrameDuration::new_saturating(
            self.frames_for_span(self.source_span, rate).ceil() as u64
        )
    }

    pub(crate) fn left_extension(self, rate: super::FrameRate) -> u64 {
        self.frames_for_span(self.source_offset, rate).floor() as u64
    }

    pub(crate) fn stretch_duration_bounds(
        self,
        bounds: (f64, f64),
        rate: super::FrameRate,
    ) -> (u64, u64) {
        let frames = |speed: f64| {
            Self {
                speed: speed as f32,
                ..self
            }
            .frames_for_span(self.source_span, rate)
        };
        (
            frames(bounds.1).ceil() as u64,
            frames(bounds.0).floor() as u64,
        )
    }

    pub(crate) fn with_duration(
        self,
        duration: super::FrameDuration,
        rate: super::FrameRate,
    ) -> Option<Self> {
        Self::new(
            self.source_offset,
            (duration.get() as f64 / rate.frames_per_second() * self.speed()) as f32,
            self.speed,
        )
        .ok()
    }

    pub(crate) fn resized(
        self,
        delta: f64,
        duration: super::FrameDuration,
        mode: super::ResizeMode,
        rate: super::FrameRate,
    ) -> Option<Self> {
        match mode {
            super::ResizeMode::Stretch => Self::new(
                self.source_offset,
                self.source_span,
                (self.source_span() * rate.frames_per_second() / duration.get() as f64) as f32,
            )
            .ok(),
            super::ResizeMode::Trim => {
                let shift = delta / rate.frames_per_second() * self.speed();
                let offset = self.source_offset() + shift;
                let offset = if offset < 0. {
                    if (-shift) as f32 != self.source_offset {
                        return None;
                    }
                    0.
                } else {
                    offset as f32
                };
                Self::new(
                    offset,
                    (duration.get() as f64 / rate.frames_per_second() * self.speed()) as f32,
                    self.speed,
                )
                .ok()
            }
        }
    }
}
