struct Conversion {
    offset: vec4<f32>, scale: vec4<f32>,
    red: vec4<f32>, green: vec4<f32>, blue: vec4<f32>, chroma: vec4<f32>,
    mode: vec4<u32>,
};
@group(0) @binding(0) var luma: texture_2d<f32>;
@group(0) @binding(1) var chroma: texture_2d<f32>;
@group(0) @binding(2) var<uniform> conversion: Conversion;

@vertex fn vertex_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(vec2(-1., -1.), vec2(3., -1.), vec2(-1., 3.));
    return vec4(positions[index], 0., 1.);
}

fn uv_sample(point: vec2<i32>) -> vec2<f32> {
    let size = vec2<i32>(textureDimensions(chroma));
    return textureLoad(chroma, clamp(point, vec2(0), size - vec2(1)), 0).rg * 255.;
}

@fragment fn fragment_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    if conversion.mode.x == 0u { return textureLoad(luma, pixel, 0); }
    let xy = (position.xy - conversion.chroma.zw) / conversion.chroma.xy;
    let base = vec2<i32>(floor(xy));
    let weight = fract(xy);
    let uv = mix(mix(uv_sample(base), uv_sample(base + vec2(1, 0)), weight.x),
                 mix(uv_sample(base + vec2(0, 1)), uv_sample(base + vec2(1, 1)), weight.x), weight.y);
    let yuv = (vec3(textureLoad(luma, pixel, 0).r * 255., uv) - conversion.offset.xyz) * conversion.scale.xyz;
    return vec4(clamp(vec3(dot(yuv, conversion.red.xyz), dot(yuv, conversion.green.xyz), dot(yuv, conversion.blue.xyz)), vec3(0.), vec3(1.)), 1.);
}
