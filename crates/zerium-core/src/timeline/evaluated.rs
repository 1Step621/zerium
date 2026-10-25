//! Sampled values are read models, never editable or serializable timeline items.
use super::{
    AspectRatio, BlendMode, EffectInstanceId, EvaluatedClip, FrameRate, ItemId, LayerId, SceneId,
    TimelineItemKind, TimelineTime,
};
use crate::{
    media::{MediaAsset, MediaMetadataCache, MediaPlayback},
    plugin::{EffectSchema, ItemSchema, TextureInput},
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
    pub playback: MediaPlayback,
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

    pub fn render_result_ranges(&self) -> impl Iterator<Item = super::RenderResultSettings> + '_ {
        self.schema()
            .into_iter()
            .flat_map(|schema| schema.inputs())
            .filter_map(|input| render_result_settings_for(input, &self.properties))
            .chain(self.effects.iter().flat_map(|effect| {
                effect
                    .schema()
                    .inputs()
                    .iter()
                    .filter_map(|input| render_result_settings_for(input, &effect.properties))
            }))
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
        let (inputs, values) = match effect {
            Some(id) => {
                let effect = self.effects.iter().find(|effect| effect.id == id)?;
                (effect.schema.inputs(), &effect.properties)
            }
            None => (self.schema()?.inputs(), &self.properties),
        };
        let source = inputs
            .iter()
            .find(|source| source.id() == input)?
            .media_source()?;
        Some((source.asset(values, cache)?, self.playback))
    }
}

pub(super) fn audio_input_for(
    schema: &ItemSchema,
    input_id: &str,
    properties: &PropertyValues,
    playback: MediaPlayback,
    cache: &MediaMetadataCache,
) -> Option<(MediaAsset, MediaPlayback, bool)> {
    let input = schema.audio_input(input_id)?;
    let asset = input.media_source().asset(properties, cache)?;
    let preserve_pitch = properties
        .property(input.preserve_pitch_property())?
        .as_bool()
        .expect("validated preserve pitch");
    Some((asset, playback, preserve_pitch))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderResultSettings {
    pub start_offset: u64,
    pub end_offset: u64,
    pub hide_original: bool,
}

impl RenderResultSettings {
    pub fn from_properties(
        properties: &PropertyValues,
        start_offset: &str,
        end_offset: &str,
        hide_original: &str,
    ) -> Option<Self> {
        let start = u64::from(properties.property(start_offset)?.as_u32()?);
        let end = u64::from(properties.property(end_offset)?.as_u32()?);
        let hide_original = properties.property(hide_original)?.as_bool()?;
        Some(Self {
            start_offset: start.min(end),
            end_offset: start.max(end),
            hide_original,
        })
    }

    pub fn includes(self, source: LayerId, candidate: LayerId) -> bool {
        let Some(offset) = source.get().checked_sub(candidate.get()) else {
            return false;
        };
        (self.start_offset..=self.end_offset).contains(&offset)
    }

    pub fn layer_bounds(self, source: LayerId) -> Option<(LayerId, LayerId)> {
        let top = source.get().saturating_sub(self.end_offset);
        let bottom = source.get().checked_sub(self.start_offset)?;
        Some((LayerId::new(top), LayerId::new(bottom)))
    }
}

fn render_result_settings_for(
    capability: &TextureInput,
    properties: &PropertyValues,
) -> Option<RenderResultSettings> {
    let TextureInput::RenderResult {
        start_offset,
        end_offset,
        hide_original,
        ..
    } = capability
    else {
        return None;
    };
    RenderResultSettings::from_properties(properties, start_offset, end_offset, hide_original)
}
