use crate::animation::{AnimationRepeat, ScalarAnimations, ScalarTrack};
use crate::timeline::history::{EditImpact, HistoryKey};
use crate::timeline::{
    EffectInstanceId, Frame, ItemId, PropertyAddress, TimelineEditor, TimelineTime,
};

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
        self.edit_project_if_changed(EditImpact::Values, None, |editor| {
            let Some(schema) = editor.property_schema(target).cloned() else {
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
                let Some(track) =
                    ScalarTrack::from_value(value, ty, item.duration(editor.frame_rate()))
                else {
                    return false;
                };
                editor
                    .animation_store_mut(item_id, effect_id)
                    .is_some_and(|animations| animations.insert(path, track))
            }
        })
    }

    pub fn animation_stop_insertion_frame(
        &self,
        address: &PropertyAddress,
        time: TimelineTime,
    ) -> Option<Frame> {
        if !self.is_item_selected(address.item_id)
            || !self
                .property_schema(address)?
                .is_editable(address.scalar_index)
        {
            return None;
        }
        let item = self.item(address.item_id)?;
        let track = self.animation_track(address)?;
        let frame = item.animation_clock(track).pattern_frame_at(time);
        if frame.get() == 0
            || frame.get() >= track.span().get()
            || track.stop_index_at(frame).is_some()
        {
            return None;
        }
        Some(frame)
    }

    /// Insert within the editable forward pattern, inheriting the nearest stop's value.
    pub fn insert_animation_stop_at_time(
        &mut self,
        address: &PropertyAddress,
        time: TimelineTime,
    ) -> Option<usize> {
        let frame = self.animation_stop_insertion_frame(address, time)?;
        let value = self
            .animation_track(address)?
            .stops()
            .iter()
            .min_by_key(|stop| stop.frame().get().abs_diff(frame.get()))?
            .value()
            .clone();
        let schema = self.property_schema(address)?;
        if !schema
            .configuration_constraints(address.scalar_index)
            .allows(&value)
        {
            return None;
        }
        let item = self.item(address.item_id)?;
        let track = self.animation_track(address)?;
        let stop_time = item.animation_clock(track).key_time_at(frame);
        let key = HistoryKey::AnimationStopInsertion(address.clone(), stop_time);
        self.edit_project_option(EditImpact::Values, Some(key), |editor| {
            editor
                .animation_track_mut(address)?
                .insert_stop(frame, value)
        })
    }

    pub fn set_animation_repeat(
        &mut self,
        address: &PropertyAddress,
        repeat: AnimationRepeat,
    ) -> bool {
        if !self.is_item_selected(address.item_id)
            || self
                .property_schema(address)
                .is_none_or(|schema| !schema.is_editable(address.scalar_index))
        {
            return false;
        }
        let key = HistoryKey::AnimationRepeat(address.clone());
        self.edit_project_if_changed(EditImpact::Values, Some(key), |editor| {
            let Some(duration) = editor
                .item(address.item_id)
                .map(|item| item.duration(editor.frame_rate()))
            else {
                return false;
            };
            editor
                .animation_track_mut(address)
                .is_some_and(|track| track.set_repeat(repeat, duration))
        })
    }

    pub fn remove_animation_stop(&mut self, address: &PropertyAddress, stop: usize) -> bool {
        if !self.is_item_selected(address.item_id)
            || self
                .property_schema(address)
                .is_none_or(|schema| !schema.is_editable(address.scalar_index))
        {
            return false;
        }
        self.edit_project_if_changed(EditImpact::Values, None, |editor| {
            editor
                .animation_track_mut(address)
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

    /// Resolve an authored track in the active document.
    pub fn animation_track(&self, address: &PropertyAddress) -> Option<&ScalarTrack> {
        self.item(address.item_id)?.animation_track(
            address.effect_id,
            &address.property_id,
            address.element_id,
            address.scalar_index,
        )
    }

    pub(super) fn animation_track_mut(
        &mut self,
        address: &PropertyAddress,
    ) -> Option<&mut ScalarTrack> {
        self.animation_store_mut(address.item_id, address.effect_id)?
            .track_mut(&address.path())
    }
}
