//! One item-wide playback declaration, shared by its visual and audio inputs.
use super::{
    ItemSchema, PluginError, TextureInput,
    validation::{PropertyReferences, has_enum_values},
};
use crate::{
    media::{MediaEndBehavior, MediaInputReference, MediaPlayback},
    property::{PropertyValue, PropertyValues, ScalarPropertyType},
    timeline::{TimeMapping, TimelineEditError},
};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ItemPlaybackSchema {
    pub source: MediaInputReference,
    pub source_start: String,
    pub source_duration: String,
    pub playback_speed: String,
    pub end_behavior: String,
}

impl ItemPlaybackSchema {
    pub fn property_ids(&self) -> [&str; 4] {
        [
            &self.source_start,
            &self.source_duration,
            &self.playback_speed,
            &self.end_behavior,
        ]
    }

    pub fn mapping(&self, values: &PropertyValues) -> Result<TimeMapping, TimelineEditError> {
        let number = |id: &str| values.property(id).and_then(PropertyValue::as_f32);
        TimeMapping::new(
            number(&self.source_start).ok_or(TimelineEditError::InvalidSourceRange)?,
            number(&self.source_duration).ok_or(TimelineEditError::InvalidSourceRange)?,
            number(&self.playback_speed).ok_or(TimelineEditError::InvalidPlaybackSpeed)?,
        )
    }

    pub fn resolve(&self, values: &PropertyValues) -> Result<MediaPlayback, TimelineEditError> {
        let end = match values
            .property(&self.end_behavior)
            .and_then(PropertyValue::as_enum)
        {
            Some(0) => MediaEndBehavior::Stop,
            Some(1) => MediaEndBehavior::Loop,
            Some(2) => MediaEndBehavior::Hold,
            _ => return Err(TimelineEditError::InvalidSourceRange),
        };
        Ok(MediaPlayback::new(self.mapping(values)?, end))
    }

    pub(super) fn validate(&self, item: &ItemSchema) -> Result<(), PluginError> {
        let found = match &self.source {
            MediaInputReference::Media(id) => item.inputs().iter().any(
                |input| matches!(input, TextureInput::Media { id: input_id, .. } if input_id == id),
            ),
            MediaInputReference::Audio(id) => item.audio_input(id).is_some(),
        };
        if !found {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' playback source was not found",
                item.id()
            )));
        }
        let context = format!("item '{}' playback", item.id());
        let references = PropertyReferences::new(&context, item.properties());
        let mut ids = HashSet::new();
        for id in [
            &self.source_start,
            &self.source_duration,
            &self.playback_speed,
        ] {
            references.check(
                id,
                "a distinct f32 without animation or scene bindings",
                |property| {
                    ids.insert(id)
                        && property.scalar_type(None, None) == Some(&ScalarPropertyType::F32)
                        && !property.is_animatable(None)
                        && !property.is_scene_bindable(None)
                },
            )?;
        }
        references.check(
            &self.end_behavior,
            "an enum [0, 1, 2] without animation or scene bindings",
            |property| {
                property
                    .scalar_type(None, None)
                    .is_some_and(|ty| has_enum_values(ty, &[0, 1, 2]))
                    && !property.is_animatable(None)
                    && !property.is_scene_bindable(None)
            },
        )?;
        self.resolve(&PropertyValues::from_properties(item.properties()))
            .map_err(|error| PluginError::invalid_definition(format!("{context}: {error}")))?;
        Ok(())
    }
}
