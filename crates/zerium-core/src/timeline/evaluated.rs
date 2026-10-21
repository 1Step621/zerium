//! Sampled values are read models, never editable or serializable timeline items.
use super::{
    AspectRatio, BlendMode, EffectInstanceId, EvaluatedClip, FrameRate, ItemId, SceneId,
    TimelineItemKind, TimelineTime,
};
use crate::{
    media::{MediaAsset, MediaMetadataCache, MediaPlayback},
    plugin::{EffectSchema, ItemSchema},
    property::{PropertySchema, PropertyValues},
};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct EvaluatedEffect {
    pub id: EffectInstanceId,
    pub plugin_id: String,
    pub effect_id: String,
    pub properties: PropertyValues,
    pub(super) schema: Arc<EffectSchema>,
    pub(super) aspect_ratio: Option<AspectRatio>,
}

impl EvaluatedEffect {
    pub fn schema(&self) -> &EffectSchema {
        &self.schema
    }
}

#[derive(Clone, Debug)]
pub struct EvaluatedItem {
    /// Placement-specific identity, used only by runtime consumers.
    pub runtime_id: ItemId,
    /// Identity in the definition, independent of placement and sampling.
    pub source_id: ItemId,
    pub placement: EvaluatedClip,
    pub blend_mode: BlendMode,
    pub kind: TimelineItemKind,
    pub properties: PropertyValues,
    pub effects: Vec<EvaluatedEffect>,
    pub(super) aspect_ratio: Option<AspectRatio>,
}

impl EvaluatedItem {
    pub fn schema(&self) -> Option<&ItemSchema> {
        match &self.kind {
            TimelineItemKind::Plugin { schema, .. } => Some(schema),
            TimelineItemKind::Scene { .. } => None,
        }
    }

    pub fn scene_id(&self) -> Option<SceneId> {
        match self.kind {
            TimelineItemKind::Scene { scene_id } => Some(scene_id),
            TimelineItemKind::Plugin { .. } => None,
        }
    }

    pub fn aspect_lock_property(
        &self,
        effect: Option<EffectInstanceId>,
    ) -> Option<&PropertySchema> {
        match effect {
            Some(id) => self
                .effects
                .iter()
                .find(|effect| effect.id == id)?
                .schema()
                .aspect_lock_property(),
            None => self.schema()?.aspect_lock_property(),
        }
    }

    pub fn aspect_ratio(&self, effect: Option<EffectInstanceId>) -> Option<AspectRatio> {
        match effect {
            Some(id) => {
                self.effects
                    .iter()
                    .find(|effect| effect.id == id)?
                    .aspect_ratio
            }
            None => self.aspect_ratio,
        }
    }

    pub fn property_values(&self, effect: Option<EffectInstanceId>) -> Option<&PropertyValues> {
        match effect {
            Some(id) => Some(
                &self
                    .effects
                    .iter()
                    .find(|effect| effect.id == id)?
                    .properties,
            ),
            None => Some(&self.properties),
        }
    }

    pub fn local_seconds(&self, time: TimelineTime, frame_rate: FrameRate) -> f64 {
        (time.frames() - self.placement.start.get() as f64) / frame_rate.frames_per_second()
    }

    pub fn media_input(
        &self,
        effect: Option<EffectInstanceId>,
        input: &str,
        cache: &MediaMetadataCache,
    ) -> Option<(MediaAsset, MediaPlayback)> {
        let (inputs, playback, values) = match effect {
            Some(id) => {
                let effect = self.effects.iter().find(|effect| effect.id == id)?;
                (
                    effect.schema.inputs(),
                    effect.schema.playback(),
                    &effect.properties,
                )
            }
            None => {
                let schema = self.schema()?;
                (schema.inputs(), schema.playback(), &self.properties)
            }
        };
        super::item::media_input_for(inputs, playback, values, input, cache)
    }
}
