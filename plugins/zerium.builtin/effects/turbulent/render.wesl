import package::generated::host::effect::{
    ZeriumVertex,
    position_to_uv,
    sample_or_transparent,
    uv_to_position,
    vertex,
};
import package::generated::turbulent::props;
import package::helpers::noise::fractal_noise;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let position = uv_to_position(input.uv);
    let point = position / properties.scale;
    let x = fractal_noise(point, properties.phase, properties.octaves, properties.seed);
    let y = fractal_noise(point + vec2(37.0, 19.0), properties.phase, properties.octaves, properties.seed + 1013u);
    let offset = (vec2(x, y) * 2.0 - vec2(1.0)) * properties.amount;
    return sample_or_transparent(position_to_uv(position - offset));
}
