//! Animation interpolation, scalar tracks, and evaluation.
mod easing;
mod evaluation;
mod interpolation;
mod track;

pub(crate) use easing::{BezierHandle, EasingDirection, EasingFamily, SegmentInterpolation};
pub(crate) use interpolation::interpolate_scalar;
pub(crate) use track::{
    AnimationChannel, ParameterAnimationAddress, ParameterAnimations, ScalarTrack,
};
