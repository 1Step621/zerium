// Scene color helpers take straight scene-linear RGB. Helpers marked encoded
// operate on display RGB; alpha remains linear coverage in either space.
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

// HSL operates on straight, display-encoded RGB in [0, 1]. Hue is in turns.
fn rgb_to_hsl(encoded: vec3<f32>) -> vec3<f32> {
    let high = max(encoded.r, max(encoded.g, encoded.b));
    let low = min(encoded.r, min(encoded.g, encoded.b));
    let chroma = high - low;
    let lightness = (high + low) * 0.5;
    if chroma <= 0.0 {
        return vec3(0.0, 0.0, lightness);
    }
    var hue = (encoded.g - encoded.b) / chroma;
    if high == encoded.g {
        hue = (encoded.b - encoded.r) / chroma + 2.0;
    } else if high == encoded.b {
        hue = (encoded.r - encoded.g) / chroma + 4.0;
    }
    let saturation = chroma / max(1.0 - abs(2.0 * lightness - 1.0), 0.0000001);
    return vec3(fract(hue / 6.0), saturation, lightness);
}

fn hsl_to_rgb(hsl: vec3<f32>) -> vec3<f32> {
    let hue = fract(hsl.x) * 6.0;
    let chroma = (1.0 - abs(2.0 * hsl.z - 1.0)) * hsl.y;
    let secondary = chroma * (1.0 - abs(hue - 2.0 * floor(hue * 0.5) - 1.0));
    var color = vec3(chroma, secondary, 0.0);
    if hue >= 5.0 {
        color = vec3(chroma, 0.0, secondary);
    } else if hue >= 4.0 {
        color = vec3(secondary, 0.0, chroma);
    } else if hue >= 3.0 {
        color = vec3(0.0, secondary, chroma);
    } else if hue >= 2.0 {
        color = vec3(0.0, chroma, secondary);
    } else if hue >= 1.0 {
        color = vec3(secondary, chroma, 0.0);
    }
    return color + vec3(hsl.z - chroma * 0.5);
}
