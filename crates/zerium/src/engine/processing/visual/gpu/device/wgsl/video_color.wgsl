// Source transfer functions are tabulated by GStreamer. Preserve linear HDR
// values and negative working-space components through the RGBA16Float target.
struct VideoColor {
    red: vec4<f32>,
    green: vec4<f32>,
    blue: vec4<f32>,
    luminance: vec4<f32>,
};

fn decode_channel(encoded: f32) -> f32 {
    let position = clamp(encoded, 0.0, 1.0) * f32(arrayLength(&color_lut) - 1u);
    let first = u32(position);
    let last = min(first + 1u, arrayLength(&color_lut) - 1u);
    return mix(color_lut[first], color_lut[last], fract(position));
}

fn scene_rgb(encoded: vec3<f32>, color: VideoColor) -> vec3<f32> {
    var linear = vec3(decode_channel(encoded.r), decode_channel(encoded.g), decode_channel(encoded.b));
    if color.luminance.w != 0.0 {
        let luminance = max(dot(linear, color.luminance.xyz), 0.0);
        linear *= pow(luminance, 0.2) * (1000.0 / 203.0);
    }
    return vec3(dot(color.red.xyz, linear), dot(color.green.xyz, linear), dot(color.blue.xyz, linear));
}
