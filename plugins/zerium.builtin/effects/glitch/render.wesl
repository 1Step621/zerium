import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_info, sample_input, straight};
import package::generated::glitch::props;
import package::helpers::noise::cell_hash;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let position = uv_to_position(input.uv);
    let phase = bitcast<u32>(i32(floor(p.phase * 24.0)));
    let row = i32(floor((position.y - input_info().bounds_min.y) / p.block_height));
    let seed = p.seed + phase * 1013u;
    let distorted = cell_hash(vec2(row, 0), seed) < p.density / 100.0;
    let offset = select(0.0, (cell_hash(vec2(row, 1), seed) * 2.0 - 1.0) * p.displacement, distorted);
    let point = position + vec2(offset, 0.0);
    let split = select(0.0, p.separation, distorted);
    let red = sample_input(point + vec2(split, 0.0));
    let green = sample_input(point);
    let blue = sample_input(point - vec2(split, 0.0));
    let alpha = max(red.a, max(green.a, blue.a));
    return vec4(vec3(straight(red).r, straight(green).g, straight(blue).b) * alpha, alpha);
}
