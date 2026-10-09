use std::sync::Arc;

use crate::animation::{AnimationClock, ScalarAnimations, ScalarTrack};
use crate::media::{MediaAsset, MediaMetadataCache, MediaPlayback};
use crate::plugin::{EffectSchema, ItemSchema, TextureInput};
use crate::property::{
    PropertyElementId, PropertyPath, PropertySchema, PropertyValue, PropertyValues,
};

use super::{
    FrameRate, ResizeMode, TimeMapping,
    aspect_ratio::AspectRatio,
    ids::{EffectInstanceId, ItemId, LayerId, SceneId},
    properties::resolve_property,
    time::{Frame, FrameDuration, TimelineTime},
};

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
    pub kind: TimelineItemKind,
    pub properties: PropertyValues,
    pub animations: ScalarAnimations,
    pub aspect_ratio: Option<AspectRatio>,
    pub effects: Vec<EffectInstance>,
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

    pub(super) fn animations(
        &self,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<&ScalarAnimations> {
        match effect_id {
            Some(id) => Some(&self.effect(id)?.animations),
            None => Some(&self.animations),
        }
    }

    pub(super) fn animations_mut(
        &mut self,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<&mut ScalarAnimations> {
        match effect_id {
            Some(id) => Some(&mut self.effect_mut(id)?.animations),
            None => Some(&mut self.animations),
        }
    }

    pub fn local_seconds(&self, time: TimelineTime, frame_rate: FrameRate) -> f64 {
        (time.frames() - self.start.get() as f64) / frame_rate.frames_per_second()
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

    /// Linear gain read through this audio input's volume reference.
    pub fn audio_gain(&self, input_id: &str) -> Option<f32> {
        let audio = self.schema()?.audio_input(input_id)?;
        Some(
            self.properties
                .property(audio.volume_property())?
                .as_f32()
                .expect("validated audio volume property")
                .max(0.),
        )
    }

    pub fn audio_gain_at(&self, input_id: &str, time: TimelineTime) -> Option<f32> {
        let schema = self.schema()?;
        let input = schema.audio_input(input_id)?;
        let property = schema.property(input.volume_property())?;
        Some(
            self.evaluated_property_at(time, None, property)?
                .as_f32()
                .expect("validated audio volume property")
                .max(0.),
        )
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

    pub fn animation_span_frames(&self) -> f64 {
        self.duration.get().saturating_sub(1).max(1) as f64
    }

    pub fn animation_clock(&self, track: &ScalarTrack) -> AnimationClock {
        AnimationClock::new(self.start, self.duration, track.repeat())
    }

    /// Resolve a loaded visual input using only its owner's media declaration.
    pub fn media_input(
        &self,
        effect_id: Option<EffectInstanceId>,
        input_id: &str,
        cache: &MediaMetadataCache,
    ) -> Option<(MediaAsset, MediaPlayback)> {
        let (capabilities, properties) = match effect_id {
            Some(id) => {
                let effect = self.effect(id)?;
                (effect.schema().inputs(), &effect.properties)
            }
            None => (self.schema()?.inputs(), &self.properties),
        };
        let capability = capabilities
            .iter()
            .find(|capability| capability.id() == input_id)?;
        let asset = capability.media_source()?.asset(properties, cache)?;
        let clock =
            capability
                .playback_properties()
                .map_or_else(MediaPlayback::default, |playback| {
                    MediaPlayback::from_properties(playback, properties)
                        .expect("validated media playback properties")
                });
        Some((asset, clock))
    }

    /// Resolve a loaded audio input using only its own audio declaration.
    pub fn audio_input(
        &self,
        input_id: &str,
        cache: &MediaMetadataCache,
    ) -> Option<(MediaAsset, MediaPlayback, bool)> {
        let input = self.schema()?.audio_input(input_id)?;
        let asset = input.media_source().asset(&self.properties, cache)?;
        let playback =
            MediaPlayback::from_properties(input.playback_properties(), &self.properties)
                .expect("validated audio playback properties");
        let preserve_pitch = self
            .properties
            .property(input.preserve_pitch_property())?
            .as_bool()
            .expect("validated preserve-pitch property");
        Some((asset, playback, preserve_pitch))
    }

    pub(super) fn timeline_mapping(&self) -> Option<TimeMapping> {
        Some(
            TimeMapping::from_properties(self.schema()?.timeline()?, &self.properties)
                .expect("validated timeline time mapping"),
        )
    }

    pub(crate) fn validate_playback(&self) -> Result<(), super::TimelineEditError> {
        let owners = self
            .schema()
            .map(|schema| (schema.inputs(), &self.properties))
            .into_iter()
            .chain(
                self.effects
                    .iter()
                    .map(|effect| (effect.schema().inputs(), &effect.properties)),
            );
        for (capabilities, properties) in owners {
            for ids in capabilities
                .iter()
                .filter_map(TextureInput::playback_properties)
            {
                MediaPlayback::from_properties(ids, properties)?;
            }
        }
        if let Some(schema) = self.schema() {
            for audio in schema.audio() {
                MediaPlayback::from_properties(audio.playback_properties(), &self.properties)?;
            }
            if let Some(timeline) = schema.timeline() {
                TimeMapping::from_properties(timeline, &self.properties)?;
            }
        }
        Ok(())
    }

    /// Update a candidate item; callers validate it before committing to the document.
    pub(super) fn store_timeline_mapping(&mut self, mapping: TimeMapping) -> Option<TimeMapping> {
        let schema = self.schema_arc()?.clone();
        let ids = schema.timeline()?;
        for (id, value) in mapping.property_values(ids) {
            self.properties.set(schema.property(id)?, value).ok()?;
        }
        TimeMapping::from_properties(ids, &self.properties).ok()
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
                ResizeMode::Trim => mapping.with_span(
                    duration.get() as f64 / frame_rate.frames_per_second() * mapping.speed(),
                ),
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

    /// Commit the interval. Trimming keeps animation keys at their existing
    /// time positions; stretching keeps their normalized positions.
    pub(super) fn set_interval(&mut self, start: Frame, duration: FrameDuration, mode: ResizeMode) {
        let old_span = self.animation_span_frames();
        let new_span = duration.get().saturating_sub(1).max(1) as f64;
        let offset = start.get() as f64 - self.start.get() as f64;
        let factor = duration.get() as f32 / self.duration.get() as f32;
        for animations in std::iter::once(&mut self.animations)
            .chain(self.effects.iter_mut().map(|effect| &mut effect.animations))
        {
            match mode {
                ResizeMode::Trim => animations.trim(offset, old_span, new_span),
                ResizeMode::Stretch => animations.stretch(factor),
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
        let mapping = TimeMapping::from_properties(
            self.schema()
                .and_then(ItemSchema::timeline)
                .expect("timeline capability is unchanged during property edits"),
            &self.properties,
        )?;
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
        resolve_property(None, self, effect_id, property, Some(time))
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
        match effect_id {
            Some(id) => Some(&self.effect(id)?.properties),
            None => Some(&self.properties),
        }
    }

    pub(super) fn property_values_mut(
        &mut self,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<&mut PropertyValues> {
        match effect_id {
            Some(id) => Some(&mut self.effect_mut(id)?.properties),
            None => Some(&mut self.properties),
        }
    }

    /// First frame after the item.
    pub fn end_exclusive(&self) -> Frame {
        Frame(self.start.0.saturating_add(self.duration.get()))
    }

    pub fn contains(&self, frame: Frame) -> bool {
        frame >= self.start && frame < self.end_exclusive()
    }
}
