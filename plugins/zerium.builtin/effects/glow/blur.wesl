import package::generated::host::compute::{context, store};
import package::generated::glow::compute::props;
import package::helpers::blur::gaussian_alpha;
override direction_x: f32 = 1.0;
override direction_y: f32 = 0.0;
override radius_scale: f32 = 1.0;

// Like the Gaussian blur effect, blur each axis twice. Different radii stagger the
// sample positions at large widths; their squared scales still add to one.
@compute @workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let output_size = vec2<u32>(context().output_size);
    if global_id.x >= output_size.x || global_id.y >= output_size.y {
        return;
    }

    let properties = props();
    if properties.width <= 0.0 || properties.intensity <= 0.0 {
        store(global_id.xy, vec4(0.0));
        return;
    }

    let dimensions = vec2<f32>(output_size);
    let uv = (vec2<f32>(global_id.xy) + vec2(0.5)) / dimensions;
    let radius = clamp(
        properties.width * radius_scale * context().composition_scale,
        0.0,
        256.0,
    );
    let direction = vec2(direction_x, direction_y);
    let alpha = gaussian_alpha(uv, dimensions, direction, radius);
    store(global_id.xy, vec4(0.0, 0.0, 0.0, alpha));
}
