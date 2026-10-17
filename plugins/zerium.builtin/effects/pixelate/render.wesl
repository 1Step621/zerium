import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_info, input_position, sample_input};
import package::generated::pixelate::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let block = max(props().block_size, 0.000001);
    let origin = input_position(vec2(0.0));
    let position = uv_to_position(input.uv);
    let center = origin + (floor((position - origin) / block) + vec2(0.5)) * block;
    let image = input_info();
    let half_pixel = vec2(0.5) / image.density;
    return sample_input(
        clamp(
            center,
            image.bounds_min + half_pixel,
            image.bounds_min + image.bounds_size - half_pixel,
        ),
    );
}
