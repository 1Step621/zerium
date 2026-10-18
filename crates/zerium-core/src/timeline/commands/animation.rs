use crate::animation::{AnimationRepeat, ScalarAnimations, ScalarTrack};
use crate::property::PropertyValue;
use crate::timeline::history::HistoryKey;
use crate::timeline::{EffectInstanceId, Frame, ItemId, PropertyAddress, TimelineEditor};

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
                let Some(track) = ScalarTrack::from_value(value, ty, item.duration) else {
                    return false;
                };
                editor
                    .animation_store_mut(item_id, effect_id)
                    .is_some_and(|animations| animations.insert(path, track))
            }
        })
    }

    /// Insert at an integer frame within the track's editable forward pattern.
    pub fn insert_animation_stop(
        &mut self,
        address: &PropertyAddress,
        frame: Frame,
        value: PropertyValue,
    ) -> Option<usize> {
        if !self.is_item_selected(address.item_id) {
            return None;
        }
        let schema = self.property_schema(address)?;
        if !schema.is_editable(address.scalar_index)
            || !schema
                .configuration_constraints(address.scalar_index)
                .allows(&value)
        {
            return None;
        }
        let item = self.item(address.item_id)?;
        let track = self.animation_track(address)?;
        let stop_time = item.animation_clock(track).key_time_at(frame);
        let key = HistoryKey::AnimationStopInsertion(address.clone(), stop_time);
        self.edit_project_option(Some(key), |editor| {
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
        self.edit_project_if_changed(Some(key), |editor| {
            let Some(duration) = editor.item(address.item_id).map(|item| item.duration) else {
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
        self.edit_project_if_changed(None, |editor| {
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
