//! One animation gesture, with its matching targets fixed before any mutation.
use super::*;
use crate::timeline::{ProjectId, PropertyAddress};
use std::ops::RangeInclusive;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationEditTarget {
    Stop(usize),
    Segment(usize),
}

struct Target {
    address: PropertyAddress,
    part: AnimationEditTarget,
}

/// Editor-local gesture state; synchronization is derived, never persisted.
pub struct AnimationEdit {
    targets: Vec<Target>,
    frame_range: Option<RangeInclusive<Frame>>,
    before: Option<HistorySnapshot>,
    revision: u64,
    scene_id: Option<SceneId>,
    project_id: ProjectId,
    group_revision: u64,
}

impl AnimationEdit {
    pub fn frame_range(&self) -> Option<&RangeInclusive<Frame>> {
        self.frame_range.as_ref()
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
        let times = Self::animation_target_frames(source, track, part)?;
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
                        let candidate_part = match part {
                            AnimationEditTarget::Stop(_) => AnimationEditTarget::Stop(index),
                            AnimationEditTarget::Segment(_) => AnimationEditTarget::Segment(index),
                        };
                        if !synchronize && candidate_part != part {
                            continue;
                        }
                        if Self::animation_target_frames(item, track, candidate_part)? == times {
                            targets.push(Target {
                                address: candidate.clone(),
                                part: candidate_part,
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
            .any(|target| target.address == *address && target.part == part)
        {
            return None;
        }
        let frame_range = match part {
            AnimationEditTarget::Stop(_) => {
                let mut minimum = Frame::new(0);
                let mut maximum = Frame::new(u64::MAX);
                for target in &targets {
                    let AnimationEditTarget::Stop(index) = target.part else {
                        unreachable!()
                    };
                    let item = self.item(target.address.item_id)?;
                    let track = self.animation_track(
                        item.id,
                        target.address.effect_id,
                        &target.address.path(),
                    )?;
                    let frame = |index: usize| {
                        TimelineTime::from_frames(
                            item.animation_timeline_frame(track.stops()[index].position()),
                        )
                        .nearest_frame()
                    };
                    minimum = minimum.max(Frame::new(frame(index - 1).get().saturating_add(1)));
                    maximum = maximum.min(Frame::new(frame(index + 1).get().saturating_sub(1)));
                }
                if minimum > maximum {
                    return None;
                }
                Some(minimum..=maximum)
            }
            AnimationEditTarget::Segment(_) => None,
        };
        Some(AnimationEdit {
            targets,
            frame_range,
            before: Some(self.history_snapshot()),
            revision: self.project_revision(),
            scene_id: self.active_scene_id(),
            project_id: self.project().id,
            group_revision: self.project_revision(),
        })
    }

    fn animation_target_frames(
        item: &TimelineItem,
        track: &ScalarTrack,
        part: AnimationEditTarget,
    ) -> Option<(Frame, Frame)> {
        let frame = |index: usize| {
            track.stops().get(index).map(|stop| {
                TimelineTime::from_frames(item.animation_timeline_frame(stop.position()))
                    .nearest_frame()
            })
        };
        match part {
            AnimationEditTarget::Stop(index) => Some((frame(index)?, frame(index)?)),
            AnimationEditTarget::Segment(index) => Some((frame(index)?, frame(index + 1)?)),
        }
    }

    pub fn move_animation_stop(&mut self, edit: &mut AnimationEdit, frame: Frame) -> Option<Frame> {
        let range = edit.frame_range.as_ref()?;
        let frame = frame.clamp(*range.start(), *range.end());
        self.apply_animation_edit(edit, |item, part, track| {
            let AnimationEditTarget::Stop(index) = part else {
                return None;
            };
            let progress = item.animation_progress_at_time(TimelineTime::from_frame(frame));
            let changed = track.move_stop(index, progress);
            (changed || Self::animation_target_frames(item, track, part)?.0 == frame)
                .then_some(changed)
        })
        .then_some(frame)
    }

    pub fn set_animation_interpolation(
        &mut self,
        edit: &mut AnimationEdit,
        interpolation: SegmentInterpolation,
    ) -> bool {
        if !interpolation.is_valid() {
            return false;
        }
        self.apply_animation_edit(edit, |_, part, track| {
            let AnimationEditTarget::Segment(index) = part else {
                return None;
            };
            Some(track.set_segment_interpolation(index, interpolation))
        })
    }

    fn apply_animation_edit(
        &mut self,
        edit: &mut AnimationEdit,
        update: impl Fn(&TimelineItem, AnimationEditTarget, &mut ScalarTrack) -> Option<bool>,
    ) -> bool {
        // Undo, scene navigation or an unrelated project edit ends this gesture.
        if edit.revision != self.project_revision()
            || edit.scene_id != self.active_scene_id()
            || edit.project_id != self.project().id
        {
            return false;
        }
        let key = HistoryKey::AnimationGesture(edit.group_revision);
        if edit.before.is_none() && !self.history.is_current_group(&(edit.scene_id, key.clone())) {
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
            let Some(updated) = update(item, target.part, track) else {
                return false;
            };
            changed |= updated;
        }
        if !changed {
            return false;
        }
        for (address, track) in updates {
            *self
                .animation_track_mut(address.item_id, address.effect_id, &address.path())
                .expect("all gesture targets were validated before mutation") = track;
        }
        self.finish_project_edit(edit.before.take(), Some(key));
        edit.revision = self.project_revision();
        true
    }
}
