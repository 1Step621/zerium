//! One animation gesture, with its matching targets fixed before any mutation.
use std::{
    collections::{HashMap, hash_map::Entry},
    ops::RangeInclusive,
};

use crate::animation::{ScalarTrack, SegmentInterpolation};
use crate::timeline::{PropertyAddress, TimelineEditor, TimelineItem, TimelineTime};

use super::EditSession;

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
