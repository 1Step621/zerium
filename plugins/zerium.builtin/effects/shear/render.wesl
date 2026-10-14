import package::generated::host::effect::{
    ZeriumVertex,
    position_to_uv,
    sample_or_transparent,
    uv_to_position,
    vertex,
};
import package::generated::shear::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let center = vec2(properties.center.v0, properties.center.v1);
    let output = uv_to_position(input.uv) - center;
    // Invert Y shear, then X shear, matching the declared bounds.
    let y = output.y - output.x * properties.amount.v1 / 100.0;
    let x = output.x - y * properties.amount.v0 / 100.0;
    return sample_or_transparent(position_to_uv(center + vec2(x, y)));
}
