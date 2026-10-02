import package::generated::host::util::ZeriumContext;

fn make_surface_context(
    output_size: vec2<f32>,
    composition_size: vec2<f32>,
    surface_size: vec2<f32>,
) -> ZeriumContext {
    let scale = output_size / surface_size;
    return ZeriumContext(output_size, composition_size, min(scale.x, scale.y), surface_size);
}
