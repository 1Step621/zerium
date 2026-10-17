use crate::animation::{AnimationRepeat, ScalarAnimations, ScalarTrack};
use crate::property::PropertyValue;
use crate::timeline::history::HistoryKey;
use crate::timeline::{
    EffectInstanceId, Frame, ItemId, PropertyAddress, TimelineEditor, TimelineTime,
};

use super::EditContext;

/// A displayed stop and its value-edit context. Only edits between keys link
/// equal neighboring values outside the displayed interval.
#[derive(Clone)]
pub struct AnimationStopEdit {
    pub value: PropertyValue,
    address: PropertyAddress,
    index: usize,
    segment: Option<usize>,
    targets: std::ops::RangeInclusive<usize>,
    context: EditContext,
    has_changes: bool,
}

impl AnimationStopEdit {
    pub fn address(&self) -> &PropertyAddress {
        &self.address
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn is_current(&self, editor: &TimelineEditor) -> bool {
        self.context == editor.edit_context()
    }

    fn history_key(&self) -> HistoryKey {
        HistoryKey::AnimationStopValue(self.address.clone(), self.index)
    }

    /// Compare the displayed key/interval without re-resolving frozen targets.
    pub fn same_value_scope(&self, other: &Self) -> bool {
        self.address == other.address && self.index == other.index && self.segment == other.segment
    }
}

impl TimelineEditor {
    // A frozen value plan can be reused only inside the validated gesture that
    // began at its revision. Other project edits invalidate the outer gesture.
    fn accepts_animation_value_context(&self, context: EditContext) -> bool {
        let current = self.edit_context();
        context == current
            || (context.project_id == current.project_id
                && context.scene_id == current.scene_id
                && self.history_group == Some(HistoryKey::Gesture(context.revision)))
    }

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
                let Some(track) = ScalarTrack::from_value(value, ty, item.duration) else {
                    return false;
                };
                editor
                    .animation_store_mut(item_id, effect_id)
                    .is_some_and(|animations| animations.insert(path, track))
            }
        })
    }

    /// Resolve an independent exact key or interval endpoints with outward linking.
    pub fn property_animation_stops(
        &self,
        address: &PropertyAddress,
        time: TimelineTime,
    ) -> Option<Vec<AnimationStopEdit>> {
        let item = self.item(address.item_id)?;
        let track = self.animation_track(address)?;
        let clock = item.animation_clock(track);
        let indices = track.stop_indices_for_segment(clock.local_frame_at(time));
        let segment = (indices.len() == 2).then(|| indices[0]);
        indices
            .into_iter()
            .map(|index| {
                let mut stop = self.property_animation_stop(address, index)?;
                stop.segment = segment;
                stop.targets = track.value_edit_targets(index, segment)?;
                Some(stop)
            })
            .collect()
    }

    /// Resolve one authored stop independently of its playback occurrence.
    pub fn property_animation_stop(
        &self,
        address: &PropertyAddress,
        index: usize,
    ) -> Option<AnimationStopEdit> {
        let track = self.animation_track(address)?;
        let stop = track.stops().get(index)?;
        Some(AnimationStopEdit {
            value: stop.value().clone(),
            address: address.clone(),
            index,
            segment: None,
            targets: index..=index,
            context: self.edit_context(),
            has_changes: false,
        })
    }

    /// Reject stale controls and validate the value before changing the track.
    pub fn set_property_animation_stop(
        &mut self,
        stop: &AnimationStopEdit,
        value: PropertyValue,
    ) -> bool {
        let address = &stop.address;
        if !self.accepts_animation_value_context(stop.context)
            || !self.is_item_selected(address.item_id)
            || !address.schema(self).is_some_and(|schema| {
                schema.is_editable(address.scalar_index)
                    && schema
                        .scalar_type(address.element_id, address.scalar_index)
                        .is_some_and(|ty| ty.allows(&value))
                    && schema
                        .configuration_constraints(address.scalar_index)
                        .allows(&value)
            })
            || self
                .animation_track(address)
                .is_none_or(|track| track.stops().get(*stop.targets.end()).is_none())
        {
            return false;
        }
        let key = stop.history_key();
        self.edit_project_if_changed(Some(key), |editor| {
            let track = editor
                .animation_track_mut(address)
                .expect("resolved animation track must exist");
            let mut changed = false;
            for index in stop.targets.clone() {
                changed |= track.set_stop(index, value.clone());
            }
            changed
        })
    }

    /// Continue a value operation using its frozen targets. The operation's
    /// own updates advance its context; unrelated edits invalidate it. New
    /// operations use normal timed coalescing, including exact/interval changes.
    pub fn continue_property_animation_stop(
        &mut self,
        stop: &mut AnimationStopEdit,
        value: PropertyValue,
    ) -> bool {
        if !stop.is_current(self) || self.history_group.is_some() {
            return false;
        }
        let key = stop.history_key();
        if stop.has_changes
            && self
                .history
                .is_current_group(&(self.active_scene_id(), key.clone()))
        {
            // A pause within the same operation does not split its Undo entry.
            self.history_group = Some(key);
        }
        let changed = self.set_property_animation_stop(stop, value);
        self.history_group = None;
        if changed {
            stop.context = self.edit_context();
            stop.has_changes = true;
        }
        changed
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
        let schema = address.schema(self)?;
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
            || address
                .schema(self)
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
            || address
                .schema(self)
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
