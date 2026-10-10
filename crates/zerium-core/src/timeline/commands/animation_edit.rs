//! One animation gesture, with its matching targets fixed before any mutation.
use std::{collections::HashMap, ops::RangeInclusive};

use crate::animation::{ScalarTrack, SegmentInterpolation};
use crate::timeline::{PropertyAddress, TimelineEditor, TimelineItem, TimelineTime};

use super::EditGesture;

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
}

struct Target {
    address: PropertyAddress,
    index: usize,
}

enum AnimationEditKind {
    Stop(RangeInclusive<TimelineTime>),
    Segment,
}

/// Editor-local gesture state; synchronization is derived, never persisted.
pub struct AnimationEdit {
    targets: Vec<Target>,
    kind: AnimationEditKind,
    gesture: EditGesture,
}

impl AnimationEdit {
    pub fn time_range(&self) -> Option<&RangeInclusive<TimelineTime>> {
        match &self.kind {
            AnimationEditKind::Stop(range) => Some(range),
            AnimationEditKind::Segment => None,
        }
    }
}

impl TimelineEditor {
    pub fn begin_animation_edit(
        &self,
        address: &PropertyAddress,
        part: AnimationEditTarget,
        synchronize: bool,
    ) -> Option<AnimationEdit> {
        let source = self.item(address.item_id)?;
        let track = self.animation_track(address.item_id, address.effect_id, &address.path())?;
        let times = Self::animation_target_times(source, track, part)?;
        let mut targets = Vec::new();
        for item_id in self.selection.sorted_current() {
            let item = self.item(item_id)?;
            for (effect_id, animations) in std::iter::once((None, &item.animations)).chain(
                item.effects
                    .iter()
                    .map(|effect| (Some(effect.id), &effect.animations)),
            ) {
                for (path, track) in animations.tracks() {
                    let candidate = PropertyAddress {
                        item_id,
                        effect_id,
                        property_id: path.property_id().to_owned(),
                        element_id: path.element_id(),
                        scalar_index: path.scalar_index(),
                    };
                    if (!synchronize && candidate != *address)
                        || candidate
                            .schema(self)
                            .is_none_or(|schema| !schema.is_editable(candidate.scalar_index))
                    {
                        continue;
                    }
                    let indices = match part {
                        // Fixed endpoints cannot participate in point movement.
                        AnimationEditTarget::Stop(_) => 1..track.stops().len() - 1,
                        AnimationEditTarget::Segment(_) => 0..track.interpolations().len(),
                    };
                    for index in indices {
                        if !synchronize && index != part.index() {
                            continue;
                        }
                        if Self::animation_target_times(item, track, part.with_index(index))?
                            == times
                        {
                            targets.push(Target {
                                address: candidate.clone(),
                                index,
                            });
                        }
                    }
                }
            }
        }
        // In particular, a fixed endpoint or an uneditable source must not move
        // other curves just because they happen to have the same timestamp.
        if !targets
            .iter()
            .any(|target| target.address == *address && target.index == part.index())
        {
            return None;
        }
        let kind = match part {
            AnimationEditTarget::Stop(_) => {
                let mut minimum = f64::NEG_INFINITY;
                let mut maximum = f64::INFINITY;
                for target in &targets {
                    let item = self.item(target.address.item_id)?;
                    let track = self.animation_track(
                        item.id,
                        target.address.effect_id,
                        &target.address.path(),
                    )?;
                    let clock = item.animation_clock(track);
                    let frame = |index: usize| {
                        clock
                            .time_at(track.stops()[index].position())
                            .frames()
                            .round()
                    };
                    minimum = minimum.max(frame(target.index - 1) + 1.);
                    maximum = maximum.min(frame(target.index + 1) - 1.);
                }
                if minimum > maximum {
                    return None;
                }
                AnimationEditKind::Stop(
                    TimelineTime::from_frames(minimum)..=TimelineTime::from_frames(maximum),
                )
            }
            AnimationEditTarget::Segment(_) => AnimationEditKind::Segment,
        };
        Some(AnimationEdit {
            targets,
            kind,
            gesture: self.begin_edit_gesture(),
        })
    }

    fn animation_target_times(
        item: &TimelineItem,
        track: &ScalarTrack,
        part: AnimationEditTarget,
    ) -> Option<(TimelineTime, TimelineTime)> {
        let clock = item.animation_clock(track);
        let frame = |index: usize| {
            track
                .stops()
                .get(index)
                .map(|stop| clock.time_at(stop.position()).rounded())
        };
        match part {
            AnimationEditTarget::Stop(index) => Some((frame(index)?, frame(index)?)),
            AnimationEditTarget::Segment(index) => Some((frame(index)?, frame(index + 1)?)),
        }
    }

    pub fn move_animation_stop(
        &mut self,
        edit: &mut AnimationEdit,
        time: TimelineTime,
    ) -> Option<TimelineTime> {
        let range = edit.time_range()?;
        let time = TimelineTime::from_frames(
            time.frames()
                .round()
                .clamp(range.start().frames(), range.end().frames()),
        );
        self.apply_animation_edit(edit, |item, index, track| {
            let progress = item.animation_clock(track).pattern_progress_at(time);
            let changed = track.move_stop(index, progress);
            (changed
                || Self::animation_target_times(item, track, AnimationEditTarget::Stop(index))?.0
                    == time)
                .then_some(changed)
        })
        .then_some(time)
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
        if !edit.gesture.is_current(self) {
            return false;
        }
        let mut updates = HashMap::new();
        let mut changed = false;
        for target in &edit.targets {
            let Some(item) = self.item(target.address.item_id) else {
                return false;
            };
            let Some(current) =
                self.animation_track(item.id, target.address.effect_id, &target.address.path())
            else {
                return false;
            };
            let track = updates
                .entry(target.address.clone())
                .or_insert_with(|| current.clone());
            let Some(updated) = update(item, target.index, track) else {
                return false;
            };
            changed |= updated;
        }
        if !changed {
            return false;
        }
        self.edit_gesture(&mut edit.gesture, |editor| {
            editor.edit_project_if_changed(None, |editor| {
                for (address, track) in updates {
                    *editor
                        .animation_track_mut(address.item_id, address.effect_id, &address.path())
                        .expect("all gesture targets were validated before mutation") = track;
                }
                true
            });
        })
    }
}
