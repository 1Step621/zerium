import package::generated::host::compute::{context, effect_input, effect_sampler, store};
import package::generated::gaussian_blur::compute::props;
import package::shared::blur::gaussian_rgba;
override direction_x: f32 = 1.0;
override direction_y: f32 = 0.0;
override radius_scale: f32 = 1.0;

// Each axis is blurred twice at radius / sqrt(2). Convolving the two passes
// gives the same Gaussian width as one full-radius pass, while filling in
// the gaps between samples and avoiding visible bands at large radii.
@compute @workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let output_size = vec2<u32>(context().output_size);
    if global_id.x >= output_size.x || global_id.y >= output_size.y {
        return;
    }

    let properties = props();
    let dimensions = vec2<f32>(output_size);
    let uv = (vec2<f32>(global_id.xy) + vec2(0.5)) / dimensions;
    let radius = clamp(
        properties.radius * radius_scale
            * context().composition_scale,
        0.0,
        256.0,
    );
    if radius < 0.5 {
        store(
            global_id.xy,
            textureSampleLevel(effect_input, effect_sampler, uv, 0.0),
        );
        return;
    }

    let direction = vec2(direction_x, direction_y);
    store(global_id.xy, gaussian_rgba(uv, dimensions, direction, radius, false));
}
