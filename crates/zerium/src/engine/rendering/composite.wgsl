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
    // Output pixel coordinates to input UV coordinates.
    uv_x: vec4<f32>,
    uv_y: vec4<f32>,
    blend_mode: vec4<u32>,
};

@group(0) @binding(2)
var<uniform> composite_info: CompositeInfo;

@group(1) @binding(0)
var composite_backdrop: texture_2d<f32>;

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
    let pixel = vec3(input.position.xy, 1.0);
    let uv = vec2(dot(composite_info.uv_x.xyz, pixel), dot(composite_info.uv_y.xyz, pixel));
    let dx = vec2(composite_info.uv_x.x, composite_info.uv_y.x);
    let dy = vec2(composite_info.uv_x.y, composite_info.uv_y.y);
    let aligned = all(composite_info.uv_x.xyz == vec3(1.0 / f32(composite_info.output_size.x), 0.0, 0.0))
        && all(composite_info.uv_y.xyz == vec3(0.0, 1.0 / f32(composite_info.output_size.y), 0.0));
    if !aligned {
        if any(uv < vec2(0.0)) || any(uv >= vec2(1.0)) {
            return vec4(0.0);
        }
        let size = vec2<f32>(composite_info.input_size);
        let scale = clamp(u32(round(max(length(dx * size), length(dy * size)))), 1u, 4u);
        var result = vec4(0.0);
        for (var y = 0u; y < scale; y += 1u) {
            for (var x = 0u; x < scale; x += 1u) {
                let offset = (vec2(f32(x), f32(y)) + vec2(0.5)) / f32(scale) - vec2(0.5);
                let sample_uv = uv + dx * offset.x + dy * offset.y;
                if all(sample_uv >= vec2(0.0)) && all(sample_uv < vec2(1.0)) {
                    result += textureSample(composite_input, composite_sampler, sample_uv);
                }
            }
        }
        return result / f32(scale * scale);
    }
    if all(composite_info.input_size == composite_info.output_size) {
        let position = min(
            vec2<u32>(input.position.xy),
            composite_info.input_size - vec2(1u),
        );
        return textureLoad(composite_input, vec2<i32>(position), 0);
    }

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

// Modes match timeline::BlendMode. Colors remain in scene-linear space.
fn blend_color(backdrop: vec3<f32>, source: vec3<f32>) -> vec3<f32> {
    switch composite_info.blend_mode.x {
        case 1u: { return min(backdrop, source); }
        case 2u: { return backdrop * source; }
        case 3u: { return max(backdrop, source); }
        case 4u: { return backdrop + source - backdrop * source; }
        // Addition retains HDR headroom rather than clipping at display white.
        case 5u: { return backdrop + source; }
        case 6u: {
            return select(2.0 * backdrop * source,
                1.0 - 2.0 * (1.0 - backdrop) * (1.0 - source), backdrop > vec3(0.5));
        }
        case 7u: {
            let d = select(sqrt(max(backdrop, vec3(0.0))),
                ((16.0 * backdrop - 12.0) * backdrop + 4.0) * backdrop, backdrop <= vec3(0.25));
            return select(backdrop + (2.0 * source - 1.0) * (d - backdrop),
                backdrop - (1.0 - 2.0 * source) * backdrop * (1.0 - backdrop), source <= vec3(0.5));
        }
        case 8u: {
            return select(2.0 * backdrop * source,
                1.0 - 2.0 * (1.0 - backdrop) * (1.0 - source), source > vec3(0.5));
        }
        case 9u: { return abs(backdrop - source); }
        case 10u: { return backdrop + source - 2.0 * backdrop * source; }
        default: { return source; }
    }
}

@fragment
fn blend_fragment_main(input: CompositeVertexOutput) -> @location(0) vec4<f32> {
    let source = composite_sample(input);
    let backdrop = textureLoad(composite_backdrop, vec2<i32>(input.position.xy), 0);
    if source.a <= 0.0 {
        return backdrop;
    }
    if backdrop.a <= 0.0 {
        return source;
    }
    // Source-over with a blend function, using premultiplied input/output:
    // https://www.w3.org/TR/compositing-1/#blending
    let blended = blend_color(backdrop.rgb / backdrop.a, source.rgb / source.a);
    let color = (1.0 - source.a) * backdrop.rgb + (1.0 - backdrop.a) * source.rgb
        + source.a * backdrop.a * blended;
    return vec4(color, source.a + backdrop.a * (1.0 - source.a));
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
