//! Typed scalar animation tracks made of value stops and interval interpolations.
use super::{BezierHandle, SegmentInterpolation, interpolate_scalar};
use crate::domain::parameter::{
    ArrayElementId, ParameterType, ParameterValue, ParameterValues, ScalarParameterType,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const STOP_POSITION_EPSILON: f32 = 0.000_001;
const COLOR_LINK_EPSILON: f32 = 0.000_01;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnimationStop {
    position: f32,
    value: ParameterValue,
}

impl AnimationStop {
    pub(crate) const fn position(&self) -> f32 {
        self.position
    }

    pub(crate) const fn value(&self) -> &ParameterValue {
        &self.value
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScalarTrack {
    stops: Vec<AnimationStop>,
    interpolations: Vec<SegmentInterpolation>,
}

impl ScalarTrack {
    fn new(value: ParameterValue, ty: &ScalarParameterType) -> Option<Self> {
        (ty.is_interpolatable() && ty.allows(&value)).then(|| Self {
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

    fn values_are_linked(left: &ParameterValue, right: &ParameterValue) -> bool {
        match (left, right) {
            (ParameterValue::Color(left), ParameterValue::Color(right)) => left
                .iter()
                .zip(right)
                .all(|(left, right)| (left - right).abs() <= COLOR_LINK_EPSILON),
            _ => left == right,
        }
    }

    pub(crate) fn stops(&self) -> &[AnimationStop] {
        &self.stops
    }

    pub(crate) fn interpolations(&self) -> &[SegmentInterpolation] {
        &self.interpolations
    }

    pub(crate) fn stop_index_at(&self, position: f32) -> Option<usize> {
        if !position.is_finite() {
            return None;
        }
        self.stops
            .iter()
            .position(|stop| (stop.position - position).abs() <= STOP_POSITION_EPSILON)
    }

    pub(crate) fn stop_index_nearest(&self, position: f32) -> Option<usize> {
        if !position.is_finite() {
            return None;
        }
        let right = self.stops.partition_point(|stop| stop.position < position);
        match right {
            0 => (!self.stops.is_empty()).then_some(0),
            right if right >= self.stops.len() => self.stops.len().checked_sub(1),
            right => {
                let left = right - 1;
                let previous = &self.stops[left];
                let next = &self.stops[right];
                if position - previous.position <= next.position - position {
                    Some(left)
                } else {
                    Some(right)
                }
            }
        }
    }

    pub(crate) fn stop_indices_for_segment(&self, position: f32) -> Vec<usize> {
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

    pub(crate) fn evaluate(&self, progress: f32) -> Option<ParameterValue> {
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

    pub(crate) fn numeric_stops(&self) -> Option<Vec<(f32, f64)>> {
        self.stops
            .iter()
            .map(|stop| {
                stop.value
                    .numeric_scalar()
                    .map(|value| (stop.position, value))
            })
            .collect()
    }

    pub(crate) fn set_stop(
        &mut self,
        index: usize,
        value: ParameterValue,
        focused_segment: Option<usize>,
    ) -> bool {
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
        if focused_segment == Some(index) {
            last_linked = index;
        }
        if focused_segment.and_then(|segment| segment.checked_add(1)) == Some(index) {
            first_linked = index;
        }
        for stop in &mut self.stops[first_linked..=last_linked] {
            stop.value = value.clone();
        }
        true
    }

    pub(crate) fn insert_stop(&mut self, position: f32, value: ParameterValue) -> Option<usize> {
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

    pub(crate) fn remove_stop(&mut self, index: usize) -> bool {
        if index == 0 || index + 1 >= self.stops.len() {
            return false;
        }
        self.stops.remove(index);
        self.interpolations.remove(index);
        true
    }

    pub(crate) fn move_stop(&mut self, index: usize, position: f32) -> bool {
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

    pub(in crate::domain) fn is_valid_for(&self, ty: &ScalarParameterType) -> bool {
        self.stops.len() >= 2
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

    pub(crate) fn set_segment_interpolation(
        &mut self,
        segment: usize,
        interpolation: SegmentInterpolation,
    ) -> bool {
        self.interpolations
            .get_mut(segment)
            .is_some_and(|slot| slot.set_interpolation(interpolation))
    }

    pub(crate) fn set_segment_handle(
        &mut self,
        segment: usize,
        handle: BezierHandle,
        position: [f32; 2],
    ) -> bool {
        self.interpolations
            .get_mut(segment)
            .is_some_and(|interpolation| interpolation.set_handle(handle, position))
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
        let (Some(first), Some(last)) = (self.stops.first(), self.stops.last()) else {
            return;
        };
        let start_value = self.evaluate(start).unwrap_or_else(|| first.value.clone());
        let end_value = self.evaluate(end).unwrap_or_else(|| last.value.clone());
        let old_stops = self.stops.clone();
        let old_interpolations = self.interpolations.clone();
        let mut stops = vec![AnimationStop {
            position: start,
            value: start_value,
        }];
        stops.extend(
            old_stops
                .iter()
                .filter(|stop| stop.position > start && stop.position < end)
                .cloned(),
        );
        stops.push(AnimationStop {
            position: end,
            value: end_value,
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AnimationChannel {
    Scalar,
    TupleElement(usize),
}

impl AnimationChannel {
    pub(crate) const fn coordinate(self) -> Option<usize> {
        match self {
            Self::Scalar => None,
            Self::TupleElement(index) => Some(index),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(deny_unknown_fields)]
pub(crate) struct ParameterAnimationAddress {
    pub parameter_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub array_element_id: Option<ArrayElementId>,
    pub channel: AnimationChannel,
}

impl ParameterAnimationAddress {
    pub(crate) fn new(
        parameter_id: impl Into<String>,
        array_element_id: Option<ArrayElementId>,
        channel: AnimationChannel,
    ) -> Self {
        Self {
            parameter_id: parameter_id.into(),
            array_element_id,
            channel,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ParameterAnimations {
    tracks: BTreeMap<ParameterAnimationAddress, ScalarTrack>,
}

impl ParameterAnimations {
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&ParameterAnimationAddress, &ScalarTrack)> {
        self.tracks.iter()
    }

    pub(in crate::domain) fn from_entries(
        entries: Vec<(ParameterAnimationAddress, ScalarTrack)>,
    ) -> Option<Self> {
        let mut tracks = BTreeMap::new();
        for (address, track) in entries {
            if tracks.insert(address, track).is_some() {
                return None;
            }
        }
        Some(Self { tracks })
    }

    pub(crate) fn get(&self, address: &ParameterAnimationAddress) -> Option<&ScalarTrack> {
        self.tracks.get(address)
    }

    pub(in crate::domain) fn get_mut(
        &mut self,
        address: &ParameterAnimationAddress,
    ) -> Option<&mut ScalarTrack> {
        self.tracks.get_mut(address)
    }

    pub(crate) fn contains(&self, address: &ParameterAnimationAddress) -> bool {
        self.tracks.contains_key(address)
    }

    pub(crate) fn enable(
        &mut self,
        address: ParameterAnimationAddress,
        values: &ParameterValues,
        ty: &ParameterType,
    ) -> bool {
        if self.tracks.contains_key(&address) {
            return false;
        }
        let Some((value, scalar_ty)) = values.scalar_at(&address, ty) else {
            return false;
        };
        let Some(track) = ScalarTrack::new(value.clone(), scalar_ty) else {
            return false;
        };
        self.tracks.insert(address, track);
        true
    }

    pub(crate) fn disable(&mut self, address: &ParameterAnimationAddress) -> bool {
        self.tracks.remove(address).is_some()
    }

    pub(crate) fn retain_valid_addresses(&mut self, values: &ParameterValues) -> bool {
        let previous_len = self.tracks.len();
        self.tracks
            .retain(|address, _| values.get_scalar_at(address).is_some());
        self.tracks.len() != previous_len
    }

    pub(crate) fn remap_time_range(&mut self, start: f64, end: f64) {
        for track in self.tracks.values_mut() {
            track.remap_time_range(start, end);
        }
    }
}
