import package::helpers::color::luminance;
import package::generated::support::util::srgb;

// Brightness uses straight linear light; coverage stays premultiplied.
fn highlight(color: vec4<f32>, threshold: f32) -> vec4<f32> {
    let brightness = luminance(color.rgb / max(color.a, 1e-5));
    let cutoff = srgb(vec3(threshold / 100.0)).x;
    return color * max(brightness - cutoff, 0.0) / max(brightness, 1e-5);
}

fn add_light(source: vec4<f32>, light: vec4<f32>, amount: f32) -> vec4<f32> {
    let coverage = clamp(light.a * amount, 0.0, 1.0);
    return vec4(source.rgb + light.rgb * amount, source.a + coverage * (1.0 - source.a));
}
