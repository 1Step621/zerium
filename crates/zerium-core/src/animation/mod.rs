//! Animation interpolation, scalar tracks, and evaluation.
mod easing;
mod evaluation;
mod interpolation;
mod timing;
mod track;

pub use easing::{BezierHandle, EasingDirection, EasingFamily, SegmentInterpolation};
pub use interpolation::interpolate_scalar;
pub use timing::{AnimationClock, AnimationRepeat, RepeatMode};
pub use track::{ScalarAnimations, ScalarTrack};
