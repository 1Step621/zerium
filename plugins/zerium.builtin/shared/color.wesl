// Color math takes straight scene-linear RGB. Alpha remains linear coverage;
// encoding is used only by filters that operate on display-referred values.
fn luminance(linear: vec3<f32>) -> f32 {
    return dot(linear, vec3(0.2126, 0.7152, 0.0722));
}

fn encode_srgb(linear: vec3<f32>) -> vec3<f32> {
    let value = max(linear, vec3(0.0));
    return select(
        1.055 * pow(value, vec3(1.0 / 2.4)) - vec3(0.055),
        value * 12.92,
        value <= vec3(0.0031308),
    );
}

fn display_rgb(linear: vec3<f32>, grayscale: bool) -> vec3<f32> {
    let value = select(linear, vec3(luminance(linear)), grayscale);
    return clamp(encode_srgb(value), vec3(0.0), vec3(1.0));
}

fn quantize_rgb(encoded: vec3<f32>, steps: f32, threshold: f32) -> vec3<f32> {
    return floor(encoded * steps + vec3(threshold)) / steps;
}

fn quantize_alpha(alpha: f32, steps: f32) -> f32 {
    return floor(clamp(alpha, 0.0, 1.0) * steps + 0.5) / steps;
}
