//! Typed scalar animation tracks made of value stops and interval interpolations.
use super::{AnimationRepeat, SegmentInterpolation, interpolate_scalar};
use crate::property::{PropertyPath, PropertyValue, ScalarPropertyType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const STOP_POSITION_EPSILON: f32 = 0.000_001;
const COLOR_LINK_EPSILON: f32 = 0.000_01;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AnimationStop {
    position: f32,
    value: PropertyValue,
}

impl AnimationStop {
    pub const fn position(&self) -> f32 {
        self.position
    }

    pub const fn value(&self) -> &PropertyValue {
        &self.value
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScalarTrack {
    #[serde(default)]
    repeat: AnimationRepeat,
    stops: Vec<AnimationStop>,
    interpolations: Vec<SegmentInterpolation>,
}

impl ScalarTrack {
    pub(crate) fn from_value(
        value: PropertyValue,
        ty: &ScalarPropertyType,
        period: f32,
    ) -> Option<Self> {
        let repeat = AnimationRepeat::new(super::RepeatMode::None, period, 0.)?;
        (ty.is_interpolatable() && ty.allows(&value)).then(|| Self {
            repeat,
            stops: vec![
                AnimationStop {
                    position: 0.,
                    value: value.clone(),
                },
                AnimationStop {
                    position: 1.,
                    value,
                },
            ],
            interpolations: vec![SegmentInterpolation::default()],
        })
    }

    pub fn repeat(&self) -> AnimationRepeat {
        self.repeat
    }

    pub fn set_repeat(&mut self, repeat: AnimationRepeat) -> bool {
        if !repeat.is_valid() || self.repeat == repeat {
            return false;
        }
        self.repeat = repeat;
        true
    }

    fn values_are_linked(left: &PropertyValue, right: &PropertyValue) -> bool {
        match (left, right) {
            (PropertyValue::Color(left), PropertyValue::Color(right)) => left
                .iter()
                .zip(right)
                .all(|(left, right)| (left - right).abs() <= COLOR_LINK_EPSILON),
            _ => left == right,
        }
    }

    pub fn stops(&self) -> &[AnimationStop] {
        &self.stops
    }

    pub fn interpolations(&self) -> &[SegmentInterpolation] {
        &self.interpolations
    }

    pub fn stop_index_at(&self, position: f32) -> Option<usize> {
        if !position.is_finite() {
            return None;
        }
        self.stops
            .iter()
            .position(|stop| (stop.position - position).abs() <= STOP_POSITION_EPSILON)
    }

    pub fn stop_indices_for_segment(&self, position: f32) -> Vec<usize> {
        if let Some(index) = self.stop_index_at(position) {
            return vec![index];
        }
        if !position.is_finite() || self.stops.len() < 2 {
            return Vec::new();
        }
        let right = self.stops.partition_point(|stop| stop.position < position);
        match right {
            0 => vec![0],
            right if right >= self.stops.len() => vec![self.stops.len() - 1],
            right => vec![right - 1, right],
        }
    }

    pub fn evaluate(&self, progress: f32) -> Option<PropertyValue> {
        if !progress.is_finite() {
            return None;
        }
        let progress = progress.clamp(0., 1.);
        let right = self
            .stops
            .partition_point(|stop| stop.position < progress)
            .min(self.stops.len().saturating_sub(1));
        if self.stops.get(right)?.position == progress || right == 0 {
            return Some(self.stops.get(right)?.value.clone());
        }
        let left = right - 1;
        let from = self.stops.get(left)?;
        let to = self.stops.get(right)?;
        let span = to.position - from.position;
        if span <= 0. {
            return None;
        }
        let local = (progress - from.position) / span;
        interpolate_scalar(
            &from.value,
            &to.value,
            self.interpolations.get(left)?.evaluate(local),
        )
    }

    pub fn numeric_stops(&self) -> Option<Vec<(f32, f64)>> {
        self.stops
            .iter()
            .map(|stop| {
                stop.value
                    .numeric_scalar()
                    .map(|value| (stop.position, value))
            })
            .collect()
    }

    /// Link neighboring equal values, keeping the endpoints of an edited
    /// interval independent. A point edit without an interval links both sides.
    pub fn set_stop(&mut self, index: usize, value: PropertyValue, segment: Option<usize>) -> bool {
        let Some(current) = self.stops.get(index).map(|stop| stop.value.clone()) else {
            return false;
        };
        if interpolate_scalar(&current, &value, 0.).is_none()
            || Self::values_are_linked(&current, &value)
        {
            return false;
        }
        let mut first_linked = self.stops[..index]
            .iter()
            .rposition(|stop| !Self::values_are_linked(&stop.value, &current))
            .map_or(0, |previous| previous + 1);
        let mut last_linked = self.stops[index + 1..]
            .iter()
            .position(|stop| !Self::values_are_linked(&stop.value, &current))
            .map_or(self.stops.len() - 1, |next| index + next);
        if segment == Some(index) {
            last_linked = index;
        }
        if segment.and_then(|segment| segment.checked_add(1)) == Some(index) {
            first_linked = index;
        }
        for stop in &mut self.stops[first_linked..=last_linked] {
            stop.value = value.clone();
        }
        true
    }

    pub fn insert_stop(&mut self, position: f32, value: PropertyValue) -> Option<usize> {
        if !position.is_finite()
            || !(0. ..=1.).contains(&position)
            || interpolate_scalar(self.stops.first()?.value(), &value, 0.).is_none()
        {
            return None;
        }
        let insertion = self.stops.partition_point(|stop| stop.position < position);
        if let Some(stop) = self.stops.get(insertion)
            && (stop.position - position).abs() <= STOP_POSITION_EPSILON
        {
            return None;
        }
        if let Some(index) = insertion.checked_sub(1)
            && (self.stops[index].position - position).abs() <= STOP_POSITION_EPSILON
        {
            return None;
        }
        if insertion == 0 || insertion == self.stops.len() {
            return None;
        }
        let inherited = *self.interpolations.get(insertion - 1)?;
        self.stops
            .insert(insertion, AnimationStop { position, value });
        self.interpolations.insert(insertion, inherited);
        Some(insertion)
    }

    pub fn remove_stop(&mut self, index: usize) -> bool {
        if index == 0 || index + 1 >= self.stops.len() {
            return false;
        }
        self.stops.remove(index);
        self.interpolations.remove(index);
        true
    }

    pub fn move_stop(&mut self, index: usize, position: f32) -> bool {
        if !position.is_finite() || index == 0 || index + 1 >= self.stops.len() {
            return false;
        }
        let minimum = self.stops[index - 1].position + STOP_POSITION_EPSILON;
        let maximum = self.stops[index + 1].position - STOP_POSITION_EPSILON;
        if position < minimum || position > maximum {
            return false;
        }
        if (self.stops[index].position - position).abs() <= STOP_POSITION_EPSILON {
            return false;
        }
        self.stops[index].position = position;
        true
    }

    pub(crate) fn is_valid_for(&self, ty: &ScalarPropertyType) -> bool {
        self.repeat.is_valid()
            && self.stops.len() >= 2
            && self.interpolations.len() + 1 == self.stops.len()
            && self.stops.first().map(AnimationStop::position) == Some(0.)
            && self.stops.last().map(AnimationStop::position) == Some(1.)
            && self.stops.iter().enumerate().all(|(index, stop)| {
                stop.position.is_finite()
                    && (0. ..=1.).contains(&stop.position)
                    && (index == 0 || self.stops[index - 1].position < stop.position)
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

    fn remap_time_range(&mut self, start: f64, end: f64) {
        if !start.is_finite()
            || !end.is_finite()
            || start >= end
            || (start == 0. && end == 1.)
            || self.stops.len() < 2
            || self.interpolations.len() + 1 != self.stops.len()
        {
            return;
        }
        let start = start as f32;
        let end = end as f32;
        if !start.is_finite() || !end.is_finite() || start >= end {
            return;
        }
        let extends_left = start < 0.;
        let extends_right = end > 1.;
        let (Some(first), Some(last)) = (self.stops.first(), self.stops.last()) else {
            return;
        };
        let start_value = self.evaluate(start).unwrap_or_else(|| first.value.clone());
        let end_value = self.evaluate(end).unwrap_or_else(|| last.value.clone());
        let old_stops = self.stops.clone();
        let old_interpolations = self.interpolations.clone();
        let stretch_left = extends_left
            && old_stops.len() >= 2
            && Self::values_are_linked(&old_stops[0].value, &old_stops[1].value);
        let stretch_right = extends_right
            && old_stops.len() >= 2
            && Self::values_are_linked(
                &old_stops[old_stops.len() - 2].value,
                &old_stops[old_stops.len() - 1].value,
            );
        let mut stops = vec![AnimationStop {
            position: start,
            value: start_value,
        }];
        stops.extend(
            old_stops
                .iter()
                .filter(|stop| {
                    stop.position > start
                        && stop.position < end
                        && !(stretch_left && stop.position <= STOP_POSITION_EPSILON)
                        && !(stretch_right && stop.position >= 1. - STOP_POSITION_EPSILON)
                })
                .cloned(),
        );
        stops.push(if stretch_right {
            AnimationStop {
                position: 1.,
                value: last.value.clone(),
            }
        } else {
            AnimationStop {
                position: end,
                value: end_value,
            }
        });
        let interpolations = stops
            .windows(2)
            .map(|pair| {
                let midpoint = (pair[0].position + pair[1].position) * 0.5;
                let segment = old_stops
                    .partition_point(|stop| stop.position <= midpoint)
                    .saturating_sub(1)
                    .min(old_interpolations.len().saturating_sub(1));
                old_interpolations.get(segment).copied().unwrap_or_default()
            })
            .collect();
        let span = end - start;
        for stop in &mut stops {
            stop.position = (stop.position - start) / span;
        }
        stops[0].position = 0.;
        if let Some(last) = stops.last_mut() {
            last.position = 1.;
        }
        self.stops = stops;
        self.interpolations = interpolations;
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScalarAnimations {
    tracks: BTreeMap<PropertyPath, ScalarTrack>,
}

impl ScalarAnimations {
    pub fn track(&self, address: &PropertyPath) -> Option<&ScalarTrack> {
        self.tracks.get(address)
    }

    pub fn track_mut(&mut self, address: &PropertyPath) -> Option<&mut ScalarTrack> {
        self.tracks.get_mut(address)
    }

    pub fn insert(&mut self, address: PropertyPath, track: ScalarTrack) -> bool {
        self.tracks.insert(address, track).is_none()
    }

    pub fn remove(&mut self, address: &PropertyPath) -> bool {
        self.tracks.remove(address).is_some()
    }

    pub fn tracks(&self) -> impl Iterator<Item = (&PropertyPath, &ScalarTrack)> {
        self.tracks.iter()
    }

    /// Remove invalid tracks only for the property whose structure changed.
    pub fn retain_valid_for_property(
        &mut self,
        property_id: &str,
        value: Option<&PropertyValue>,
    ) -> bool {
        let previous = self.tracks.len();
        self.tracks.retain(|address, _| {
            address.property_id() != property_id
                || value
                    .and_then(|value| value.scalar(address.element_id(), address.scalar_index()))
                    .is_some()
        });
        previous != self.tracks.len()
    }

    pub fn trim(&mut self, offset: f64, old_span: f64, new_span: f64) {
        if !offset.is_finite()
            || !old_span.is_finite()
            || old_span <= 0.
            || !new_span.is_finite()
            || new_span <= 0.
        {
            return;
        }
        for track in self.tracks.values_mut() {
            let repeat = track.repeat;
            if repeat.mode() != super::RepeatMode::None {
                track.repeat = repeat.shifted(offset);
            } else {
                track.remap_time_range(offset / old_span, (offset + new_span) / old_span);
            }
        }
    }

    pub fn stretch(&mut self, factor: f32) {
        if !factor.is_finite() || factor <= 0. {
            return;
        }
        for track in self.tracks.values_mut() {
            track.repeat = track.repeat.scaled(factor);
        }
    }
}
