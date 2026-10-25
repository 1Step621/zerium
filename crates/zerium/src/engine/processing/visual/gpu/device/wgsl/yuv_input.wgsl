// Import display-encoded YUV to the existing RGBA8 sRGB input texture.
// Write through its non-sRGB attachment view so encoding is applied once,
// when the effect/media pipeline samples the sRGB view.
struct Conversion {
    offset: vec4<f32>,
    scale: vec4<f32>,
    red: vec4<f32>,
    green: vec4<f32>,
    blue: vec4<f32>,
    chroma: vec4<f32>,
};
@group(0) @binding(0) var y_plane: texture_2d<u32>;
@group(0) @binding(1) var u_plane: texture_2d<u32>;
@group(0) @binding(2) var v_plane: texture_2d<u32>;
@group(0) @binding(3) var<uniform> conversion: Conversion;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(positions[index], 0.0, 1.0);
}

fn load_plane(plane: texture_2d<u32>, position: vec2<i32>) -> f32 {
    let limit = vec2<i32>(textureDimensions(plane)) - vec2(1);
    return f32(textureLoad(plane, clamp(position, vec2(0), limit), 0).r);
}

// Integer textures preserve 10/12/16-bit samples without requiring optional
// filterable R16Unorm support. Chroma interpolation is done explicitly.
fn sample_chroma(plane: texture_2d<u32>, position: vec2<f32>) -> f32 {
    let base = vec2<i32>(floor(position));
    let weight = fract(position);
    let top = mix(load_plane(plane, base), load_plane(plane, base + vec2(1, 0)), weight.x);
    let bottom = mix(load_plane(plane, base + vec2(0, 1)), load_plane(plane, base + vec2(1, 1)), weight.x);
    return mix(top, bottom, weight.y);
}

@fragment
fn fragment_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let chroma_position = (position.xy - conversion.chroma.zw) / conversion.chroma.xy;
    let samples = vec3(
        load_plane(y_plane, vec2<i32>(position.xy)),
        sample_chroma(u_plane, chroma_position),
        sample_chroma(v_plane, chroma_position),
    );
    let yuv = (samples - conversion.offset.xyz) * conversion.scale.xyz;
    let rgb = vec3(dot(conversion.red.xyz, yuv), dot(conversion.green.xyz, yuv), dot(conversion.blue.xyz, yuv));
    return vec4(clamp(rgb, vec3(0.0), vec3(1.0)), 1.0);
}
