//! Animation interpolation, scalar tracks, and evaluation.
mod animations;
mod easing;
mod interpolation;
mod timing;
mod track;

pub use animations::ScalarAnimations;
pub use easing::{BezierHandle, EasingDirection, EasingFamily, SegmentInterpolation};
pub use interpolation::interpolate_scalar;
pub use timing::{AnimationClock, AnimationRepeat, RepeatMode};
pub use track::ScalarTrack;
