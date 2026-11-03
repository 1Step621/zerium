use crate::{
    animation::{
        AnimationRepeat, AnimationTiming, ScalarAnimations, ScalarTrack, SegmentInterpolation,
    },
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
    pub fn can_animate_property(&self, target: &PropertyAddress) -> bool {
        self.is_item_selected(target.item_id)
            && self.property_schema(target).is_some_and(|schema| {
                schema.is_animatable(target.scalar_index)
                    && schema
                        .scalar_type(target.element_id, target.scalar_index)
                        .is_some()
            })
            && !self
                .active_scene()
                .bindings_for(target)
                .any(|(_, binding)| {
                    binding.conflicts_with_animation(
                        &target.property_id,
                        target.element_id,
                        target.scalar_index,
                    )
                })
    }

    pub fn set_property_animation_enabled(
        &mut self,
        target: &PropertyAddress,
        enabled: bool,
    ) -> bool {
        if !self.is_item_selected(target.item_id) || (enabled && !self.can_animate_property(target))
        {
            return false;
        }
        let item_id = target.item_id;
        let effect_id = target.effect_id;
        let element_id = target.element_id;
        let scalar_index = target.scalar_index;
        self.edit_project_if_changed(EditImpact::Values, None, |editor| {
            let path = target.path();
            if !enabled {
                editor
                    .animation_store_mut(item_id, effect_id)
                    .is_some_and(|animations| animations.remove(&path))
            } else {
                let Some(schema) = editor.property_schema(target) else {
                    return false;
                };
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

    /// Replace an animation's timing while keeping the current displayed value at every key.
    pub fn paste_property_animation_timing(
        &mut self,
        target: &PropertyAddress,
        timing: &AnimationTiming,
    ) -> bool {
        if !self.can_animate_property(target) {
            return false;
        }
        self.edit_project_if_changed(EditImpact::Values, None, |editor| {
            let Some(item) = editor.item(target.item_id) else {
                return false;
            };
            let Some(schema) = editor.property_schema(target) else {
                return false;
            };
            let value = editor
                .evaluated_property_value(target, TimelineTime::from_frame(editor.playhead()));
            let Some((value, ty)) =
                value.zip(schema.scalar_type(target.element_id, target.scalar_index))
            else {
                return false;
            };
            let Some(track) =
                ScalarTrack::from_timing(timing, value, ty, item.duration(editor.frame_rate()))
            else {
                return false;
            };
            let Some(animations) = editor.animation_store_mut(target.item_id, target.effect_id)
            else {
                return false;
            };
            let path = target.path();
            match animations.track_mut(&path) {
                Some(current) if *current == track => false,
                Some(current) => {
                    *current = track;
                    true
                }
                None => animations.insert(path, track),
            }
        })
    }

    pub fn animation_stop_insertion_frame(
        &self,
        address: &PropertyAddress,
        time: TimelineTime,
    ) -> Option<Frame> {
        if !self.is_item_selected(address.item_id) {
            return None;
        }
        let item = self.item(address.item_id)?;
        let track = self.animation_track(address)?;
        let clock = item.animation_clock(track);
        if time < clock.key_time_at(Frame::new(0))
            || time > clock.key_time_at(Frame::new(track.span().get()))
        {
            return None;
        }
        let frame = clock.pattern_frame_at(time);
        if frame.get() == 0
            || frame.get() >= track.span().get()
            || track.stop_index_at(frame).is_some()
        {
            return None;
        }
        Some(frame)
    }

    /// Insert within the authored pattern using its evaluated value at this time.
    pub fn insert_animation_stop_at_time(
        &mut self,
        address: &PropertyAddress,
        time: TimelineTime,
    ) -> Option<usize> {
        let frame = self.animation_stop_insertion_frame(address, time)?;
        let value = self.evaluated_property_value(address, time)?;
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
        if !self.is_item_selected(address.item_id) {
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
        self.remove_animation_stops(&[(address.clone(), stop)])
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
    time: TimelineTime,
}

enum AnimationEditKind {
    Stop {
        origin: TimelineTime,
        range: RangeInclusive<TimelineTime>,
    },
    Segment,
}

/// Explicit edit scope and Undo session.
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

    pub fn time_range(&self) -> Option<&RangeInclusive<TimelineTime>> {
        match &self.kind {
            AnimationEditKind::Stop { range, .. } => Some(range),
            AnimationEditKind::Segment => None,
        }
    }
}

impl TimelineEditor {
    pub fn begin_animation_edit(
        &self,
        address: &PropertyAddress,
        part: AnimationEditTarget,
    ) -> Option<AnimationEdit> {
        if let AnimationEditTarget::Stop(index) = part {
            return self.begin_animation_stop_edit(&[(address.clone(), index)]);
        }
        let source = self.item(address.item_id)?;
        let track = self.animation_track(address)?;
        let time = part.times(source, track)?.0;
        Some(AnimationEdit {
            targets: vec![Target {
                address: address.clone(),
                index: part.index(),
                time,
            }],
            kind: AnimationEditKind::Segment,
            gesture: self.begin_edit_gesture(),
        })
    }

    /// Move only the explicitly selected interior keys, preserving their time offsets.
    pub fn begin_animation_stop_edit(
        &self,
        selected: &[(PropertyAddress, usize)],
    ) -> Option<AnimationEdit> {
        let (anchor, index) = selected.first()?;
        let origin = AnimationEditTarget::Stop(*index)
            .times(self.item(anchor.item_id)?, self.animation_track(anchor)?)?
            .0;
        let mut minimum = f64::NEG_INFINITY;
        let mut maximum = f64::INFINITY;
        let mut targets = Vec::new();
        for (address, index) in selected {
            if targets
                .iter()
                .any(|target: &Target| target.address == *address && target.index == *index)
            {
                continue;
            }
            let item = self.item(address.item_id)?;
            let track = self.animation_track(address)?;
            let stops = track.stops();
            if *index == 0 || index.checked_add(1)? >= stops.len() {
                return None;
            }
            let clock = item.animation_clock(track);
            let time = clock.key_time_at(stops[*index].frame());
            let unselected = |i| !selected.iter().any(|(a, j)| a == address && *j == i);
            let left = (0..*index).rev().find(|i| unselected(*i))?;
            let right = (*index + 1..stops.len()).find(|i| unselected(*i))?;
            let lower = clock.key_time_at(Frame::new(stops[left].frame().get() + 1));
            let upper = clock.key_time_at(Frame::new(stops[right].frame().get() - 1));
            minimum = minimum.max(lower.frames() - time.frames());
            maximum = maximum.min(upper.frames() - time.frames());
            targets.push(Target {
                address: address.clone(),
                index: *index,
                time,
            });
        }
        if minimum > maximum {
            return None;
        }
        Some(AnimationEdit {
            targets,
            kind: AnimationEditKind::Stop {
                origin,
                range: TimelineTime::from_frames(origin.frames() + minimum)
                    ..=TimelineTime::from_frames(origin.frames() + maximum),
            },
            gesture: self.begin_edit_gesture(),
        })
    }

    pub fn move_animation_stop(&mut self, edit: &mut AnimationEdit, time: TimelineTime) -> bool {
        let AnimationEditKind::Stop { origin, range } = &edit.kind else {
            return false;
        };
        let time = time
            .frames()
            .round()
            .clamp(range.start().frames(), range.end().frames());
        let delta = time - origin.frames();
        // Move the leading key first so selected neighbors cannot block each other.
        let Some(first) = edit.targets.first() else {
            return false;
        };
        let Some(item) = self.item(first.address.item_id) else {
            return false;
        };
        let Some(track) = self.animation_track(&first.address) else {
            return false;
        };
        let Some(stop) = track.stops().get(first.index) else {
            return false;
        };
        let forward = first.time.frames() + delta
            >= item
                .animation_clock(track)
                .key_time_at(stop.frame())
                .frames();
        edit.targets.sort_by(|a, b| {
            if forward {
                b.index.cmp(&a.index)
            } else {
                a.index.cmp(&b.index)
            }
        });
        self.apply_animation_edit(edit, |item, target, track| {
            let time = TimelineTime::from_frames(target.time.frames() + delta);
            let frame = item.animation_clock(track).pattern_frame_at(time);
            let changed = track.move_stop(target.index, frame);
            (changed || track.stops().get(target.index)?.frame() == frame).then_some(changed)
        })
    }

    pub fn remove_animation_stops(&mut self, selected: &[(PropertyAddress, usize)]) -> bool {
        let mut targets = selected.to_vec();
        targets.sort_by_key(|target| std::cmp::Reverse(target.1));
        let mut seen = std::collections::HashSet::new();
        targets.retain(|target| seen.insert(target.clone()));
        let mut candidates = HashMap::new();
        let mut changed = false;
        for (address, index) in targets {
            if !self.is_item_selected(address.item_id) {
                return false;
            }
            let candidate = match candidates.entry(address.item_id) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let Ok(candidate) = self.active_document().prepare_item_edit(address.item_id)
                    else {
                        return false;
                    };
                    entry.insert(candidate)
                }
            };
            let Some(track) = candidate
                .item
                .animations_mut(address.effect_id)
                .and_then(|animations| animations.track_mut(&address.path()))
            else {
                return false;
            };
            changed |= track.remove_stop(index);
        }
        changed
            && self
                .commit_item_edits(None, candidates.into_values())
                .unwrap_or(false)
    }

    pub fn set_animation_interpolation(
        &mut self,
        edit: &mut AnimationEdit,
        interpolation: SegmentInterpolation,
    ) -> bool {
        let AnimationEditKind::Segment = edit.kind else {
            return false;
        };
        if !interpolation.is_valid() {
            return false;
        }
        self.apply_animation_edit(edit, |_, target, track| {
            Some(track.set_segment_interpolation(target.index, interpolation))
        })
    }

    fn apply_animation_edit(
        &mut self,
        edit: &mut AnimationEdit,
        update: impl Fn(&TimelineItem, &Target, &mut ScalarTrack) -> Option<bool>,
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
            let Some(updated) = update(item, target, track) else {
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
