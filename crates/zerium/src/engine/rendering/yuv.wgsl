// Converts the export surface to YUV420P planes on the GPU so the CPU
// readback transfers 1.5 bytes per pixel instead of 4.
//
// The export surface is Rgba8UnormSrgb holding a double sRGB encoding for the
// GPUI preview path. Sampling it performs the hardware sRGB decode, yielding
// conventional display-referred sRGB values. That matches the values the CPU
// readback used to reconstruct with a lookup table before feeding swscale.
struct YuvVertexOutput {
    @builtin(position) position: vec4<f32>,
};

@group(0) @binding(0)
var src_texture: texture_2d<f32>;
@group(0) @binding(1)
var src_sampler: sampler;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> YuvVertexOutput {
    let positions = array<vec2<f32>, 3>(
        vec2(-1.0, -1.0),
        vec2(3.0, -1.0),
        vec2(-1.0, 3.0),
    );
    var output: YuvVertexOutput;
    output.position = vec4(positions[vertex_index], 0.0, 1.0);
    return output;
}

// ITU-R BT.709 luma coefficients, matching sws_getCoefficients(SWS_CS_ITU709).
const KR: f32 = 0.2126;
const KG: f32 = 0.7152;
const KB: f32 = 0.0722;

fn luma(rgb: vec3<f32>) -> f32 {
    return dot(rgb, vec3(KR, KG, KB));
}

// Limited (MPEG) range mapping, matching the export VideoColorSpec.
fn to_y(rgb: vec3<f32>) -> f32 {
    return 16.0 / 255.0 + 219.0 / 255.0 * luma(rgb);
}

fn to_u(rgb: vec3<f32>) -> f32 {
    return 128.0 / 255.0 + 224.0 / 255.0 * 0.5 * (rgb.b - luma(rgb)) / (1.0 - KB);
}

fn to_v(rgb: vec3<f32>) -> f32 {
    return 128.0 / 255.0 + 224.0 / 255.0 * 0.5 * (rgb.r - luma(rgb)) / (1.0 - KR);
}

fn load_src(texel: vec2<u32>) -> vec3<f32> {
    let dims = vec2<f32>(textureDimensions(src_texture));
    let uv = (vec2<f32>(texel) + vec2(0.5)) / dims;
    // Export always composites over an opaque background, so match the old
    // CPU path and feed premultiplied display values straight through.
    return textureSample(src_texture, src_sampler, uv).rgb;
}

@fragment
fn y_main(input: YuvVertexOutput) -> @location(0) vec4<f32> {
    let texel = vec2<u32>(floor(input.position.xy));
    let y = clamp(to_y(load_src(texel)), 0.0, 1.0);
    return vec4(y, 0.0, 0.0, 1.0);
}

fn load_quad(base: vec2<u32>) -> vec3<f32> {
    let c00 = load_src(base);
    let c10 = load_src(base + vec2(1u, 0u));
    let c01 = load_src(base + vec2(0u, 1u));
    let c11 = load_src(base + vec2(1u, 1u));
    return (c00 + c10 + c01 + c11) * 0.25;
}

@fragment
fn u_main(input: YuvVertexOutput) -> @location(0) vec4<f32> {
    let base = vec2<u32>(floor(input.position.xy)) * 2u;
    let u = clamp(to_u(load_quad(base)), 0.0, 1.0);
    return vec4(u, 0.0, 0.0, 1.0);
}

@fragment
fn v_main(input: YuvVertexOutput) -> @location(0) vec4<f32> {
    let base = vec2<u32>(floor(input.position.xy)) * 2u;
    let v = clamp(to_v(load_quad(base)), 0.0, 1.0);
    return vec4(v, 0.0, 0.0, 1.0);
}
