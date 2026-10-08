use crate::animation::{AnimationRepeat, ScalarAnimations, ScalarTrack};
use crate::property::{PropertyPath, PropertyValue};
use crate::timeline::history::HistoryKey;
use crate::timeline::{
    EffectInstanceId, ItemId, PropertyAddress, TimelineEditor, TimelineItem, TimelineTime,
};

use super::EditContext;

/// A displayed stop and its corresponding value-edit targets. The interval
/// shown by the inspector determines how equal neighboring values are linked.
#[derive(Clone)]
pub struct AnimationStopEdit {
    pub value: PropertyValue,
    pub mixed: bool,
    targets: Vec<(PropertyAddress, usize, Option<usize>)>,
    time: TimelineTime,
    context: EditContext,
}

impl AnimationStopEdit {
    pub fn address(&self) -> &PropertyAddress {
        &self.targets[0].0
    }

    pub fn index(&self) -> usize {
        self.targets[0].1
    }
}

impl TimelineEditor {
    pub fn set_property_animation_enabled(
        &mut self,
        target: &PropertyAddress,
        enabled: bool,
    ) -> bool {
        if !self.is_item_selected(target.item_id) {
            return false;
        }
        let item_id = target.item_id;
        let effect_id = target.effect_id;
        let property_id = &target.property_id;
        let element_id = target.element_id;
        let scalar_index = target.scalar_index;
        if enabled
            && self.active_scene_has_binding(item_id, effect_id, property_id, |binding| {
                binding.conflicts_with_animation(property_id, element_id, scalar_index)
            })
        {
            return false;
        }
        self.edit_project_if_changed(None, |editor| {
            let Some(schema) = target.schema(editor).cloned() else {
                return false;
            };
            if !schema.is_editable(scalar_index) {
                return false;
            }
            let path = target.path();
            if !enabled {
                editor
                    .animation_store_mut(item_id, effect_id)
                    .is_some_and(|animations| animations.remove(&path))
            } else {
                if !schema.is_animatable(scalar_index) {
                    return false;
                }
                let Some(item) = editor.active_document().item(item_id) else {
                    return false;
                };
                let value = editor.property_value(target);
                let Some((value, ty)) = value.zip(schema.scalar_type(element_id, scalar_index))
                else {
                    return false;
                };
                let Some(track) = ScalarTrack::from_value(value, ty, item.duration.get() as f32)
                else {
                    return false;
                };
                editor
                    .animation_store_mut(item_id, effect_id)
                    .is_some_and(|animations| animations.insert(path, track))
            }
        })
    }

    /// Resolve the current stop or interval on already materialized inspector
    /// items. Common inputs require matching timeline times on every item.
    pub fn property_animation_stops(
        &self,
        items: &[TimelineItem],
        target: &PropertyAddress,
        time: TimelineTime,
    ) -> Option<Vec<AnimationStopEdit>> {
        let source = items.iter().find(|item| item.id == target.item_id)?;
        let tracks = items
            .iter()
            .map(|item| {
                let address = target.on_item(source, item)?;
                let track = item.animation_track(
                    address.effect_id,
                    &address.property_id,
                    address.element_id,
                    address.scalar_index,
                )?;
                let clock = item.animation_clock(track);
                let stops = track
                    .stop_indices_for_segment(clock.progress_at(time))
                    .into_iter()
                    .map(|index| {
                        let stop = &track.stops()[index];
                        let stop_time = clock.time_at(stop.position()).rounded();
                        (index, stop_time, stop.value())
                    })
                    .collect::<Vec<_>>();
                Some((address, stops))
            })
            .collect::<Option<Vec<_>>>()?;
        let (_, source) = tracks.first()?;
        if source.is_empty()
            || !tracks.iter().all(|(_, stops)| {
                stops
                    .iter()
                    .map(|(_, time, _)| time)
                    .eq(source.iter().map(|(_, time, _)| time))
            })
        {
            return None;
        }
        Some(
            source
                .iter()
                .enumerate()
                .map(|(offset, (_, time, value))| AnimationStopEdit {
                    value: (*value).clone(),
                    mixed: tracks
                        .iter()
                        .skip(1)
                        .any(|(_, stops)| stops[offset].2 != *value),
                    targets: tracks
                        .iter()
                        .map(|(address, stops)| {
                            (
                                address.clone(),
                                stops[offset].0,
                                (stops.len() == 2).then_some(stops[0].0),
                            )
                        })
                        .collect(),
                    time: *time,
                    context: self.edit_context(),
                })
                .collect(),
        )
    }

    /// Resolve one authored stop independently of its playback occurrence.
    pub fn property_animation_stop(
        &self,
        address: &PropertyAddress,
        index: usize,
    ) -> Option<AnimationStopEdit> {
        let item = self.item(address.item_id)?;
        let track = self.animation_track(item.id, address.effect_id, &address.path())?;
        let stop = track.stops().get(index)?;
        Some(AnimationStopEdit {
            value: stop.value().clone(),
            mixed: false,
            targets: vec![(address.clone(), index, None)],
            time: item
                .animation_clock(track)
                .time_at(stop.position())
                .rounded(),
            context: self.edit_context(),
        })
    }

    /// Write exactly the targets resolved for a displayed input. Reject stale
    /// controls and validate every value before changing any track.
    pub fn set_property_animation_stop(
        &mut self,
        stop: &AnimationStopEdit,
        value: PropertyValue,
    ) -> bool {
        if stop.context != self.edit_context()
            || !stop.targets.iter().all(|(address, index, _)| {
                self.is_item_selected(address.item_id)
                    && address.schema(self).is_some_and(|schema| {
                        schema.is_editable(address.scalar_index)
                            && schema
                                .scalar_type(address.element_id, address.scalar_index)
                                .is_some_and(|ty| ty.allows(&value))
                            && schema
                                .configuration_constraints(address.scalar_index)
                                .allows(&value)
                    })
                    && self
                        .animation_track(address.item_id, address.effect_id, &address.path())
                        .is_some_and(|track| track.stops().get(*index).is_some())
            })
        {
            return false;
        }
        let key = HistoryKey::AnimationStopValue(
            stop.targets
                .iter()
                .map(|(address, _, _)| address.clone())
                .collect(),
            stop.time,
        );
        self.edit_project_if_changed(Some(key), |editor| {
            let mut changed = false;
            for (address, index, segment) in &stop.targets {
                changed |= editor
                    .animation_track_mut(address.item_id, address.effect_id, &address.path())
                    .expect("resolved animation track must exist")
                    .set_stop(*index, value.clone(), *segment);
            }
            changed
        })
    }

    pub fn insert_animation_stop(
        &mut self,
        address: &PropertyAddress,
        position: f32,
        value: PropertyValue,
    ) -> Option<usize> {
        if !self.is_item_selected(address.item_id) {
            return None;
        }
        let schema = address.schema(self)?;
        if !schema.is_editable(address.scalar_index)
            || !schema
                .configuration_constraints(address.scalar_index)
                .allows(&value)
        {
            return None;
        }
        let item = self.item(address.item_id)?;
        let track = item.animation_track(
            address.effect_id,
            &address.property_id,
            address.element_id,
            address.scalar_index,
        )?;
        let stop_time = item.animation_clock(track).time_at(position).rounded();
        let key = HistoryKey::AnimationStopValue(vec![address.clone()], stop_time);
        self.edit_project_option(Some(key), |editor| {
            editor
                .animation_track_mut(address.item_id, address.effect_id, &address.path())?
                .insert_stop(position, value)
        })
    }

    pub fn set_animation_repeat(
        &mut self,
        address: &PropertyAddress,
        repeat: AnimationRepeat,
    ) -> bool {
        if !self.is_item_selected(address.item_id)
            || address
                .schema(self)
                .is_none_or(|schema| !schema.is_editable(address.scalar_index))
        {
            return false;
        }
        let key = HistoryKey::AnimationRepeat(address.clone());
        self.edit_project_if_changed(Some(key), |editor| {
            editor
                .animation_track_mut(address.item_id, address.effect_id, &address.path())
                .is_some_and(|track| track.set_repeat(repeat))
        })
    }

    pub fn remove_animation_stop(&mut self, address: &PropertyAddress, stop: usize) -> bool {
        if !self.is_item_selected(address.item_id)
            || address
                .schema(self)
                .is_none_or(|schema| !schema.is_editable(address.scalar_index))
        {
            return false;
        }
        self.edit_project_if_changed(None, |editor| {
            editor
                .animation_track_mut(address.item_id, address.effect_id, &address.path())
                .is_some_and(|track| track.remove_stop(stop))
        })
    }

    fn animation_store_mut(
        &mut self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<&mut ScalarAnimations> {
        self.active_document_mut()
            .item_mut(item_id)?
            .animations_mut(effect_id)
    }

    pub(super) fn animation_track(
        &self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        address: &PropertyPath,
    ) -> Option<&ScalarTrack> {
        self.active_document().item(item_id)?.animation_track(
            effect_id,
            address.property_id(),
            address.element_id(),
            address.scalar_index(),
        )
    }

    pub(super) fn animation_track_mut(
        &mut self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        address: &PropertyPath,
    ) -> Option<&mut ScalarTrack> {
        self.animation_store_mut(item_id, effect_id)?
            .track_mut(address)
    }
}
