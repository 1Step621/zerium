//! Normalize validated capability declarations into shared, typed source clocks.

use std::collections::BTreeMap;

use crate::media::{MediaEndBehavior, MediaPlayback};
use crate::property::PropertyValues;
use crate::timeline::{TimeMapping, TimelineEditError};

use super::{AudioCapability, PlaybackProperties, TextureInput, TimeMappingProperties};

#[derive(Clone, Debug, PartialEq, Eq)]
struct MappingBinding {
    source_start: String,
    source_duration: String,
    playback_speed: String,
}

impl MappingBinding {
    fn properties(&self) -> TimeMappingProperties<'_> {
        TimeMappingProperties {
            source_start: &self.source_start,
            source_duration: &self.source_duration,
            playback_speed: &self.playback_speed,
        }
    }

    fn resolve(&self, values: &PropertyValues) -> Result<TimeMapping, TimelineEditError> {
        self.properties().resolve(values)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PlaybackBinding {
    mapping: usize,
    end_behavior: String,
}

impl PlaybackBinding {
    fn end_behavior(&self, values: &PropertyValues) -> Result<MediaEndBehavior, TimelineEditError> {
        match values
            .property(&self.end_behavior)
            .and_then(|value| value.as_enum())
        {
            Some(0) => Ok(MediaEndBehavior::Stop),
            Some(1) => Ok(MediaEndBehavior::Loop),
            Some(2) => Ok(MediaEndBehavior::Hold),
            _ => Err(TimelineEditError::InvalidSourceRange),
        }
    }

    fn resolve(
        &self,
        mappings: &[MappingBinding],
        values: &PropertyValues,
    ) -> Result<MediaPlayback, TimelineEditError> {
        Ok(MediaPlayback::new(
            mappings[self.mapping].resolve(values)?,
            self.end_behavior(values)?,
        ))
    }
}

/// Derived once after manifest validation. Inputs may share a mapping or retain
/// independent clocks; runtime consumers no longer walk capability declarations.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlaybackBindings {
    mappings: Vec<MappingBinding>,
    media: BTreeMap<String, PlaybackBinding>,
    audio: BTreeMap<String, PlaybackBinding>,
    timeline: Option<usize>,
}

impl PlaybackBindings {
    pub(super) fn compile(
        inputs: &[TextureInput],
        audio: &[AudioCapability],
        timeline: Option<TimeMappingProperties<'_>>,
    ) -> Self {
        let mut bindings = Self::default();
        for input in inputs {
            if let Some(properties) = input.playback_properties() {
                let binding = bindings.bind(properties);
                bindings.media.insert(input.id().to_owned(), binding);
            }
        }
        for input in audio {
            let binding = bindings.bind(input.playback_properties());
            bindings.audio.insert(input.id().to_owned(), binding);
        }
        bindings.timeline = timeline.map(|properties| bindings.intern(properties));
        bindings
    }

    fn intern(&mut self, properties: TimeMappingProperties<'_>) -> usize {
        let mapping = MappingBinding {
            source_start: properties.source_start.to_owned(),
            source_duration: properties.source_duration.to_owned(),
            playback_speed: properties.playback_speed.to_owned(),
        };
        if let Some(index) = self.mappings.iter().position(|current| *current == mapping) {
            index
        } else {
            let index = self.mappings.len();
            self.mappings.push(mapping);
            index
        }
    }

    fn bind(&mut self, properties: PlaybackProperties<'_>) -> PlaybackBinding {
        PlaybackBinding {
            mapping: self.intern(properties.time_mapping()),
            end_behavior: properties.end_behavior.to_owned(),
        }
    }

    pub fn media(
        &self,
        id: &str,
        values: &PropertyValues,
    ) -> Result<MediaPlayback, TimelineEditError> {
        self.media.get(id).map_or_else(
            || Ok(MediaPlayback::default()),
            |binding| binding.resolve(&self.mappings, values),
        )
    }

    pub fn audio(
        &self,
        id: &str,
        values: &PropertyValues,
    ) -> Option<Result<MediaPlayback, TimelineEditError>> {
        self.audio
            .get(id)
            .map(|binding| binding.resolve(&self.mappings, values))
    }

    pub fn timeline(
        &self,
        values: &PropertyValues,
    ) -> Result<Option<TimeMapping>, TimelineEditError> {
        self.timeline
            .map(|index| self.mappings[index].resolve(values))
            .transpose()
    }

    pub fn validate(&self, values: &PropertyValues) -> Result<(), TimelineEditError> {
        for mapping in &self.mappings {
            mapping.resolve(values)?;
        }
        for binding in self.media.values().chain(self.audio.values()) {
            binding.end_behavior(values)?;
        }
        Ok(())
    }
}
