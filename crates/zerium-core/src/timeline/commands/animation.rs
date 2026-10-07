use super::*;
use crate::property::PropertyPath;
use crate::timeline::{ProjectId, PropertyAddress};

/// A displayed stop and its corresponding value-edit targets. The interval
/// shown by the inspector determines how equal neighboring values are linked.
#[derive(Clone)]
pub struct AnimationStopEdit {
    pub value: PropertyValue,
    pub mixed: bool,
    targets: Vec<(PropertyAddress, usize, Option<usize>)>,
    frame: Frame,
    revision: u64,
    project_id: ProjectId,
    scene_id: Option<SceneId>,
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
        let property_id = target.property_id.clone();
        let element_id = target.element_id;
        let scalar_index = target.scalar_index;
        if enabled
            && self.active_scene_has_binding(item_id, effect_id, &property_id, |binding| {
                binding.conflicts_with_animation(&property_id, element_id, scalar_index)
            })
        {
            return false;
        }
        let before = self.history_snapshot();
        let Some(schema) = self.animation_schema(item_id, effect_id, &property_id) else {
            return false;
        };
        if !schema.is_editable(scalar_index) {
            return false;
        }
        let address = PropertyPath::new(property_id.clone(), element_id, scalar_index);
        let changed = if !enabled {
            self.animation_store_mut(item_id, effect_id)
                .is_some_and(|animations| animations.remove(&address))
        } else {
            if !schema.is_animatable(scalar_index) {
                return false;
            }
            let Some(item) = self.active_document().item(item_id) else {
                return false;
            };
            let values = match effect_id {
                Some(effect_id) => item
                    .effects
                    .iter()
                    .find(|effect| effect.id == effect_id)
                    .map(|effect| effect.properties.clone()),
                None => Some(self.materialized_item(item).properties),
            };
            let Some(values) = values else {
                return false;
            };
            let Some(resolved) = values
                .property(&property_id)
                .and_then(|value| schema.resolve_scalar(value, element_id, scalar_index))
            else {
                return false;
            };
            let Some(track) = ScalarTrack::from_value(resolved.value.clone(), resolved.ty) else {
                return false;
            };
            self.animation_store_mut(item_id, effect_id)
                .is_some_and(|animations| animations.insert(address, track))
        };
        self.finish_project_edit_if_changed(changed, Some(before), None)
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
                let stops = track
                    .stop_indices_for_segment(item.animation_progress_at_time(time))
                    .into_iter()
                    .map(|index| {
                        let stop = &track.stops()[index];
                        let frame = TimelineTime::from_frames(
                            item.animation_timeline_frame(stop.position()),
                        )
                        .nearest_frame();
                        (index, frame, stop.value())
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
                    .map(|(_, frame, _)| frame)
                    .eq(source.iter().map(|(_, frame, _)| frame))
            })
        {
            return None;
        }
        Some(
            source
                .iter()
                .enumerate()
                .map(|(offset, (_, frame, value))| AnimationStopEdit {
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
                    frame: *frame,
                    revision: self.project_revision(),
                    project_id: self.project().id,
                    scene_id: self.active_scene_id(),
                })
                .collect(),
        )
    }

    /// Write exactly the targets resolved for a displayed input. Reject stale
    /// controls and validate every value before changing any track.
    pub fn set_property_animation_stop(
        &mut self,
        stop: &AnimationStopEdit,
        value: PropertyValue,
    ) -> bool {
        if stop.revision != self.project_revision()
            || stop.project_id != self.project().id
            || stop.scene_id != self.active_scene_id()
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
            stop.frame,
        );
        let before = self.history_snapshot_for_edit(Some(&key));
        let mut changed = false;
        for (address, index, segment) in &stop.targets {
            changed |= self
                .animation_track_mut(address.item_id, address.effect_id, &address.path())
                .expect("resolved animation track must exist")
                .set_stop(*index, value.clone(), *segment);
        }
        self.finish_project_edit_if_changed(changed, before, Some(key))
    }

    pub fn set_property_animation_pair_stop_at(
        &mut self,
        target: &PropertyAddress,
        position: f32,
        value: [f32; 2],
    ) -> bool {
        if !self.is_item_selected(target.item_id) {
            return false;
        }
        let item_id = target.item_id;
        let effect_id = target.effect_id;
        let property_id = target.property_id.as_str();
        let element_id = target.element_id;
        let Some(item) = self.active_document().item(item_id) else {
            return false;
        };
        let stop_frame = Frame::new(item.animation_timeline_frame(position).round().max(0.) as u64);
        let key = HistoryKey::AnimationPairStopValue(
            item_id,
            effect_id,
            property_id.to_owned(),
            element_id,
            stop_frame,
        );
        let before = self.history_snapshot_for_edit(Some(&key));
        let Some(schema) = self.animation_schema(item_id, effect_id, property_id) else {
            return false;
        };
        let editable = [0_usize, 1].map(|scalar_index| {
            schema.is_editable(Some(scalar_index))
                && schema
                    .configuration_constraints(Some(scalar_index))
                    .allows(&PropertyValue::F32(value[scalar_index]))
        });
        let mut changed = false;
        for scalar_index in 0..2 {
            if !editable[scalar_index] {
                continue;
            }
            let address = PropertyPath::new(property_id, element_id, Some(scalar_index));
            let Some(index) = self
                .animation_track(item_id, effect_id, &address)
                .and_then(|track| track.stop_index_at(position))
            else {
                continue;
            };
            changed |= self
                .animation_track_mut(item_id, effect_id, &address)
                .is_some_and(|track| {
                    track.set_stop(index, PropertyValue::F32(value[scalar_index]), None)
                });
        }
        self.finish_project_edit_if_changed(changed, before, Some(key))
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
        let stop_frame =
            TimelineTime::from_frames(item.animation_timeline_frame(position)).nearest_frame();
        let key = HistoryKey::AnimationStopValue(vec![address.clone()], stop_frame);
        let before = self.history_snapshot_for_edit(Some(&key));
        let inserted = self
            .animation_track_mut(address.item_id, address.effect_id, &address.path())?
            .insert_stop(position, value);
        if inserted.is_some() {
            self.finish_project_edit(before, Some(key));
        }
        inserted
    }

    pub fn remove_animation_stop(&mut self, address: &PropertyAddress, stop: usize) -> bool {
        if !self.is_item_selected(address.item_id)
            || address
                .schema(self)
                .is_none_or(|schema| !schema.is_editable(address.scalar_index))
        {
            return false;
        }
        let before = self.history_snapshot();
        let changed = self
            .animation_track_mut(address.item_id, address.effect_id, &address.path())
            .is_some_and(|track| track.remove_stop(stop));
        self.finish_project_edit_if_changed(changed, Some(before), None)
    }

    fn animation_schema(
        &self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
    ) -> Option<PropertySchema> {
        self.property_schema(item_id, effect_id, property_id)
            .cloned()
    }

    fn animation_store_mut(
        &mut self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<&mut ScalarAnimations> {
        let item = self.active_document_mut().item_mut(item_id)?;
        match effect_id {
            Some(effect_id) => item
                .effects
                .iter_mut()
                .find(|effect| effect.id == effect_id)
                .map(|effect| &mut effect.animations),
            None => Some(&mut item.animations),
        }
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
