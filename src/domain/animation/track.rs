//! Typed scalar animation tracks made of value stops and interval interpolations.
use super::{BezierHandle, SegmentInterpolation, interpolate_scalar};
use crate::domain::property::{
    PropertyElementId, PropertyValue, PropertyValues, ScalarPropertyType,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const STOP_POSITION_EPSILON: f32 = 0.000_001;
const COLOR_LINK_EPSILON: f32 = 0.000_01;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnimationStop {
    position: f32,
    value: PropertyValue,
}

impl AnimationStop {
    pub(crate) const fn position(&self) -> f32 {
        self.position
    }

    pub(crate) const fn value(&self) -> &PropertyValue {
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
    pub(in crate::domain) fn from_value(
        value: PropertyValue,
        ty: &ScalarPropertyType,
    ) -> Option<Self> {
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

    fn values_are_linked(left: &PropertyValue, right: &PropertyValue) -> bool {
        match (left, right) {
            (PropertyValue::Color(left), PropertyValue::Color(right)) => left
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

    pub(crate) fn evaluate(&self, progress: f32) -> Option<PropertyValue> {
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
        value: PropertyValue,
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

    pub(crate) fn insert_stop(&mut self, position: f32, value: PropertyValue) -> Option<usize> {
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

    pub(in crate::domain) fn is_valid_for(&self, ty: &ScalarPropertyType) -> bool {
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

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PropertyAnimations {
    properties: BTreeMap<String, PropertyAnimation>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PropertyAnimation {
    value: ElementAnimations,
    elements: BTreeMap<PropertyElementId, ElementAnimations>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ElementAnimations {
    whole: Option<ScalarTrack>,
    scalars: BTreeMap<usize, ScalarTrack>,
}

impl ElementAnimations {
    pub(crate) fn scalar(&self, scalar_index: Option<usize>) -> Option<&ScalarTrack> {
        match scalar_index {
            Some(index) => self.scalars.get(&index),
            None => self.whole.as_ref(),
        }
    }

    pub(crate) fn scalar_mut(&mut self, scalar_index: Option<usize>) -> Option<&mut ScalarTrack> {
        match scalar_index {
            Some(index) => self.scalars.get_mut(&index),
            None => self.whole.as_mut(),
        }
    }

    pub(crate) fn insert(&mut self, scalar_index: Option<usize>, track: ScalarTrack) -> bool {
        match scalar_index {
            Some(index) => self.scalars.insert(index, track).is_none(),
            None => self.whole.replace(track).is_none(),
        }
    }

    pub(crate) fn remove(&mut self, scalar_index: Option<usize>) -> bool {
        match scalar_index {
            Some(index) => self.scalars.remove(&index).is_some(),
            None => self.whole.take().is_some(),
        }
    }

    pub(crate) fn tracks(&self) -> impl Iterator<Item = (Option<usize>, &ScalarTrack)> {
        self.whole.iter().map(|track| (None, track)).chain(
            self.scalars
                .iter()
                .map(|(index, track)| (Some(*index), track)),
        )
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.whole.is_none() && self.scalars.is_empty()
    }

    fn remap_time_range(&mut self, start: f64, end: f64) {
        if let Some(track) = self.whole.as_mut() {
            track.remap_time_range(start, end);
        }
        for track in self.scalars.values_mut() {
            track.remap_time_range(start, end);
        }
    }

    fn retain_valid(&mut self, value: &PropertyValue) -> bool {
        let previous = self.whole.is_some() as usize + self.scalars.len();
        if self.whole.is_some() && value.scalar_at(None).is_none() {
            self.whole = None;
        }
        self.scalars
            .retain(|scalar_index, _| value.scalar_at(Some(*scalar_index)).is_some());
        previous != self.whole.is_some() as usize + self.scalars.len()
    }
}

impl PropertyAnimation {
    pub(crate) fn element(
        &self,
        element_id: Option<PropertyElementId>,
    ) -> Option<&ElementAnimations> {
        match element_id {
            Some(element_id) => self.elements.get(&element_id),
            None => Some(&self.value),
        }
    }

    pub(crate) fn element_mut(
        &mut self,
        element_id: Option<PropertyElementId>,
    ) -> Option<&mut ElementAnimations> {
        match element_id {
            Some(element_id) => self.elements.get_mut(&element_id),
            None => Some(&mut self.value),
        }
    }

    pub(crate) fn element_or_insert(
        &mut self,
        element_id: Option<PropertyElementId>,
    ) -> &mut ElementAnimations {
        match element_id {
            Some(element_id) => self.elements.entry(element_id).or_default(),
            None => &mut self.value,
        }
    }

    pub(crate) fn elements(&self) -> impl Iterator<Item = (PropertyElementId, &ElementAnimations)> {
        self.elements
            .iter()
            .map(|(id, animations)| (*id, animations))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.value.is_empty() && self.elements.values().all(ElementAnimations::is_empty)
    }

    fn remap_time_range(&mut self, start: f64, end: f64) {
        self.value.remap_time_range(start, end);
        for element in self.elements.values_mut() {
            element.remap_time_range(start, end);
        }
    }

    fn retain_valid(&mut self, value: &PropertyValue) -> bool {
        let mut changed = self.value.retain_valid(value);
        let PropertyValue::Array(elements) = value else {
            changed |= !self.elements.is_empty();
            self.elements.clear();
            return changed;
        };
        let previous = self.elements.len();
        self.elements.retain(|element_id, animations| {
            let Some(element) = elements
                .iter()
                .find(|element| element.element_id() == *element_id)
            else {
                return false;
            };
            changed |= animations.retain_valid(element.value());
            !animations.is_empty()
        });
        changed || previous != self.elements.len()
    }
}

impl PropertyAnimations {
    pub(crate) fn property(&self, property_id: &str) -> Option<&PropertyAnimation> {
        self.properties.get(property_id)
    }

    pub(crate) fn property_mut(&mut self, property_id: &str) -> Option<&mut PropertyAnimation> {
        self.properties.get_mut(property_id)
    }

    pub(crate) fn property_or_insert(&mut self, property_id: &str) -> &mut PropertyAnimation {
        self.properties.entry(property_id.to_owned()).or_default()
    }

    pub(crate) fn properties(&self) -> impl Iterator<Item = (&str, &PropertyAnimation)> {
        self.properties
            .iter()
            .map(|(property_id, animation)| (property_id.as_str(), animation))
    }

    pub(crate) fn remove_property_if_empty(&mut self, property_id: &str) -> bool {
        self.properties
            .get(property_id)
            .is_some_and(PropertyAnimation::is_empty)
            && self.properties.remove(property_id).is_some()
    }

    pub(crate) fn retain_valid(&mut self, values: &PropertyValues) -> bool {
        let mut changed = false;
        self.properties.retain(|property_id, animations| {
            let Some(value) = values.property(property_id) else {
                changed = true;
                return false;
            };
            changed |= animations.retain_valid(value);
            !animations.is_empty()
        });
        changed
    }

    pub(crate) fn remap_time_range(&mut self, start: f64, end: f64) {
        for property in self.properties.values_mut() {
            property.remap_time_range(start, end);
        }
    }
}
