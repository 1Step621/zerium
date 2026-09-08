//! Scalar targets, typed endpoints, and their ordered track collections.
use super::{AnimationCurve, interpolate_scalar, supports_scalar};
use crate::domain::parameter::{ParameterValue, ParameterValueType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct ScalarTrack {
    target: AnimationChannel,
    from: ParameterValue,
    to: ParameterValue,
    curve: AnimationCurve,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ParameterAnimation {
    tracks: Vec<ScalarTrack>,
}

impl ParameterAnimation {
    fn new(
        value: ParameterValue,
        ty: &ParameterValueType,
        channel: AnimationChannel,
    ) -> Option<Self> {
        let mut result = Self { tracks: Vec::new() };
        (supports_scalar(ty.scalar_at(channel.coordinate())?)
            && result.enable_channel(channel, &value)
            && result.is_valid_for(&value, ty))
        .then_some(result)
    }

    pub(crate) fn evaluate(&self, base: &ParameterValue, progress: f32) -> Option<ParameterValue> {
        if !progress.is_finite() {
            return None;
        }
        let mut result = base.clone();
        for track in &self.tracks {
            let value = interpolate_scalar(&track.from, &track.to, track.curve.evaluate(progress))?;
            *result.scalar_at_mut(track.target.coordinate())? = value;
        }
        Some(result)
    }

    pub(crate) fn numeric_range(&self, channel: AnimationChannel) -> Option<(f64, f64)> {
        let (from, to) = self.endpoints(channel)?;
        Some((from.numeric_scalar()?, to.numeric_scalar()?))
    }

    pub(crate) fn set_numeric_range(
        &mut self,
        channel: AnimationChannel,
        from: f64,
        to: f64,
    ) -> bool {
        let Some((current_from, current_to)) = self.endpoints(channel) else {
            return false;
        };
        let Some(from) = current_from.with_numeric_scalar(from) else {
            return false;
        };
        let Some(to) = current_to.with_numeric_scalar(to) else {
            return false;
        };
        self.set_scalar_endpoints(channel, from, to)
    }

    pub(crate) fn endpoints(
        &self,
        channel: AnimationChannel,
    ) -> Option<(&ParameterValue, &ParameterValue)> {
        let track = self.tracks.iter().find(|track| track.target == channel)?;
        Some((&track.from, &track.to))
    }

    pub(crate) fn set_scalar_endpoints(
        &mut self,
        channel: AnimationChannel,
        from: ParameterValue,
        to: ParameterValue,
    ) -> bool {
        let Some(track) = self.tracks.iter_mut().find(|track| track.target == channel) else {
            return false;
        };
        let animation = track;
        if interpolate_scalar(&animation.from, &from, 0.).is_none()
            || interpolate_scalar(&animation.to, &to, 0.).is_none()
            || (animation.from == from && animation.to == to)
        {
            return false;
        }
        animation.from = from;
        animation.to = to;
        true
    }

    pub(crate) fn channel_enabled(&self, channel: AnimationChannel) -> bool {
        self.tracks.iter().any(|track| track.target == channel)
    }

    fn enable_channel(&mut self, channel: AnimationChannel, value: &ParameterValue) -> bool {
        if self.channel_enabled(channel) {
            return false;
        }
        let Some(value) = value
            .scalar_at(channel.coordinate())
            .and_then(super::interpolation::interpolatable_scalar)
        else {
            return false;
        };
        self.tracks.push(ScalarTrack {
            target: channel,
            from: value.clone(),
            to: value,
            curve: AnimationCurve::default(),
        });
        true
    }

    fn disable_channel(&mut self, channel: AnimationChannel) -> bool {
        let len = self.tracks.len();
        self.tracks.retain(|track| track.target != channel);
        self.tracks.len() != len
    }

    fn has_enabled_channels(&self) -> bool {
        !self.tracks.is_empty()
    }

    pub(crate) fn curve(&self, channel: AnimationChannel) -> Option<&AnimationCurve> {
        Some(
            &self
                .tracks
                .iter()
                .find(|track| track.target == channel)?
                .curve,
        )
    }
    pub(in crate::domain) fn curve_mut(
        &mut self,
        channel: AnimationChannel,
    ) -> Option<&mut AnimationCurve> {
        Some(
            &mut self
                .tracks
                .iter_mut()
                .find(|track| track.target == channel)?
                .curve,
        )
    }

    pub(crate) fn curves(&self) -> impl Iterator<Item = (AnimationChannel, &AnimationCurve)> {
        self.tracks.iter().map(|track| (track.target, &track.curve))
    }

    fn remap_time_range(&mut self, start: f64, end: f64) {
        for track in &mut self.tracks {
            track.curve.remap_time_range(start, end);
        }
    }

    pub(crate) fn is_valid_for(&self, value: &ParameterValue, ty: &ParameterValueType) -> bool {
        let mut seen = std::collections::HashSet::new();
        self.has_enabled_channels()
            && self.tracks.iter().all(|track| {
                let channel = track.target;
                seen.insert(channel)
                    && ty.scalar_at(channel.coordinate()).is_some_and(|ty| {
                        supports_scalar(ty)
                            && track.from.matches_scalar(ty)
                            && track.to.matches_scalar(ty)
                    })
                    && value.scalar_at(channel.coordinate()).is_some()
                    && track.curve.is_valid()
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ParameterAnimationTarget {
    pub parameter_id: String,
    pub array_index: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ParameterAnimationAddress {
    pub array_index: Option<usize>,
    pub channel: AnimationChannel,
}

impl ParameterAnimationAddress {
    pub(crate) const fn tuple_element(array_index: Option<usize>, coordinate: usize) -> Self {
        Self {
            array_index,
            channel: AnimationChannel::TupleElement(coordinate),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ParameterAnimations {
    animations: HashMap<ParameterAnimationTarget, ParameterAnimation>,
    activation_order: Vec<ParameterAnimationTarget>,
}

impl ParameterAnimations {
    pub(crate) fn iter(
        &self,
    ) -> impl Iterator<Item = (&ParameterAnimationTarget, &ParameterAnimation)> {
        self.animations.iter()
    }

    pub(crate) fn ordered_iter(
        &self,
    ) -> impl Iterator<Item = (&ParameterAnimationTarget, &ParameterAnimation)> {
        self.activation_order
            .iter()
            .filter_map(|target| self.animations.get_key_value(target))
    }

    pub(in crate::domain) fn from_ordered_entries(
        entries: Vec<(ParameterAnimationTarget, ParameterAnimation)>,
    ) -> Option<Self> {
        let mut animations = HashMap::with_capacity(entries.len());
        let mut activation_order = Vec::with_capacity(entries.len());
        for (target, animation) in entries {
            if animations.insert(target.clone(), animation).is_some() {
                return None;
            }
            activation_order.push(target);
        }
        Some(Self {
            animations,
            activation_order,
        })
    }

    pub(crate) fn get(
        &self,
        parameter_id: &str,
        array_index: Option<usize>,
    ) -> Option<&ParameterAnimation> {
        self.animations.get(&ParameterAnimationTarget {
            parameter_id: parameter_id.to_owned(),
            array_index,
        })
    }

    pub(in crate::domain) fn get_mut(
        &mut self,
        parameter_id: &str,
        array_index: Option<usize>,
    ) -> Option<&mut ParameterAnimation> {
        let key = ParameterAnimationTarget {
            parameter_id: parameter_id.to_owned(),
            array_index,
        };
        self.animations.get_mut(&key)
    }

    pub(crate) fn enable(
        &mut self,
        parameter_id: &str,
        address: ParameterAnimationAddress,
        value: ParameterValue,
        ty: &ParameterValueType,
    ) -> bool {
        let key = ParameterAnimationTarget {
            parameter_id: parameter_id.to_owned(),
            array_index: address.array_index,
        };
        if let Some(animation) = self.animations.get_mut(&key) {
            if !animation.is_valid_for(&value, ty)
                || !ty
                    .scalar_at(address.channel.coordinate())
                    .is_some_and(supports_scalar)
                || !animation.enable_channel(address.channel, &value)
            {
                return false;
            }
            self.activation_order.retain(|candidate| candidate != &key);
            self.activation_order.push(key);
            return true;
        }
        let Some(animation) = ParameterAnimation::new(value, ty, address.channel) else {
            return false;
        };
        self.animations.insert(key.clone(), animation);
        self.activation_order.push(key);
        true
    }

    pub(crate) fn disable(
        &mut self,
        parameter_id: &str,
        address: ParameterAnimationAddress,
    ) -> bool {
        let key = ParameterAnimationTarget {
            parameter_id: parameter_id.to_owned(),
            array_index: address.array_index,
        };
        let Some(animation) = self.animations.get_mut(&key) else {
            return false;
        };
        if !animation.disable_channel(address.channel) {
            return false;
        }
        let remove_target = !animation.has_enabled_channels();
        if remove_target {
            self.animations.remove(&key);
            self.activation_order.retain(|candidate| candidate != &key);
        }
        true
    }

    pub(crate) fn retain_parameter_array_len(
        &mut self,
        parameter_id: &str,
        array_len: usize,
    ) -> bool {
        let previous_len = self.animations.len();
        self.animations.retain(|target, _| {
            target.parameter_id != parameter_id
                || target.array_index.is_some_and(|index| index < array_len)
        });
        self.activation_order.retain(|target| {
            target.parameter_id != parameter_id
                || target.array_index.is_some_and(|index| index < array_len)
        });
        self.animations.len() != previous_len
    }

    pub(crate) fn remap_time_range(&mut self, start: f64, end: f64) {
        for animation in self.animations.values_mut() {
            animation.remap_time_range(start, end);
        }
    }
}
