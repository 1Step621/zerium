use std::sync::Arc;

use crate::animation::{AnimationClock, ScalarAnimations, ScalarTrack};
use crate::media::{MediaAsset, MediaMetadataCache, MediaPlayback};
use crate::plugin::{EffectSchema, ItemSchema, TextureInput};
use crate::property::{
    PropertyElementId, PropertyPath, PropertySchema, PropertyValue, PropertyValues,
};

use super::{
    BlendMode, FrameRate, ResizeMode, TimeMapping,
    aspect_ratio::AspectRatio,
    ids::{EffectInstanceId, ItemId, LayerId, SceneId},
    properties::resolve_property,
    time::{Frame, FrameDuration, TimelineTime},
};

/// Resolve media from sampled values without materializing an editable item.
pub(super) fn media_input_for(
    inputs: &[TextureInput],
    playback: &crate::plugin::PlaybackBindings,
    properties: &PropertyValues,
    input_id: &str,
    cache: &MediaMetadataCache,
) -> Option<(MediaAsset, MediaPlayback)> {
    let capability = inputs
        .iter()
        .find(|capability| capability.id() == input_id)?;
    let asset = capability.media_source()?.asset(properties, cache)?;
    let clock = playback
        .media(input_id, properties)
        .expect("validated media playback properties");
    Some((asset, clock))
}

pub(super) fn audio_input_for(
    schema: &ItemSchema,
    input_id: &str,
    properties: &PropertyValues,
    cache: &MediaMetadataCache,
) -> Option<(MediaAsset, MediaPlayback, bool)> {
    let input = schema.audio_input(input_id)?;
    let asset = input.media_source().asset(properties, cache)?;
    let playback = schema
        .playback()
        .audio(input_id, properties)?
        .expect("validated audio playback properties");
    let preserve_pitch = properties
        .property(input.preserve_pitch_property())?
        .as_bool()
        .expect("validated preserve-pitch property");
    Some((asset, playback, preserve_pitch))
}

const MAX_ITEM_LABEL_CHARS: usize = 40;

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

pub(super) fn size_values(values: &PropertyValues, schema: &ItemSchema) -> Option<[f32; 2]> {
    let property = schema.size_property()?;
    let value = values.property(&property.id)?;
    Some([
        value.scalar_at(Some(0))?.numeric_scalar()? as f32,
        value.scalar_at(Some(1))?.numeric_scalar()? as f32,
    ])
}

pub(super) fn set_size_values(
    values: &mut PropertyValues,
    schema: &ItemSchema,
    size: [f32; 2],
) -> bool {
    let Some(property) = schema.size_property() else {
        return false;
    };
    values
        .set(property, PropertyValue::f32_tuple(size))
        .unwrap_or(false)
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectInstance {
    pub id: EffectInstanceId,
    pub plugin_id: String,
    pub effect_id: String,
    pub properties: PropertyValues,
    pub animations: ScalarAnimations,
    pub aspect_ratio: Option<AspectRatio>,
    pub schema: Arc<EffectSchema>,
}

impl EffectInstance {
    pub fn schema(&self) -> &EffectSchema {
        &self.schema
    }

    pub fn render_result_settings(&self) -> impl Iterator<Item = RenderResultSettings> + '_ {
        self.schema()
            .inputs()
            .iter()
            .filter_map(|capability| render_result_settings_for(capability, &self.properties))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TimelineItemKind {
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
pub struct TimelineItem {
    pub id: ItemId,
    pub start: Frame,
    pub duration: FrameDuration,
    pub blend_mode: BlendMode,
    pub kind: TimelineItemKind,
    pub properties: PropertyValues,
    pub animations: ScalarAnimations,
    pub aspect_ratio: Option<AspectRatio>,
    pub effects: Vec<EffectInstance>,
}

/// One resolved property owner, shared by schema lookup, evaluation and editing.
pub(super) struct PropertyOwnerRef<'a> {
    pub values: &'a PropertyValues,
    pub animations: &'a ScalarAnimations,
    pub schemas: &'a [PropertySchema],
}

pub(super) struct PropertyOwnerMut<'a> {
    pub values: &'a mut PropertyValues,
    pub animations: &'a mut ScalarAnimations,
}

impl TimelineItem {
    pub(super) fn scene_instance(
        id: ItemId,
        start: Frame,
        duration: FrameDuration,
        scene_id: SceneId,
    ) -> Self {
        Self {
            id,
            start,
            duration,
            blend_mode: BlendMode::Normal,
            kind: TimelineItemKind::Scene { scene_id },
            properties: PropertyValues::default(),
            animations: ScalarAnimations::default(),
            aspect_ratio: None,
            effects: Vec::new(),
        }
    }

    pub(super) fn effect(&self, id: EffectInstanceId) -> Option<&EffectInstance> {
        self.effects.iter().find(|effect| effect.id == id)
    }

    pub(super) fn effect_mut(&mut self, id: EffectInstanceId) -> Option<&mut EffectInstance> {
        self.effects.iter_mut().find(|effect| effect.id == id)
    }

    pub(super) fn property_owner(
        &self,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<PropertyOwnerRef<'_>> {
        Some(match effect_id {
            Some(id) => {
                let effect = self.effect(id)?;
                PropertyOwnerRef {
                    values: &effect.properties,
                    animations: &effect.animations,
                    schemas: effect.schema.properties(),
                }
            }
            None => PropertyOwnerRef {
                values: &self.properties,
                animations: &self.animations,
                schemas: self.schema().map_or(&[], |schema| schema.properties()),
            },
        })
    }

    pub(super) fn property_owner_mut(
        &mut self,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<PropertyOwnerMut<'_>> {
        Some(match effect_id {
            Some(id) => {
                let effect = self.effect_mut(id)?;
                PropertyOwnerMut {
                    values: &mut effect.properties,
                    animations: &mut effect.animations,
                }
            }
            None => PropertyOwnerMut {
                values: &mut self.properties,
                animations: &mut self.animations,
            },
        })
    }

    pub(super) fn animations(
        &self,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<&ScalarAnimations> {
        Some(self.property_owner(effect_id)?.animations)
    }

    pub(super) fn animations_mut(
        &mut self,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<&mut ScalarAnimations> {
        Some(self.property_owner_mut(effect_id)?.animations)
    }

    pub fn schema(&self) -> Option<&ItemSchema> {
        self.schema_arc().map(AsRef::as_ref)
    }

    pub(super) fn schema_arc(&self) -> Option<&Arc<ItemSchema>> {
        match &self.kind {
            TimelineItemKind::Plugin { schema, .. } => Some(schema),
            TimelineItemKind::Scene { .. } => None,
        }
    }

    pub fn plugin_id(&self) -> Option<&str> {
        match &self.kind {
            TimelineItemKind::Plugin { plugin_id, .. } => Some(plugin_id),
            TimelineItemKind::Scene { .. } => None,
        }
    }

    pub fn item_id(&self) -> Option<&str> {
        match &self.kind {
            TimelineItemKind::Plugin { item_id, .. } => Some(item_id),
            TimelineItemKind::Scene { .. } => None,
        }
    }

    pub const fn scene_id(&self) -> Option<SceneId> {
        match self.kind {
            TimelineItemKind::Plugin { .. } => None,
            TimelineItemKind::Scene { scene_id } => Some(scene_id),
        }
    }

    /// Derives the display label owned by a plugin-backed item.
    /// Scene-instance labels are resolved by `TimelineEditor` from their `SceneId`.
    pub fn intrinsic_label(&self) -> Option<String> {
        let schema = self.schema()?;
        if let Some(label) = schema
            .label_property()
            .and_then(|property| self.properties.property(&property.id))
            .and_then(|value| match value {
                PropertyValue::String(value) => concise_label(value),
                _ => None,
            })
        {
            return Some(label);
        }

        let asset_labels = schema
            .file_properties()
            .filter_map(|property| self.properties.property(property.id())?.file())
            .filter_map(|path| path.file_stem()?.to_str().and_then(concise_label))
            .collect::<Vec<_>>();
        if !asset_labels.is_empty()
            && let Some(label) = concise_label(&asset_labels.join(" + "))
        {
            return Some(label);
        }

        Some(schema.label().to_owned())
    }

    pub fn symbol(&self) -> &str {
        if self.scene_id().is_some() {
            return "◇";
        }
        self.schema()
            .expect("timeline item has a registered schema")
            .symbol()
    }

    pub fn render_result_ranges(&self) -> impl Iterator<Item = RenderResultSettings> + '_ {
        self.schema()
            .into_iter()
            .flat_map(|schema| schema.inputs())
            .filter_map(|capability| render_result_settings_for(capability, &self.properties))
            .chain(
                self.effects
                    .iter()
                    .flat_map(EffectInstance::render_result_settings),
            )
    }

    pub fn aspect_lock_property(
        &self,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<&PropertySchema> {
        match effect_id {
            Some(id) => self.effect(id)?.schema().aspect_lock_property(),
            None => self.schema()?.aspect_lock_property(),
        }
    }

    pub fn aspect_ratio(&self, effect_id: Option<EffectInstanceId>) -> Option<AspectRatio> {
        match effect_id {
            Some(id) => self.effect(id)?.aspect_ratio,
            None => self.aspect_ratio,
        }
    }

    pub fn animation_clock(&self, track: &ScalarTrack) -> AnimationClock {
        AnimationClock::new(self.start, track)
    }

    pub(super) fn timeline_mapping(&self) -> Option<TimeMapping> {
        self.schema()?
            .playback()
            .timeline(&self.properties)
            .expect("validated timeline time mapping")
    }

    pub(crate) fn validate_playback(&self) -> Result<(), super::TimelineEditError> {
        if let Some(schema) = self.schema() {
            schema.playback().validate(&self.properties)?;
        }
        for effect in &self.effects {
            effect.schema().playback().validate(&effect.properties)?;
        }
        Ok(())
    }

    /// Update a candidate item; callers validate it before committing to the document.
    pub(super) fn store_timeline_mapping(&mut self, mapping: TimeMapping) -> Option<TimeMapping> {
        let schema = self.schema_arc()?.clone();
        let ids = schema.timeline()?;
        for (id, value) in ids.values_for(mapping) {
            self.properties.set(schema.property(id)?, value).ok()?;
        }
        schema.playback().timeline(&self.properties).ok().flatten()
    }

    pub(super) fn timeline_speed_bounds(&self) -> (f64, f64) {
        let (min, max) = self
            .schema()
            .and_then(|schema| schema.property(schema.timeline()?.playback_speed))
            .and_then(|property| {
                property
                    .configuration_constraints(None)
                    .numeric_bounds(&crate::property::ScalarPropertyType::F32)
            })
            .unwrap_or((TimeMapping::MIN_SPEED, TimeMapping::MAX_SPEED));
        (
            min.max(TimeMapping::MIN_SPEED),
            max.min(TimeMapping::MAX_SPEED),
        )
    }

    /// Resize a timeline interval and its declared source clock together.
    pub(super) fn resize_to(
        &mut self,
        start: Frame,
        duration: FrameDuration,
        mode: ResizeMode,
        frame_rate: FrameRate,
    ) -> Option<()> {
        if let Some(mapping) = self.timeline_mapping() {
            let mapping = match mode {
                ResizeMode::Trim if start != self.start => {
                    let delta = start.get() as f64 - self.start.get() as f64;
                    mapping.trim_start(delta / frame_rate.frames_per_second())
                }
                ResizeMode::Trim => mapping.with_timeline_duration(duration, frame_rate),
                ResizeMode::Stretch => {
                    let (min_speed, max_speed) = self.timeline_speed_bounds();
                    mapping.with_speed(
                        (mapping.source_span() * frame_rate.frames_per_second()
                            / duration.get() as f64)
                            .clamp(min_speed, max_speed),
                    )?
                }
            };
            self.store_timeline_mapping(mapping)?;
        }
        self.set_interval(start, duration, mode);
        Some(())
    }

    /// Commit the interval. Trimming preserves retained key positions, adopts
    /// crossed keys at the bounds, and moves constant endpoints when extending.
    /// Nonconstant endpoints are retained with a new constant endpoint added.
    /// Stretching rescales integer keys within the new span.
    pub(super) fn set_interval(&mut self, start: Frame, duration: FrameDuration, mode: ResizeMode) {
        let offset = i128::from(start.get()) - i128::from(self.start.get());
        for animations in std::iter::once(&mut self.animations)
            .chain(self.effects.iter_mut().map(|effect| &mut effect.animations))
        {
            match mode {
                ResizeMode::Trim => animations.trim(offset, duration),
                ResizeMode::Stretch => animations.stretch(self.duration, duration),
            }
        }
        self.start = start;
        self.duration = duration;
    }

    /// Synchronize the interval after editing ordinary source-clock properties.
    pub(super) fn synchronize_timeline(
        &mut self,
        previous: Option<TimeMapping>,
        frame_rate: FrameRate,
    ) -> Result<(), super::TimelineEditError> {
        let Some(previous) = previous else {
            return Ok(());
        };
        let mapping = self
            .schema()
            .expect("timeline capability belongs to a plugin item")
            .playback()
            .timeline(&self.properties)?
            .expect("timeline capability is unchanged during property edits");
        if previous.source_span() != mapping.source_span() || previous.speed() != mapping.speed() {
            let mode = if previous.speed() == mapping.speed() {
                ResizeMode::Trim
            } else {
                ResizeMode::Stretch
            };
            self.set_interval(self.start, mapping.timeline_duration(frame_rate), mode);
        }
        Ok(())
    }

    /// Evaluate one property with the same defaults, clock and constraints as playback.
    pub fn evaluated_property_at(
        &self,
        time: TimelineTime,
        effect_id: Option<EffectInstanceId>,
        property: &PropertySchema,
    ) -> Option<PropertyValue> {
        resolve_property(None, self, effect_id, property, Some(time)).map(|resolved| resolved.value)
    }

    pub fn animation_track(
        &self,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
    ) -> Option<&ScalarTrack> {
        let address = PropertyPath::new(property_id, element_id, scalar_index);
        self.animations(effect_id)?.track(&address)
    }

    pub fn property_values(&self, effect_id: Option<EffectInstanceId>) -> Option<&PropertyValues> {
        Some(self.property_owner(effect_id)?.values)
    }

    pub(super) fn property_values_mut(
        &mut self,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<&mut PropertyValues> {
        Some(self.property_owner_mut(effect_id)?.values)
    }

    /// First frame after the item.
    pub fn end_exclusive(&self) -> Frame {
        Frame(self.start.0.saturating_add(self.duration.get()))
    }

    pub fn contains(&self, frame: Frame) -> bool {
        frame >= self.start && frame < self.end_exclusive()
    }
}

impl TimelineItem {
    pub(super) fn set_aspect_ratio(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        ratio: Option<AspectRatio>,
    ) -> bool {
        let current = match effect_id {
            Some(id) => {
                &mut self
                    .effect_mut(id)
                    .expect("prepared effect must exist")
                    .aspect_ratio
            }
            None => &mut self.aspect_ratio,
        };
        if *current == ratio {
            return false;
        }
        *current = ratio;
        true
    }

    pub(super) fn add_effect(
        &mut self,
        instance_id: EffectInstanceId,
        plugin_id: &str,
        effect_id: &str,
        schema: Arc<EffectSchema>,
    ) -> bool {
        let properties = PropertyValues::from_properties(schema.properties());
        let aspect_ratio = schema
            .aspect_lock_default()
            .then(|| {
                schema.aspect_lock_property().and_then(|property| {
                    AspectRatio::from_value(properties.property(property.id())?)
                })
            })
            .flatten();
        if self.scene_id().is_none() {
            let Some(item_schema) = self.schema() else {
                return false;
            };
            if item_schema.render().is_none() {
                return false;
            }
        }
        self.effects.push(EffectInstance {
            id: instance_id,
            plugin_id: plugin_id.to_owned(),
            effect_id: effect_id.to_owned(),
            properties,
            animations: ScalarAnimations::default(),
            aspect_ratio,
            schema,
        });
        true
    }

    pub(super) fn remove_effect(&mut self, effect_id: EffectInstanceId) -> bool {
        let previous_len = self.effects.len();
        self.effects.retain(|effect| effect.id != effect_id);
        self.effects.len() != previous_len
    }

    pub(super) fn move_effect(&mut self, effect_id: EffectInstanceId, target_index: usize) -> bool {
        let Some(source_index) = self
            .effects
            .iter()
            .position(|effect| effect.id == effect_id)
        else {
            return false;
        };
        if target_index >= self.effects.len() || source_index == target_index {
            return false;
        }
        let effect = self.effects.remove(source_index);
        self.effects.insert(target_index, effect);
        true
    }
}
