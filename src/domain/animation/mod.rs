//! Animation curves, scalar interpolation, tracks, and evaluation.
mod curve;
mod easing;
mod evaluation;
mod interpolation;
mod track;

pub(crate) use curve::{AnimationCurve, BezierHandle};
pub(crate) use easing::{EasingDirection, EasingFamily, SegmentInterpolation};
pub(crate) use interpolation::{interpolate_scalar, supports_scalar, supports_value, target_type};
pub(crate) use track::{
    AnimationChannel, ParameterAnimation, ParameterAnimationAddress, ParameterAnimationTarget,
    ParameterAnimations,
};
