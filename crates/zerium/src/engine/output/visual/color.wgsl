@group(0) @binding(0) var image: texture_2d<f32>;

@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(p[index], 0.0, 1.0);
}

// Sampling OUTPUT_FORMAT removes the extra display encoding used by GPUI.
fn yuv(rgb: vec3<f32>) -> vec3<f32> {
    let y = dot(rgb, vec3(0.2126, 0.7152, 0.0722));
    return vec3(16.0 / 255.0 + 219.0 / 255.0 * y,
        128.0 / 255.0 + 224.0 / 255.0 * 0.5 * (rgb.b - y) / (1.0 - 0.0722),
        128.0 / 255.0 + 224.0 / 255.0 * 0.5 * (rgb.r - y) / (1.0 - 0.2126));
}

@fragment fn luma(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(clamp(yuv(textureLoad(image, vec2<i32>(p.xy), 0).rgb).x, 0.0, 1.0), 0.0, 0.0, 1.0);
}

@fragment fn chroma(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let base = vec2<i32>(p.xy) * 2;
    let rgb = (textureLoad(image, base, 0).rgb + textureLoad(image, base + vec2(1, 0), 0).rgb
        + textureLoad(image, base + vec2(0, 1), 0).rgb + textureLoad(image, base + vec2(1, 1), 0).rgb) * 0.25;
    return vec4(clamp(yuv(rgb).yz, vec2(0.0), vec2(1.0)), 0.0, 1.0);
}
