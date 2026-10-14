import package::generated::host::compute::{context, store};
import package::shared::blur::gaussian_rgba;

// Staggered separable passes blur bloom colors and shadow coverage.
fn blur_rgba(
    global_id: vec3<u32>,
    logical_radius: f32,
    direction: vec2<f32>,
    radius_scale: f32,
) {
    let size = vec2<u32>(context().output_size);
    if any(global_id.xy >= size) {
        return;
    }
    let dimensions = vec2<f32>(size);
    let uv = (vec2<f32>(global_id.xy) + vec2(0.5)) / dimensions;
    let radius = logical_radius * radius_scale * context().composition_scale;
    store(global_id.xy, gaussian_rgba(uv, dimensions, direction, radius, true));
}
