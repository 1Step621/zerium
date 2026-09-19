//! Segment interpolation modes and their mathematical evaluation.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BezierHandle {
    In,
    Out,
}

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

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SegmentInterpolation {
    #[default]
    Linear,
    Hold,
    Ease {
        family: EasingFamily,
        direction: EasingDirection,
    },
    Custom {
        handle_out: [f32; 2],
        handle_in: [f32; 2],
    },
}

impl SegmentInterpolation {
    pub(crate) fn evaluate(self, progress: f32) -> f32 {
        let progress = progress.clamp(0., 1.);
        if progress == 0. || progress == 1. {
            return progress;
        }
        match self {
            Self::Linear => progress,
            Self::Hold => 0.,
            Self::Ease { family, direction } => direction.apply(family, progress),
            Self::Custom {
                handle_out,
                handle_in,
            } => cubic_bezier_progress(
                progress,
                handle_out[0],
                handle_out[1],
                handle_in[0],
                handle_in[1],
            ),
        }
    }

    pub(crate) fn is_valid(self) -> bool {
        let Self::Custom {
            handle_out,
            handle_in,
        } = self
        else {
            return true;
        };
        [handle_out, handle_in].into_iter().all(|handle| {
            handle.iter().all(|value| value.is_finite())
                && (0. ..=1.).contains(&handle[0])
                && (0. ..=1.).contains(&handle[1])
        })
    }

    pub(crate) fn with_handle(self, handle: BezierHandle, position: [f32; 2]) -> Option<Self> {
        if !position.iter().all(|value| value.is_finite()) {
            return None;
        }
        let position = [position[0].clamp(0., 1.), position[1].clamp(0., 1.)];
        match (handle, self) {
            (BezierHandle::Out, Self::Custom { handle_in, .. }) => Some(Self::Custom {
                handle_out: position,
                handle_in,
            }),
            (BezierHandle::In, Self::Custom { handle_out, .. }) => Some(Self::Custom {
                handle_out,
                handle_in: position,
            }),
            _ => None,
        }
    }

    pub(crate) fn custom_default() -> Self {
        Self::Custom {
            handle_out: [1. / 3., 1. / 3.],
            handle_in: [2. / 3., 2. / 3.],
        }
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

fn cubic(start: f32, point_a: f32, point_b: f32, end: f32, t: f32) -> f32 {
    let inverse = 1. - t;
    inverse.powi(3) * start
        + 3. * inverse.powi(2) * t * point_a
        + 3. * inverse * t.powi(2) * point_b
        + t.powi(3) * end
}
