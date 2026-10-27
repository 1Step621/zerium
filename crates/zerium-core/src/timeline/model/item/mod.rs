mod aspect_ratio;
mod blend_mode;
pub(in crate::timeline) mod property;

pub use self::{aspect_ratio::AspectRatio, blend_mode::BlendMode, property::PropertyAddress};

use std::sync::Arc;

use crate::{
    animation::{AnimationClock, ScalarAnimations, ScalarTrack},
    plugin::{EffectSchema, ItemSchema},
    property::{
        PropertyElementId, PropertyOverrides, PropertyPath, PropertySchema, PropertyValue,
        PropertyValues,
    },
    timeline::{FrameRate, ResizeMode, TimeMapping},
};

use super::{
    EffectInstanceId, ItemId, SceneId,
    time::{Frame, FrameDuration},
};

const MAX_ITEM_LABEL_CHARS: usize = 40;

/// Initial authored values and placement, before an item receives its identity.
pub struct ItemCreation {
    pub start: Frame,
    /// Omission uses the source's initial duration. Playback items derive it
    /// from their playback properties and reject an explicit duration.
    pub duration: Option<FrameDuration>,
    pub properties: PropertyOverrides,
}

/// A new timeline interval for an existing item.
pub struct ItemInterval {
    pub item_id: ItemId,
    pub start: Frame,
    pub duration: FrameDuration,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ItemSource {
    Plugin { plugin_id: String, item_id: String },
    Scene(SceneId),
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

pub(in crate::timeline) fn size_values(
    values: &PropertyOverrides,
    schema: &ItemSchema,
) -> Option<[f32; 2]> {
    let property = schema.size_property()?;
    let value = values.resolve(property);
    Some([
        value.scalar_at(Some(0))?.numeric_scalar()? as f32,
        value.scalar_at(Some(1))?.numeric_scalar()? as f32,
    ])
}

pub(in crate::timeline) fn set_size_values(
    values: &mut PropertyOverrides,
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
    pub hidden: bool,
    pub plugin_id: String,
    pub effect_id: String,
    pub properties: PropertyOverrides,
    pub animations: ScalarAnimations,
    pub aspect_ratio: Option<AspectRatio>,
    pub schema: Arc<EffectSchema>,
}

impl EffectInstance {
    pub fn schema(&self) -> &EffectSchema {
        &self.schema
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
    pub hidden: bool,
    pub(crate) start: Frame,
    pub(crate) fixed_duration: Option<FrameDuration>,
    pub blend_mode: BlendMode,
    pub kind: TimelineItemKind,
    pub properties: PropertyOverrides,
    pub animations: ScalarAnimations,
    pub aspect_ratio: Option<AspectRatio>,
    pub effects: Vec<EffectInstance>,
}

/// One resolved property owner, shared by schema lookup, evaluation and editing.
pub(in crate::timeline) struct PropertyOwnerRef<'a> {
    pub values: &'a PropertyOverrides,
    pub animations: &'a ScalarAnimations,
    pub schemas: &'a [PropertySchema],
}

pub(in crate::timeline) struct PropertyOwnerMut<'a> {
    pub values: &'a mut PropertyOverrides,
    pub animations: &'a mut ScalarAnimations,
}

impl TimelineItem {
    pub(in crate::timeline) fn scene_instance(
        id: ItemId,
        start: Frame,
        duration: FrameDuration,
        scene_id: SceneId,
    ) -> Self {
        Self {
            id,
            hidden: false,
            start,
            fixed_duration: Some(duration),
            blend_mode: BlendMode::Normal,
            kind: TimelineItemKind::Scene { scene_id },
            properties: PropertyOverrides::default(),
            animations: ScalarAnimations::default(),
            aspect_ratio: None,
            effects: Vec::new(),
        }
    }

    pub(in crate::timeline) fn effect(&self, id: EffectInstanceId) -> Option<&EffectInstance> {
        self.effects.iter().find(|effect| effect.id == id)
    }

    pub(in crate::timeline) fn effect_mut(
        &mut self,
        id: EffectInstanceId,
    ) -> Option<&mut EffectInstance> {
        self.effects.iter_mut().find(|effect| effect.id == id)
    }

    pub(in crate::timeline) fn property_owner(
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

    pub(in crate::timeline) fn property_owner_mut(
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

    pub(in crate::timeline) fn animations(
        &self,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<&ScalarAnimations> {
        Some(self.property_owner(effect_id)?.animations)
    }

    pub(in crate::timeline) fn animations_mut(
        &mut self,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<&mut ScalarAnimations> {
        Some(self.property_owner_mut(effect_id)?.animations)
    }

    pub fn schema(&self) -> Option<&ItemSchema> {
        self.schema_arc().map(AsRef::as_ref)
    }

    pub(in crate::timeline) fn schema_arc(&self) -> Option<&Arc<ItemSchema>> {
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
            .map(|property| self.properties.resolve(property))
            .and_then(|value| match value {
                PropertyValue::String(value) => concise_label(&value),
                _ => None,
            })
        {
            return Some(label);
        }

        let asset_labels = schema
            .file_properties()
            .filter_map(|property| {
                let value = self.properties.resolve(property);
                value.file()?.file_stem()?.to_str().and_then(concise_label)
            })
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
        AnimationClock::new(self.start(), track)
    }

    pub fn start(&self) -> Frame {
        self.start
    }

    pub fn duration(&self, rate: FrameRate) -> FrameDuration {
        self.fixed_duration.unwrap_or_else(|| {
            self.source_mapping()
                .expect("playback clip")
                .timeline_duration(rate)
        })
    }

    pub(crate) fn try_playback(
        &self,
    ) -> Result<crate::media::MediaPlayback, crate::timeline::TimelineEditError> {
        let Some(schema) = self.schema() else {
            return Ok(crate::media::MediaPlayback::default());
        };
        let Some(playback) = schema.playback() else {
            return Ok(crate::media::MediaPlayback::default());
        };
        let mut values = PropertyValues::default();
        for id in playback.property_ids() {
            let property = schema.property(id).expect("validated playback property");
            values
                .set(property, self.properties.resolve(property))
                .expect("authored property contract");
        }
        playback.resolve(&values)
    }

    pub fn playback(&self) -> crate::media::MediaPlayback {
        self.try_playback().expect("validated item playback")
    }

    fn source_mapping(&self) -> Option<TimeMapping> {
        self.schema()?.playback()?;
        Some(self.playback().mapping())
    }

    pub(in crate::timeline) fn authored_property(
        &self,
        effect: Option<EffectInstanceId>,
        property: &PropertySchema,
    ) -> Option<PropertyValue> {
        Some(self.property_overrides(effect)?.resolve(property))
    }

    pub(crate) fn validate_playback(
        &self,
        rate: FrameRate,
    ) -> Result<(), crate::timeline::TimelineEditError> {
        self.try_playback()?;
        let derived = self.schema().and_then(|schema| schema.playback()).is_some();
        if derived == self.fixed_duration.is_some() {
            return Err(crate::timeline::TimelineEditError::InvalidSourceRange);
        }
        self.start
            .get()
            .checked_add(self.duration(rate).get())
            .ok_or(crate::timeline::TimelineEditError::PlacementUnavailable)?;
        Ok(())
    }

    pub(in crate::timeline) fn source_speed_bounds(&self) -> (f64, f64) {
        let (min, max) = self
            .schema()
            .and_then(|schema| schema.property(&schema.playback()?.playback_speed))
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

    pub(in crate::timeline) fn left_extension(&self, rate: FrameRate) -> Option<u64> {
        Some(self.source_mapping()?.left_extension(rate))
    }

    pub(in crate::timeline) fn stretch_duration_bounds(
        &self,
        rate: FrameRate,
    ) -> Option<(u64, u64)> {
        Some(
            self.source_mapping()?
                .stretch_duration_bounds(self.source_speed_bounds(), rate),
        )
    }

    pub(in crate::timeline) fn move_to(&mut self, start: Frame) {
        self.start = start;
    }

    fn write_source_mapping(&mut self, mapping: TimeMapping) -> Option<()> {
        let schema = self.schema_arc()?.clone();
        let playback = schema.playback()?;
        for (id, value) in [
            (&playback.source_start, mapping.source_offset()),
            (&playback.source_duration, mapping.source_span()),
            (&playback.playback_speed, mapping.speed()),
        ] {
            let property = schema.property(id)?;
            let value = PropertyValue::F32(value as f32);
            if self.properties.resolve(property) != value {
                self.properties.set(property, value).ok()?;
            }
        }
        Some(())
    }

    pub(in crate::timeline) fn initialize_source(
        &mut self,
        span: std::time::Duration,
        rate: FrameRate,
    ) -> Option<()> {
        let old_duration = self.duration(rate);
        let mapping = self.source_mapping()?;
        TimeMapping::new(0., span.as_secs_f32(), mapping.speed() as f32).ok()?;
        let schema = self.schema_arc()?.clone();
        let playback = schema.playback()?;
        for (id, value) in [
            (&playback.source_start, 0.),
            (&playback.source_duration, span.as_secs_f32()),
        ] {
            self.properties
                .set(schema.property(id)?, PropertyValue::F32(value))
                .ok()?;
        }
        self.resize_animations(
            self.start,
            old_duration,
            self.duration(rate),
            ResizeMode::Trim,
        );
        Some(())
    }

    pub(in crate::timeline) fn expand_at(&mut self, point: Frame, extra: u64, rate: FrameRate) {
        if extra == 0 {
            return;
        }
        if !self.contains(point, rate) {
            if self.start > point {
                self.move_to(Frame::new(self.start.get().saturating_add(extra)));
            }
            return;
        }
        let duration =
            FrameDuration::new_saturating(self.duration(rate).get().saturating_add(extra));
        let local_frame = Frame::new(point.get().saturating_sub(self.start.get()));
        for animations in std::iter::once(&mut self.animations)
            .chain(self.effects.iter_mut().map(|effect| &mut effect.animations))
        {
            animations.extend_at(local_frame, duration);
        }
        if let Some(mapping) = self.source_mapping() {
            self.write_source_mapping(
                mapping
                    .with_duration(duration, rate)
                    .expect("expanded source range"),
            )
            .expect("expanded source properties");
        } else {
            self.fixed_duration = Some(duration);
        }
    }

    pub(in crate::timeline) fn resize_to(
        &mut self,
        start: Frame,
        duration: FrameDuration,
        mode: ResizeMode,
        rate: FrameRate,
    ) -> Option<()> {
        let old_start = self.start;
        let old_duration = self.duration(rate);
        if let Some(mapping) = self.source_mapping() {
            let delta = (i128::from(start.get()) - i128::from(old_start.get())) as f64;
            self.write_source_mapping(mapping.resized(delta, duration, mode, rate)?)?;
        } else {
            self.fixed_duration = Some(duration);
        }
        self.start = start;
        self.resize_animations(old_start, old_duration, self.duration(rate), mode);
        self.validate_playback(rate).ok()
    }

    pub(in crate::timeline) fn retime_to(
        &mut self,
        start: Frame,
        duration: FrameDuration,
        previous_duration: FrameDuration,
    ) {
        let old_start = self.start;
        self.start = start;
        if self.fixed_duration.is_some() {
            self.fixed_duration = Some(duration);
        }
        self.resize_animations(old_start, previous_duration, duration, ResizeMode::Stretch);
    }

    pub(in crate::timeline) fn resize_animations(
        &mut self,
        previous_start: Frame,
        previous_duration: FrameDuration,
        duration: FrameDuration,
        mode: ResizeMode,
    ) {
        let offset = i128::from(self.start.get()) - i128::from(previous_start.get());
        if offset == 0 && previous_duration == duration {
            return;
        }
        for animations in std::iter::once(&mut self.animations)
            .chain(self.effects.iter_mut().map(|effect| &mut effect.animations))
        {
            match mode {
                ResizeMode::Trim => animations.trim(offset, duration),
                ResizeMode::Stretch => animations.stretch(previous_duration, duration),
            }
        }
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

    /// Authored scalar tracks of this item and its effects, with stable addresses.
    pub fn animation_tracks(&self) -> impl Iterator<Item = (PropertyAddress, &ScalarTrack)> {
        std::iter::once((None, &self.animations))
            .chain(
                self.effects
                    .iter()
                    .map(|effect| (Some(effect.id), &effect.animations)),
            )
            .flat_map(move |(effect_id, animations)| {
                animations.tracks().map(move |(path, track)| {
                    (
                        PropertyAddress {
                            item_id: self.id,
                            effect_id,
                            property_id: path.property_id().to_owned(),
                            element_id: path.element_id(),
                            scalar_index: path.scalar_index(),
                        },
                        track,
                    )
                })
            })
    }

    pub fn property_overrides(
        &self,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<&PropertyOverrides> {
        Some(self.property_owner(effect_id)?.values)
    }

    pub(in crate::timeline) fn property_overrides_mut(
        &mut self,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<&mut PropertyOverrides> {
        Some(self.property_owner_mut(effect_id)?.values)
    }

    /// First frame after the item.
    pub fn end_exclusive(&self, rate: FrameRate) -> Frame {
        Frame(self.start().get().saturating_add(self.duration(rate).get()))
    }

    pub fn contains(&self, frame: Frame, rate: FrameRate) -> bool {
        frame >= self.start() && frame < self.end_exclusive(rate)
    }
}

impl TimelineItem {
    pub(in crate::timeline) fn set_aspect_ratio(
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

    pub(in crate::timeline) fn add_effect(
        &mut self,
        instance_id: EffectInstanceId,
        plugin_id: &str,
        effect_id: &str,
        schema: Arc<EffectSchema>,
        mut properties: PropertyOverrides,
        frame_rate: FrameRate,
    ) -> bool {
        crate::plugin::initialize_frame_rates(
            schema.inputs(),
            schema.properties(),
            &mut properties,
            frame_rate,
        );
        let aspect_ratio = schema
            .aspect_lock_default()
            .then(|| {
                schema
                    .aspect_lock_property()
                    .and_then(|property| AspectRatio::from_value(&properties.resolve(property)))
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
            hidden: false,
            plugin_id: plugin_id.to_owned(),
            effect_id: effect_id.to_owned(),
            properties,
            animations: ScalarAnimations::default(),
            aspect_ratio,
            schema,
        });
        true
    }

    pub(in crate::timeline) fn remove_effect(&mut self, effect_id: EffectInstanceId) -> bool {
        let previous_len = self.effects.len();
        self.effects.retain(|effect| effect.id != effect_id);
        self.effects.len() != previous_len
    }

    pub(in crate::timeline) fn move_effect(
        &mut self,
        effect_id: EffectInstanceId,
        target_index: usize,
    ) -> bool {
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
