//! Segment interpolation modes and their mathematical evaluation.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EasingFamily {
    Bezier,
    Sine,
    Quad,
    Bounce,
    Elastic,
}

impl EasingFamily {
    fn ease_in(self, progress: f32) -> f32 {
        match self {
            Self::Bezier => cubic_bezier_progress(progress, 0.42, 0., 1., 1.),
            Self::Sine => 1. - (progress * std::f32::consts::FRAC_PI_2).cos(),
            Self::Quad => progress * progress,
            Self::Bounce => 1. - bounce_out(1. - progress),
            Self::Elastic if progress == 0. || progress == 1. => progress,
            Self::Elastic => {
                const PERIOD: f32 = std::f32::consts::TAU / 3.;
                -2_f32.powf(10. * progress - 10.) * ((10. * progress - 10.75) * PERIOD).sin()
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EasingDirection {
    In,
    Out,
    InOut,
}

impl EasingDirection {
    fn apply(self, family: EasingFamily, progress: f32) -> f32 {
        match self {
            Self::In => family.ease_in(progress),
            Self::Out => 1. - family.ease_in(1. - progress),
            Self::InOut if progress < 0.5 => family.ease_in(progress * 2.) / 2.,
            Self::InOut => 1. - family.ease_in((1. - progress) * 2.) / 2.,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SegmentInterpolation {
    Linear,
    Hold,
    Ease {
        family: EasingFamily,
        direction: EasingDirection,
    },
    Custom {
        control_out: [f32; 2],
        control_in: [f32; 2],
    },
}

impl SegmentInterpolation {
    pub(crate) fn is_custom(self) -> bool {
        matches!(self, Self::Custom { .. })
    }

    pub(crate) fn custom_default(start: [f32; 2], end: [f32; 2]) -> Self {
        let delta = [end[0] - start[0], end[1] - start[1]];
        Self::Custom {
            control_out: [start[0] + delta[0] / 3., start[1] + delta[1] / 3.],
            control_in: [start[0] + delta[0] * 2. / 3., start[1] + delta[1] * 2. / 3.],
        }
    }
}

pub(super) fn easing(interpolation: SegmentInterpolation, progress: f32) -> f32 {
    match interpolation {
        SegmentInterpolation::Linear => progress,
        SegmentInterpolation::Hold => {
            if progress < 1. {
                0.
            } else {
                1.
            }
        }
        SegmentInterpolation::Ease { family, direction } => direction.apply(family, progress),
        SegmentInterpolation::Custom { .. } => progress,
    }
}

fn cubic_bezier_progress(progress: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let mut low = 0.;
    let mut high = 1.;
    for _ in 0..16 {
        let t = (low + high) * 0.5;
        if cubic(0., x1, x2, 1., t) < progress {
            low = t;
        } else {
            high = t;
        }
    }
    cubic(0., y1, y2, 1., (low + high) * 0.5)
}

fn bounce_out(progress: f32) -> f32 {
    const N1: f32 = 7.5625;
    const D1: f32 = 2.75;

    if progress < 1. / D1 {
        N1 * progress * progress
    } else if progress < 2. / D1 {
        let progress = progress - 1.5 / D1;
        N1 * progress * progress + 0.75
    } else if progress < 2.5 / D1 {
        let progress = progress - 2.25 / D1;
        N1 * progress * progress + 0.9375
    } else {
        let progress = progress - 2.625 / D1;
        N1 * progress * progress + 0.984_375
    }
}

pub(super) fn cubic(start: f32, control_a: f32, control_b: f32, end: f32, t: f32) -> f32 {
    let inverse = 1. - t;
    inverse.powi(3) * start
        + 3. * inverse.powi(2) * t * control_a
        + 3. * inverse * t.powi(2) * control_b
        + t.powi(3) * end
}
