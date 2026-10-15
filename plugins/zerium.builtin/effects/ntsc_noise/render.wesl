import package::generated::host::effect::{
    ZeriumVertex,
    input_uv_to_position,
    position_to_uv,
    sample_or_transparent,
    straight,
    uv_to_position,
    vertex,
};
import package::generated::host::util::srgb;
import package::generated::ntsc_noise::props;
import package::helpers::color::encode_srgb;
import package::helpers::noise::cell_hash;

fn yiq(rgb: vec3<f32>) -> vec3<f32> {
    return vec3(dot(rgb, vec3(0.299, 0.587, 0.114)), dot(rgb, vec3(0.596, -0.274, -0.322)), dot(rgb, vec3(0.211, -0.523, 0.312)));
}

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let position = uv_to_position(input.uv);
    if properties.noise <= 0.0 && properties.chroma_shift == 0.0 && properties.tracking <= 0.0 && properties.scanlines <= 0.0 {
        return sample_or_transparent(position_to_uv(position));
    }
    // Keep noise cells relative to the input, independent of output padding.
    let pixel = position - input_uv_to_position(vec2(0.0));
    let phase = i32(floor(properties.phase * 60.0));
    let row_noise = cell_hash(vec2(i32(floor(pixel.y)), phase), properties.seed);
    let jitter = (row_noise - 0.5) * properties.tracking;
    let source_position = position + vec2(jitter, 0.0);
    let source = sample_or_transparent(position_to_uv(source_position));
    let shifted = sample_or_transparent(position_to_uv(source_position + vec2(properties.chroma_shift, 0.0)));
    let luma = yiq(encode_srgb(straight(source))).x;
    let chroma = yiq(encode_srgb(straight(shifted))).yz;
    let grain = (cell_hash(vec2<i32>(floor(pixel)) + vec2(phase, -phase), properties.seed + 1013u) - 0.5) * properties.noise / 100.0;
    let scanline = 1.0 - properties.scanlines / 100.0 * (0.5 + 0.5 * cos(pixel.y * 3.14159265359));
    let y = (luma + grain) * scanline;
    let rgb = vec3(y + 0.956 * chroma.x + 0.621 * chroma.y, y - 0.272 * chroma.x - 0.647 * chroma.y, y - 1.106 * chroma.x + 1.703 * chroma.y);
    return vec4(srgb(clamp(rgb, vec3(0.0), vec3(1.0))) * source.a, source.a);
}
