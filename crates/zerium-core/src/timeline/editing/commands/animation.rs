use crate::{
    animation::{AnimationRepeat, ScalarAnimations, ScalarTrack, SegmentInterpolation},
    timeline::{
        EffectInstanceId, Frame, ItemId, PropertyAddress, TimelineEditor, TimelineItem,
        TimelineTime,
        editing::{
            gesture::EditSession,
            history::{EditImpact, HistoryKey},
        },
    },
};
use std::{
    collections::{HashMap, hash_map::Entry},
    ops::RangeInclusive,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationEditTarget {
    Stop(usize),
    Segment(usize),
}

impl AnimationEditTarget {
    fn index(self) -> usize {
        match self {
            Self::Stop(index) | Self::Segment(index) => index,
        }
    }

    fn with_index(self, index: usize) -> Self {
        match self {
            Self::Stop(_) => Self::Stop(index),
            Self::Segment(_) => Self::Segment(index),
        }
    }

    fn times(
        self,
        item: &TimelineItem,
        track: &ScalarTrack,
    ) -> Option<(TimelineTime, TimelineTime)> {
        let clock = item.animation_clock(track);
        let frame = |index: usize| {
            track
                .stops()
                .get(index)
                .map(|stop| clock.key_time_at(stop.frame()))
        };
        match self {
            AnimationEditTarget::Stop(index) => Some((frame(index)?, frame(index)?)),
            AnimationEditTarget::Segment(index) => Some((frame(index)?, frame(index + 1)?)),
        }
    }
}

struct Target {
    address: PropertyAddress,
    index: usize,
}

enum AnimationEditKind {
    Stop(RangeInclusive<TimelineTime>),
    Segment,
}

/// Fixed edit scope and Undo session, also used to preview synchronization.
/// Creating an edit is read-only; history starts with its first mutation.
pub struct AnimationEdit {
    targets: Vec<Target>,
    kind: AnimationEditKind,
    gesture: EditSession,
}

impl AnimationEdit {
    pub fn is_current(&self, editor: &TimelineEditor) -> bool {
        self.gesture.is_current(editor)
    }

    /// Each affected scalar property occurs exactly once.
    pub fn properties(&self) -> impl Iterator<Item = &PropertyAddress> {
        self.targets.iter().map(|target| &target.address)
    }

    pub fn time_range(&self) -> Option<&RangeInclusive<TimelineTime>> {
        match &self.kind {
            AnimationEditKind::Stop(range) => Some(range),
            AnimationEditKind::Segment => None,
        }
    }
}

fn stop_move_range(
    item: &TimelineItem,
    track: &ScalarTrack,
    index: usize,
) -> Option<RangeInclusive<TimelineTime>> {
    let clock = item.animation_clock(track);
    let range = track.stop_move_range(index)?;
    Some(clock.key_time_at(*range.start())..=clock.key_time_at(*range.end()))
}

impl TimelineEditor {
    pub fn begin_animation_edit(
        &self,
        address: &PropertyAddress,
        part: AnimationEditTarget,
        synchronize: bool,
    ) -> Option<AnimationEdit> {
        let source = self.item(address.item_id)?;
        let track = self.animation_track(address)?;
        if !self
            .property_schema(address)?
            .is_editable(address.scalar_index)
        {
            return None;
        }
        let times = part.times(source, track)?;
        let (interpolation, mut kind) = match part {
            AnimationEditTarget::Stop(index) => (
                None,
                AnimationEditKind::Stop(stop_move_range(source, track, index)?),
            ),
            AnimationEditTarget::Segment(index) => (
                Some(*track.interpolations().get(index)?),
                AnimationEditKind::Segment,
            ),
        };
        let mut targets = vec![Target {
            address: address.clone(),
            index: part.index(),
        }];
        // Direct manipulation always includes the source item. Only a selected
        // source extends the gesture to the other selected items.
        if synchronize {
            let include_selection = self.is_item_selected(address.item_id);
            let item_ids = std::iter::once(address.item_id).chain(
                self.selected_item_ids()
                    .filter(|item_id| include_selection && *item_id != address.item_id),
            );
            for item_id in item_ids {
                let item = self.item(item_id)?;
                for (candidate, track) in item.animation_tracks() {
                    if candidate == *address
                        || self
                            .property_schema(&candidate)
                            .is_none_or(|schema| !schema.is_editable(candidate.scalar_index))
                    {
                        continue;
                    }
                    let clock = item.animation_clock(track);
                    let Some(index) = track.stop_index_at(clock.pattern_frame_at(times.0)) else {
                        continue;
                    };
                    if part.with_index(index).times(item, track) != Some(times)
                        || interpolation.is_some_and(|interpolation| {
                            track.interpolations().get(index) != Some(&interpolation)
                        })
                    {
                        continue;
                    }
                    if let AnimationEditKind::Stop(range) = &mut kind {
                        let Some(other) = stop_move_range(item, track, index) else {
                            continue;
                        };
                        let minimum = range.start().frames().max(other.start().frames());
                        let maximum = range.end().frames().min(other.end().frames());
                        if minimum > maximum {
                            return None;
                        }
                        *range =
                            TimelineTime::from_frames(minimum)..=TimelineTime::from_frames(maximum);
                    }
                    targets.push(Target {
                        address: candidate,
                        index,
                    });
                }
            }
        }
        Some(AnimationEdit {
            targets,
            kind,
            gesture: self.begin_edit_gesture(),
        })
    }

    pub fn move_animation_stop(&mut self, edit: &mut AnimationEdit, time: TimelineTime) -> bool {
        let Some(range) = edit.time_range() else {
            return false;
        };
        let time = TimelineTime::from_frames(
            time.frames()
                .round()
                .clamp(range.start().frames(), range.end().frames()),
        );
        self.apply_animation_edit(edit, |item, index, track| {
            let frame = item.animation_clock(track).pattern_frame_at(time);
            let changed = track.move_stop(index, frame);
            (changed || AnimationEditTarget::Stop(index).times(item, track)?.0 == time)
                .then_some(changed)
        })
    }

    pub fn set_animation_interpolation(
        &mut self,
        edit: &mut AnimationEdit,
        interpolation: SegmentInterpolation,
    ) -> bool {
        if !matches!(edit.kind, AnimationEditKind::Segment) || !interpolation.is_valid() {
            return false;
        }
        self.apply_animation_edit(edit, |_, index, track| {
            Some(track.set_segment_interpolation(index, interpolation))
        })
    }

    fn apply_animation_edit(
        &mut self,
        edit: &mut AnimationEdit,
        update: impl Fn(&TimelineItem, usize, &mut ScalarTrack) -> Option<bool>,
    ) -> bool {
        if !edit.is_current(self) {
            return false;
        }
        let mut candidates = HashMap::new();
        let mut changed = false;
        for target in &edit.targets {
            let Some(item) = self.item(target.address.item_id) else {
                return false;
            };
            let candidate = match candidates.entry(item.id) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let Ok(candidate) = self.active_document().prepare_item_edit(item.id) else {
                        return false;
                    };
                    entry.insert(candidate)
                }
            };
            let Some(track) = candidate
                .item
                .animations_mut(target.address.effect_id)
                .and_then(|animations| animations.track_mut(&target.address.path()))
            else {
                return false;
            };
            let Some(updated) = update(item, target.index, track) else {
                return false;
            };
            changed |= updated;
        }
        if !changed {
            return false;
        }
        self.edit_gesture(&mut edit.gesture, |editor| {
            editor
                .commit_item_edits(None, candidates.into_values())
                .unwrap_or(false);
        })
    }
}
