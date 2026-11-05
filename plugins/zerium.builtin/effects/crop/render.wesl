import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_info, sample_input};
import package::generated::crop::props;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let source = input_info();
    let before = max(vec2(p.left, p.top), vec2(0.0));
    let after = max(vec2(p.right, p.bottom), vec2(0.0));
    let lower = source.bounds_min + before;
    let upper = source.bounds_min + source.bounds_size - after;
    let position = uv_to_position(input.uv);
    let pixel_size = max(fwidth(position), vec2(0.000001));
    if any(upper <= lower) {
        return vec4(0.0);
    }
    // Uncropped edges already carry the input image's coverage.
    let leading = select(vec2(1.0), clamp((position - lower) / pixel_size + 0.5, vec2(0.0), vec2(1.0)), before > vec2(0.0));
    let trailing = select(vec2(1.0), clamp((upper - position) / pixel_size + 0.5, vec2(0.0), vec2(1.0)), after > vec2(0.0));
    let coverage = max(leading + trailing - vec2(1.0), vec2(0.0));
    return sample_input(position) * coverage.x * coverage.y;
}
