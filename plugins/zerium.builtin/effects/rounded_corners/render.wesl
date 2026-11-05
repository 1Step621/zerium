import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_info, sample_input};
import package::generated::rounded_corners::props;
import package::helpers::geometry::{rounded_rectangle_distance, distance_coverage};

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let info = input_info();
    let position = uv_to_position(input.uv);
    let half_size = info.bounds_size * 0.5;
    let local = position - info.bounds_min - half_size;
    let rounded = distance_coverage(rounded_rectangle_distance(local, half_size, p.radius), 0.00001);
    let original = distance_coverage(rounded_rectangle_distance(local, half_size, 0.0), 0.00001);
    // The input already contains its straight-edge coverage; remove only the rounded corners.
    let mask = select(min(rounded / max(original, 0.00001), 1.0), 1.0, p.radius <= 0.0);
    return sample_input(position) * mask;
}
