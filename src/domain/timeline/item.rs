use std::{collections::HashMap, sync::Arc};

use crate::domain::animation::{ParameterAnimation, ParameterAnimations};
use crate::domain::media::MediaAsset;
use crate::domain::parameter::{ParameterValue, ParameterValues};
use crate::domain::plugin::{EffectSchema, ItemSchema};

use super::{
    ids::{EffectInstanceId, ItemId, SceneId},
    time::{Frame, FrameDuration, TimelineTime},
};

const MAX_ITEM_LABEL_CHARS: usize = 40;

fn concise_label(value: &str) -> Option<String> {
    let first_line = value.lines().map(str::trim).find(|line| !line.is_empty())?;
    let normalized = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return None;
    }
    if normalized.chars().count() <= MAX_ITEM_LABEL_CHARS {
        return Some(normalized);
    }

    let mut shortened = normalized
        .chars()
        .take(MAX_ITEM_LABEL_CHARS.saturating_sub(1))
        .collect::<String>();
    shortened.push('…');
    Some(shortened)
}

fn asset_label(asset: &MediaAsset) -> Option<String> {
    asset
        .path
        .file_stem()
        .and_then(|name| name.to_str())
        .and_then(concise_label)
        .or_else(|| concise_label(&asset.name))
}

pub(super) fn size_values(values: &ParameterValues, schema: &ItemSchema) -> Option<[f32; 2]> {
    let parameter = schema.size_parameter()?;
    let value = values.get(&parameter.id)?;
    Some([
        value.scalar_at(Some(0))?.numeric_scalar()? as f32,
        value.scalar_at(Some(1))?.numeric_scalar()? as f32,
    ])
}

pub(super) fn set_size_values(
    values: &mut ParameterValues,
    schema: &ItemSchema,
    size: [f32; 2],
) -> bool {
    let Some(parameter) = schema.size_parameter() else {
        return false;
    };
    values
        .set(parameter, ParameterValue::f32_tuple(size))
        .unwrap_or(false)
}

pub(super) fn size_with_derived_height(
    requested: [f32; 2],
    aspect_ratio: f32,
    schema: &ItemSchema,
) -> [f32; 2] {
    if !aspect_ratio.is_finite() || aspect_ratio <= 0. {
        return requested;
    }
    let Some(parameter) = schema.size_parameter() else {
        return requested;
    };
    let width = parameter.scalar_constraints(Some(0));
    let height = parameter.scalar_constraints(Some(1));
    let ratio = f64::from(aspect_ratio);
    // Intersect both axes in width units before writing either component.
    // Keep a positive size so a locked ratio survives even schemas allowing zero.
    let positive = f64::from(f32::MIN_POSITIVE);
    let minimum = width
        .min
        .unwrap_or(positive)
        .max(positive)
        .max(height.min.unwrap_or(positive).max(positive) * ratio);
    let maximum = width
        .max
        .unwrap_or(f64::from(f32::MAX))
        .min(f64::from(f32::MAX))
        .min(
            height
                .max
                .unwrap_or(f64::from(f32::MAX))
                .min(f64::from(f32::MAX))
                * ratio,
        );
    if minimum > maximum || !requested[0].is_finite() {
        return requested;
    }
    let width = f64::from(requested[0]).clamp(minimum, maximum);
    [width as f32, (width / ratio) as f32]
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EffectInstance {
    pub id: EffectInstanceId,
    pub plugin_id: String,
    pub effect_id: String,
    pub parameters: ParameterValues,
    pub animations: ParameterAnimations,
    pub(crate) schema: Arc<EffectSchema>,
}

impl EffectInstance {
    pub(crate) fn schema(&self) -> &EffectSchema {
        &self.schema
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TimelineItemKind {
    Plugin {
        plugin_id: String,
        item_id: String,
        schema: Arc<ItemSchema>,
    },
    Scene {
        scene_id: SceneId,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TimelineItem {
    pub id: ItemId,
    pub start: Frame,
    pub duration: FrameDuration,
    pub(crate) kind: TimelineItemKind,
    pub assets: HashMap<String, MediaAsset>,
    pub parameters: ParameterValues,
    pub animations: ParameterAnimations,
    pub aspect_ratio_locked: bool,
    pub effects: Vec<EffectInstance>,
}

impl TimelineItem {
    pub(crate) fn schema(&self) -> Option<&ItemSchema> {
        self.schema_arc().map(AsRef::as_ref)
    }

    pub(super) fn schema_arc(&self) -> Option<&Arc<ItemSchema>> {
        match &self.kind {
            TimelineItemKind::Plugin { schema, .. } => Some(schema),
            TimelineItemKind::Scene { .. } => None,
        }
    }

    pub(crate) fn plugin_id(&self) -> Option<&str> {
        match &self.kind {
            TimelineItemKind::Plugin { plugin_id, .. } => Some(plugin_id),
            TimelineItemKind::Scene { .. } => None,
        }
    }

    pub(crate) fn item_id(&self) -> Option<&str> {
        match &self.kind {
            TimelineItemKind::Plugin { item_id, .. } => Some(item_id),
            TimelineItemKind::Scene { .. } => None,
        }
    }

    pub(crate) const fn scene_id(&self) -> Option<SceneId> {
        match self.kind {
            TimelineItemKind::Plugin { .. } => None,
            TimelineItemKind::Scene { scene_id } => Some(scene_id),
        }
    }

    pub(crate) fn media(&self, input_id: &str) -> Option<&MediaAsset> {
        self.assets.get(input_id)
    }

    /// Derives the display label owned by a plugin-backed item.
    /// Scene-instance labels are resolved by `TimelineEditor` from their `SceneId`.
    pub(crate) fn intrinsic_label(&self) -> Option<String> {
        let schema = self.schema()?;
        if let Some(label) = schema
            .label_parameter()
            .and_then(|parameter| self.parameters.get(&parameter.id))
            .and_then(|value| match value {
                ParameterValue::String(value) => concise_label(value),
                _ => None,
            })
        {
            return Some(label);
        }

        let asset_labels = schema
            .files()
            .iter()
            .filter_map(|file| self.assets.get(file.id()))
            .filter_map(asset_label)
            .collect::<Vec<_>>();
        if !asset_labels.is_empty()
            && let Some(label) = concise_label(&asset_labels.join(" + "))
        {
            return Some(label);
        }

        Some(schema.label().to_owned())
    }

    pub(crate) fn symbol(&self) -> &str {
        if self.scene_id().is_some() {
            return "◇";
        }
        self.schema()
            .expect("timeline item has a registered schema")
            .symbol()
    }

    /// Linear audio gain read through the audio capability's volume reference.
    pub(crate) fn audio_gain(&self) -> f32 {
        let Some(audio) = self.schema().and_then(ItemSchema::audio) else {
            return 1.;
        };
        match self.parameters.get(audio.volume_parameter()) {
            Some(ParameterValue::F32(value)) => value.max(0.),
            _ => 1.,
        }
    }

    pub(super) fn current_aspect_ratio(&self, schema: &ItemSchema) -> Option<f32> {
        let current_size = size_values(&self.parameters, schema);
        let source_size = schema.files().iter().find_map(|file| {
            self.assets
                .get(file.id())?
                .kind
                .dimensions()
                .map(|[width, height]| [width as f32, height as f32])
        });
        let [width, height] = current_size.or(source_size)?;
        (width.is_finite() && height.is_finite() && width > 0. && height > 0.)
            .then_some(width / height)
    }

    pub(super) fn preserves_aspect_ratio(&self) -> bool {
        self.aspect_ratio_locked
    }

    pub(super) fn constrain_size_to_aspect_ratio(&mut self, aspect_ratio: f32) -> bool {
        let Some(schema) = self.schema().cloned() else {
            return false;
        };
        let Some(requested) = size_values(&self.parameters, &schema) else {
            return false;
        };
        let adjusted = size_with_derived_height(requested, aspect_ratio, &schema);
        set_size_values(&mut self.parameters, &schema, adjusted)
    }

    pub(crate) fn animation_span_frames(&self) -> f64 {
        self.duration.get().saturating_sub(1).max(1) as f64
    }

    pub(crate) fn animation_progress_at_time(&self, time: TimelineTime) -> f32 {
        ((time.frames() - self.start.get() as f64) / self.animation_span_frames()).clamp(0., 1.)
            as f32
    }

    pub(crate) fn animation_timeline_frame(&self, progress: f32) -> f64 {
        self.start.get() as f64 + f64::from(progress.clamp(0., 1.)) * self.animation_span_frames()
    }

    pub(super) fn trim_left_to(&mut self, start: Frame, duration: FrameDuration) {
        let old_span = self.animation_span_frames();
        let new_span = duration.get().saturating_sub(1).max(1) as f64;
        self.remap_animations((old_span - new_span) / old_span, 1.);
        self.start = start;
        self.duration = duration;
    }

    pub(super) fn trim_right_to(&mut self, duration: FrameDuration) {
        let old_span = self.animation_span_frames();
        let new_span = duration.get().saturating_sub(1).max(1) as f64;
        self.remap_animations(0., new_span / old_span);
        self.duration = duration;
    }

    fn remap_animations(&mut self, start: f64, end: f64) {
        self.animations.remap_time_range(start, end);
        for effect in &mut self.effects {
            effect.animations.remap_time_range(start, end);
        }
    }

    pub(crate) fn evaluated_at_time(&self, time: TimelineTime) -> Self {
        let progress = self.animation_progress_at_time(time);
        let mut item = self.clone();
        let Some(schema) = self.schema() else {
            for effect in &mut item.effects {
                effect.parameters = effect.animations.evaluated_values(
                    &effect.parameters,
                    effect.schema.parameters(),
                    progress,
                );
            }
            return item;
        };
        let mut parameters =
            self.animations
                .evaluated_values(&self.parameters, schema.parameters(), progress);
        if self.aspect_ratio_locked
            && let Some(aspect_ratio) = self.current_aspect_ratio(schema)
            && let Some(requested) = size_values(&parameters, schema)
        {
            let adjusted = size_with_derived_height(requested, aspect_ratio, schema);
            set_size_values(&mut parameters, schema, adjusted);
        }
        item.parameters = parameters;
        for effect in &mut item.effects {
            effect.parameters = effect.animations.evaluated_values(
                &effect.parameters,
                effect.schema.parameters(),
                progress,
            );
        }
        item
    }

    pub(crate) fn animation(
        &self,
        effect_id: Option<EffectInstanceId>,
        parameter_id: &str,
        array_index: Option<usize>,
    ) -> Option<&ParameterAnimation> {
        match effect_id {
            Some(effect_id) => self
                .effects
                .iter()
                .find(|effect| effect.id == effect_id)?
                .animations
                .get(parameter_id, array_index),
            None => self.animations.get(parameter_id, array_index),
        }
    }

    /// First frame after the item.
    pub(crate) fn end_exclusive(&self) -> Frame {
        Frame(self.start.0.saturating_add(self.duration.get()))
    }

    pub(super) fn contains(&self, frame: Frame) -> bool {
        frame >= self.start && frame < self.end_exclusive()
    }
}
