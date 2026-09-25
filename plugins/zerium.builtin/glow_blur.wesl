import package::generated::compute::{effect_input, effect_sampler, context, store};
import package::generated::properties_glow_blur::props;
import package::generated::effect_bounds::contains;
import constants::{direction_x, direction_y, radius_scale};

// Like gaussian_blur.wesl, blur each axis twice. Different radii stagger the
// sample positions at large widths; their squared scales still add to one.
const BLUR_OFFSETS: array<f32, 17> = array(
    -1.0, -0.875, -0.75, -0.625, -0.5, -0.375, -0.25, -0.125, 0.0,
    0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0,
);
const WEIGHTS: array<f32, 17> = array(
    0.01396019, 0.02230832, 0.03348875, 0.04722671, 0.06256523,
    0.07786368, 0.09103187, 0.09997895, 0.10315262, 0.09997895,
    0.09103187, 0.07786368, 0.06256523, 0.04722671, 0.03348875,
    0.02230832, 0.01396019,
);

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

    // The two horizontal and two vertical kernels each reach at most their
    // radius. Include bilinear sampling and rounding at every pass.
    let reach = properties.width * context().composition_scale * 1.39384685 + 4.0;
    if !contains(global_id.xy, output_size, reach) {
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
    var alpha = 0.0;
    for (var tap = 0u; tap < 17u; tap += 1u) {
        let sample_uv = uv + direction * BLUR_OFFSETS[tap] * radius / dimensions;
        alpha += textureSampleLevel(effect_input, effect_sampler, sample_uv, 0.0).a
            * WEIGHTS[tap];
    }
    store(global_id.xy, vec4(0.0, 0.0, 0.0, alpha));
}
