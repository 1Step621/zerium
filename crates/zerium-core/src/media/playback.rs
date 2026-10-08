use crate::timeline::{TimeMapping, TimelineEditError};
use std::time::Duration;

/// How a media input behaves after its source ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediaEndBehavior {
    #[default]
    Stop,
    Loop,
    Hold,
}

/// Per-input time mapping and behavior at the reader's actual end of stream.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MediaPlayback {
    mapping: TimeMapping,
    end_behavior: MediaEndBehavior,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MediaSample {
    pub time: Duration,
    /// A held video frame; audio is silent here.
    pub held: bool,
}

impl MediaPlayback {
    pub fn from_properties(
        ids: crate::plugin::PlaybackProperties<'_>,
        values: &crate::property::PropertyValues,
    ) -> Result<Self, TimelineEditError> {
        use crate::property::PropertyValue;
        let end_behavior = match values
            .property(ids.end_behavior)
            .ok_or(TimelineEditError::InvalidSourceRange)?
        {
            PropertyValue::Enum(0) => MediaEndBehavior::Stop,
            PropertyValue::Enum(1) => MediaEndBehavior::Loop,
            PropertyValue::Enum(2) => MediaEndBehavior::Hold,
            _ => return Err(TimelineEditError::InvalidSourceRange),
        };
        Ok(Self {
            mapping: TimeMapping::from_properties(ids.time_mapping(), values)?,
            end_behavior,
        })
    }

    pub fn source_span(self) -> f64 {
        self.mapping.source_span()
    }

    pub fn speed(self) -> f64 {
        self.mapping.speed()
    }

    pub fn source_seconds(self, local_seconds: f64) -> f64 {
        self.mapping.source_seconds(local_seconds)
    }

    pub fn end_behavior(self) -> MediaEndBehavior {
        self.end_behavior
    }

    /// The single mapping used by preview, export, and audio playback.
    pub fn sample(self, local_seconds: f64, source_duration: Duration) -> Option<MediaSample> {
        if local_seconds < 0. || local_seconds * self.speed() >= self.mapping.source_span() {
            return None;
        }
        let seconds = self.source_seconds(local_seconds);
        let duration = source_duration.as_secs_f64();
        if !seconds.is_finite() || seconds < 0. || duration <= 0. {
            return None;
        }
        let (seconds, held) = match self.end_behavior {
            MediaEndBehavior::Stop if seconds >= duration => return None,
            MediaEndBehavior::Stop => (seconds, false),
            MediaEndBehavior::Loop => (seconds.rem_euclid(duration), false),
            MediaEndBehavior::Hold => (
                seconds.min((duration - 0.000_000_001).max(0.)),
                seconds >= duration,
            ),
        };
        Some(MediaSample {
            time: Duration::try_from_secs_f64(seconds).ok()?,
            held,
        })
    }
}
