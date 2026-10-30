@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<storage, read> color_lut: array<f32>;
@group(0) @binding(2) var<uniform> color: VideoColor;

@vertex fn vertex_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(vec2(-1., -1.), vec2(3., -1.), vec2(-1., 3.));
    return vec4(positions[index], 0., 1.);
}

@fragment fn fragment_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = textureLoad(source, vec2<i32>(position.xy), 0);
    return vec4(scene_rgb(pixel.rgb, color), pixel.a);
}
