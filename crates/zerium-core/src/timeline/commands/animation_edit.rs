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
        let track = self.animation_track(address)?;
        let times = part.times(source, track)?;
        let interpolation = match part {
            AnimationEditTarget::Stop(_) => None,
            AnimationEditTarget::Segment(index) => Some(*track.interpolations().get(index)?),
        };
        let mut targets = Vec::new();
        for item_id in self.selected_item_ids() {
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
                        || self
                            .property_schema(&candidate)
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
                        if part.with_index(index).times(item, track)? == times
                            && interpolation.is_none_or(|interpolation| {
                                track.interpolations().get(index) == Some(&interpolation)
                            })
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
                    let track = self.animation_track(&target.address)?;
                    let clock = item.animation_clock(track);
                    let frame =
                        |index: usize| clock.key_time_at(track.stops()[index].frame()).frames();
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
            let frame = item.animation_clock(track).pattern_frame_at(time);
            let changed = track.move_stop(index, frame);
            (changed || AnimationEditTarget::Stop(index).times(item, track)?.0 == time)
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
            let Some(current) = self.animation_track(&target.address) else {
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
                        .animation_track_mut(&address)
                        .expect("all gesture targets were validated before mutation") = track;
                }
                true
            });
        })
    }
}
