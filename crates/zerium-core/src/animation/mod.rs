//! Animation interpolation, scalar tracks, and evaluation.
mod easing;
mod evaluation;
mod interpolation;
mod track;

pub use easing::{BezierHandle, EasingDirection, EasingFamily, SegmentInterpolation};
pub use interpolation::interpolate_scalar;
pub use track::{ScalarAnimations, ScalarTrack};
