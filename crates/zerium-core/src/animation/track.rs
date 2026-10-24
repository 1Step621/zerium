//! Typed scalar tracks with integer authored frames and continuous evaluation.
use super::timing::scale_frames;
use super::{AnimationRepeat, SegmentInterpolation, interpolate_scalar};
use crate::{
    property::{PropertyValue, ScalarPropertyType},
    timeline::{Frame, FrameDuration},
};
use serde::{Deserialize, Serialize};

const COLOR_VALUE_EPSILON: f32 = 0.000_01;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AnimationStop {
    frame: Frame,
    value: PropertyValue,
}

impl AnimationStop {
    pub const fn frame(&self) -> Frame {
        self.frame
    }

    pub const fn value(&self) -> &PropertyValue {
        &self.value
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScalarTrack {
    span_frames: FrameDuration,
    repeat: AnimationRepeat,
    stops: Vec<AnimationStop>,
    interpolations: Vec<SegmentInterpolation>,
}

impl ScalarTrack {
    pub(crate) fn from_value(
        value: PropertyValue,
        ty: &ScalarPropertyType,
        duration: FrameDuration,
    ) -> Option<Self> {
        (ty.is_interpolatable() && ty.allows(&value)).then(|| Self {
            span_frames: duration.animation_span(),
            repeat: AnimationRepeat::new(super::RepeatMode::None, duration, Frame::new(0)),
            stops: vec![
                AnimationStop {
                    frame: Frame::new(0),
                    value: value.clone(),
                },
                AnimationStop {
                    frame: Frame::new(duration.animation_span().get()),
                    value,
                },
            ],
            interpolations: vec![SegmentInterpolation::default()],
        })
    }

    pub fn span(&self) -> FrameDuration {
        self.span_frames
    }

    pub fn repeat(&self) -> AnimationRepeat {
        self.repeat
    }

    pub(crate) fn set_repeat(&mut self, repeat: AnimationRepeat, duration: FrameDuration) -> bool {
        let span = repeat.pattern_span(duration);
        if self.repeat == repeat && self.span_frames == span {
            return false;
        }
        self.resize_span(span);
        self.repeat = repeat;
        true
    }

    /// Rescale keys with their outgoing curves. Interior keys take priority
    /// over endpoints; collisions between interior keys keep the later key.
    fn resize_span(&mut self, span: FrameDuration) {
        if self.span_frames == span {
            return;
        }
        for stop in &mut self.stops {
            stop.frame = Frame::new(scale_frames(
                stop.frame.get(),
                span.get(),
                self.span_frames.get(),
            ));
        }
        let last = self.stops.len() - 1;
        let mut retained: Vec<_> = (0..self.stops.len()).collect();
        retained.dedup_by(|later, earlier| {
            if self.stops[*later].frame != self.stops[*earlier].frame {
                return false;
            }
            if *later != last {
                *earlier = *later;
            }
            true
        });
        self.interpolations = retained
            .windows(2)
            .map(|pair| self.interpolations[pair[0]])
            .collect();
        self.stops = retained
            .into_iter()
            .map(|index| self.stops[index].clone())
            .collect();
        self.span_frames = span;
    }

    fn values_are_equal(left: &PropertyValue, right: &PropertyValue) -> bool {
        match (left, right) {
            (PropertyValue::Color(left), PropertyValue::Color(right)) => left
                .iter()
                .zip(right)
                .all(|(left, right)| (left - right).abs() <= COLOR_VALUE_EPSILON),
            _ => left == right,
        }
    }

    pub fn stops(&self) -> &[AnimationStop] {
        &self.stops
    }

    pub fn interpolations(&self) -> &[SegmentInterpolation] {
        &self.interpolations
    }

    pub fn stop_index_at(&self, frame: Frame) -> Option<usize> {
        self.stops
            .binary_search_by_key(&frame, AnimationStop::frame)
            .ok()
    }

    /// Exact authored keys are compared in frames, without a normalized epsilon.
    pub fn stop_indices_for_segment(&self, frame: f64) -> Vec<usize> {
        if !frame.is_finite() || self.stops.len() < 2 {
            return Vec::new();
        }
        let right = self
            .stops
            .partition_point(|stop| (stop.frame.get() as f64) < frame);
        if self
            .stops
            .get(right)
            .is_some_and(|stop| stop.frame.get() as f64 == frame)
        {
            return vec![right];
        }
        match right {
            0 => vec![0],
            right if right >= self.stops.len() => vec![self.stops.len() - 1],
            right => vec![right - 1, right],
        }
    }

    /// Playback and temporal effects can sample between the integer keys.
    pub fn evaluate(&self, frame: f64) -> Option<PropertyValue> {
        if !frame.is_finite() {
            return None;
        }
        let frame = frame.clamp(0., self.span_frames.get() as f64);
        let right = self
            .stops
            .partition_point(|stop| (stop.frame.get() as f64) < frame)
            .min(self.stops.len().saturating_sub(1));
        let to = self.stops.get(right)?;
        if to.frame.get() as f64 == frame || right == 0 {
            return Some(to.value.clone());
        }
        let left = right - 1;
        let from = self.stops.get(left)?;
        let span = to.frame.get().checked_sub(from.frame.get())?;
        if span == 0 {
            return None;
        }
        let local = ((frame - from.frame.get() as f64) / span as f64) as f32;
        interpolate_scalar(
            &from.value,
            &to.value,
            self.interpolations.get(left)?.evaluate(local),
        )
    }

    pub(super) fn extend_at(&mut self, frame: Frame, new_span: FrameDuration) {
        if new_span.get() <= self.span_frames.get() {
            return;
        }
        let extra = new_span.get() - self.span_frames.get();
        let segment = self
            .stops
            .partition_point(|stop| stop.frame <= frame)
            .clamp(1, self.stops.len() - 1)
            - 1;
        for stop in &mut self.stops[segment + 1..] {
            stop.frame = Frame::new(stop.frame.get() + extra);
        }
        self.span_frames = new_span;
    }

    /// Resolve the fixed targets of a value edit before any values change.
    pub(crate) fn value_edit_targets(
        &self,
        index: usize,
        segment: Option<usize>,
    ) -> Option<std::ops::RangeInclusive<usize>> {
        let current = self.stops.get(index)?.value();
        let (first, last) = match segment {
            Some(segment) if index == segment && segment < self.interpolations.len() => {
                let first = self.stops[..index]
                    .iter()
                    .rposition(|stop| !Self::values_are_equal(&stop.value, current))
                    .map_or(0, |previous| previous + 1);
                (first, index)
            }
            Some(segment) if segment.checked_add(1) == Some(index) => {
                let last = self.stops[index + 1..]
                    .iter()
                    .position(|stop| !Self::values_are_equal(&stop.value, current))
                    .map_or(self.stops.len() - 1, |next| index + next);
                (index, last)
            }
            Some(_) => return None,
            None => (index, index),
        };
        Some(first..=last)
    }

    /// Set one authored key. Linked edits apply this to their resolved targets.
    pub fn set_stop(&mut self, index: usize, value: PropertyValue) -> bool {
        let Some(current) = self.stops.get(index).map(AnimationStop::value) else {
            return false;
        };
        if !self.accepts_value(&value) || Self::values_are_equal(current, &value) {
            return false;
        }
        self.stops[index].value = value;
        true
    }

    pub fn insert_stop(&mut self, frame: Frame, value: PropertyValue) -> Option<usize> {
        if !self.accepts_value(&value) || frame.get() == 0 || frame.get() >= self.span_frames.get()
        {
            return None;
        }
        let insertion = self
            .stops
            .binary_search_by_key(&frame, AnimationStop::frame)
            .err()?;
        let inherited = *self.interpolations.get(insertion.checked_sub(1)?)?;
        self.stops.insert(insertion, AnimationStop { frame, value });
        self.interpolations.insert(insertion, inherited);
        Some(insertion)
    }

    pub fn remove_stop(&mut self, index: usize) -> bool {
        if index == 0 || index >= self.stops.len().saturating_sub(1) {
            return false;
        }
        self.stops.remove(index);
        self.interpolations.remove(index);
        true
    }

    pub fn stop_move_range(&self, index: usize) -> Option<std::ops::RangeInclusive<Frame>> {
        let previous = self.stops.get(index.checked_sub(1)?)?.frame;
        let next = self.stops.get(index.checked_add(1)?)?.frame;
        let minimum = previous.get().checked_add(1)?;
        let maximum = next.get().checked_sub(1)?;
        (minimum <= maximum).then_some(Frame::new(minimum)..=Frame::new(maximum))
    }

    pub fn move_stop(&mut self, index: usize, frame: Frame) -> bool {
        let Some(range) = self.stop_move_range(index) else {
            return false;
        };
        if !range.contains(&frame) || self.stops[index].frame == frame {
            return false;
        }
        self.stops[index].frame = frame;
        true
    }

    fn accepts_value(&self, value: &PropertyValue) -> bool {
        let ty = match self.stops.first().map(AnimationStop::value) {
            Some(PropertyValue::F32(_)) => ScalarPropertyType::F32,
            Some(PropertyValue::I32(_)) => ScalarPropertyType::I32,
            Some(PropertyValue::U32(_)) => ScalarPropertyType::U32,
            Some(PropertyValue::Color(_)) => ScalarPropertyType::Color,
            _ => return false,
        };
        ty.allows(value)
    }

    pub(crate) fn is_valid_for(&self, ty: &ScalarPropertyType) -> bool {
        ty.is_interpolatable()
            && self.stops.len() >= 2
            && self.interpolations.len() + 1 == self.stops.len()
            && self.stops.first().map(AnimationStop::frame) == Some(Frame::new(0))
            && self.stops.last().map(AnimationStop::frame)
                == Some(Frame::new(self.span_frames.get()))
            && self.stops.iter().enumerate().all(|(index, stop)| {
                stop.frame.get() <= self.span_frames.get()
                    && (index == 0 || self.stops[index - 1].frame < stop.frame)
                    && ty.allows(&stop.value)
            })
            && self
                .interpolations
                .iter()
                .copied()
                .all(SegmentInterpolation::is_valid)
    }

    pub fn set_segment_interpolation(
        &mut self,
        segment: usize,
        interpolation: SegmentInterpolation,
    ) -> bool {
        let Some(slot) = self.interpolations.get_mut(segment) else {
            return false;
        };
        if !interpolation.is_valid() || *slot == interpolation {
            return false;
        }
        *slot = interpolation;
        true
    }

    pub(super) fn trim(&mut self, start: i128, new_span: FrameDuration) {
        let end = start + i128::from(new_span.get());
        let first = self
            .stops
            .partition_point(|stop| i128::from(stop.frame.get()) <= start)
            .saturating_sub(1);
        let last = self
            .stops
            .partition_point(|stop| i128::from(stop.frame.get()) < end)
            .min(self.stops.len() - 1);
        let mut keys: Vec<_> = self
            .stops
            .iter()
            .enumerate()
            .filter_map(|(index, stop)| {
                // Move constant endpoints; preserve transitions by adding a key instead.
                let frame = match index {
                    0 if Self::values_are_equal(&stop.value, &self.stops[1].value) => {
                        i128::from(stop.frame.get()).min(start)
                    }
                    index
                        if index == self.stops.len() - 1
                            && Self::values_are_equal(
                                &stop.value,
                                &self.stops[index - 1].value,
                            ) =>
                    {
                        i128::from(stop.frame.get()).max(end)
                    }
                    _ => i128::from(stop.frame.get()),
                };
                (frame >= start && frame <= end).then(|| (frame, stop.value.clone()))
            })
            .collect();
        if keys.first().is_none_or(|(frame, _)| *frame != start) {
            keys.insert(0, (start, self.stops[first].value.clone()));
        }
        if keys.last().is_none_or(|(frame, _)| *frame != end) {
            keys.push((end, self.stops[last].value.clone()));
        }
        let interpolations = keys
            .windows(2)
            .map(|pair| {
                let midpoint_twice = pair[0].0 + pair[1].0;
                let segment = self
                    .stops
                    .partition_point(|stop| i128::from(stop.frame.get()) * 2 <= midpoint_twice)
                    .saturating_sub(1)
                    .min(self.interpolations.len() - 1);
                self.interpolations[segment]
            })
            .collect();
        self.stops = keys
            .into_iter()
            .map(|(frame, value)| AnimationStop {
                frame: Frame::new((frame - start) as u64),
                value,
            })
            .collect();
        self.interpolations = interpolations;
        self.span_frames = new_span;
    }
}
