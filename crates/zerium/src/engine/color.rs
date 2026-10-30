//! Scalar sRGB transfer functions shared by CPU image processing.

pub(super) fn srgb_to_linear(value: f32) -> f32 {
    let value = value.clamp(0., 1.);
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

pub(super) fn linear_to_srgb(value: f32) -> f32 {
    let value = value.max(0.);
    if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1. / 2.4) - 0.055
    }
}
