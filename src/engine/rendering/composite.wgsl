struct CompositeVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0)
var composite_input: texture_2d<f32>;

@group(0) @binding(1)
var composite_sampler: sampler;

struct CompositeInfo {
    input_size: vec2<u32>,
    output_size: vec2<u32>,
};

@group(0) @binding(2)
var<uniform> composite_info: CompositeInfo;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> CompositeVertexOutput {
    let positions = array<vec2<f32>, 3>(
        vec2(-1.0, -1.0),
        vec2( 3.0, -1.0),
        vec2(-1.0,  3.0),
    );
    let position = positions[vertex_index];
    var output: CompositeVertexOutput;
    output.position = vec4(position, 0.0, 1.0);
    output.uv = position * vec2(0.5, -0.5) + vec2(0.5);
    return output;
}

fn composite_sample(input: CompositeVertexOutput) -> vec4<f32> {
    let scale = max(composite_info.input_size.x / composite_info.output_size.x, 1u);
    if scale == 1u {
        return textureSample(composite_input, composite_sampler, input.uv);
    }

    let output_position = min(
        vec2<u32>(input.position.xy),
        composite_info.output_size - vec2(1u),
    );
    let first = output_position * scale;
    var result = vec4(0.0);
    for (var offset_y = 0u; offset_y < scale; offset_y += 1u) {
        for (var offset_x = 0u; offset_x < scale; offset_x += 1u) {
            let source_position = min(
                first + vec2(offset_x, offset_y),
                composite_info.input_size - vec2(1u),
            );
            result += textureLoad(composite_input, source_position, 0);
        }
    }
    return result / f32(scale * scale);
}

fn scene_to_srgb_channel(value: f32) -> f32 {
    if value <= 0.0031308 {
        return value * 12.92;
    }
    return 1.055 * pow(value, 1.0 / 2.4) - 0.055;
}

fn scene_to_srgb(color: vec3<f32>) -> vec3<f32> {
    return vec3(
        scene_to_srgb_channel(color.r),
        scene_to_srgb_channel(color.g),
        scene_to_srgb_channel(color.b),
    );
}

@fragment
fn fragment_main(input: CompositeVertexOutput) -> @location(0) vec4<f32> {
    return composite_sample(input);
}

// GPUI samples an embedded sRGB surface with hardware decode before composing
// it into the window. Return display-encoded values here so that decode yields
// the display code expected by GPUI's final target. The sRGB attachment applies
// its own storage encoding after this function.
@fragment
fn output_fragment_main(input: CompositeVertexOutput) -> @location(0) vec4<f32> {
    let color = composite_sample(input);
    if color.a <= 0.00001 {
        return vec4(0.0);
    }
    let straight = color.rgb / color.a;
    return vec4(scene_to_srgb(max(straight, vec3(0.0))) * color.a, color.a);
}
