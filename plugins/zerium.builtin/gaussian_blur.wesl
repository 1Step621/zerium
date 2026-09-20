import package::generated::compute::{effect_input, effect_sampler, context, store};
import package::generated::properties_gaussian_blur::props;
import constants::{direction_x, direction_y, radius_scale};

// Each axis is blurred twice at radius / sqrt(2). Convolving the two passes
// gives the same Gaussian width as one full-radius pass, while filling in
// the gaps between samples and avoiding visible bands at large radii.
const BLUR_OFFSETS: array<f32, 17> = array(
    -1.0, -0.875, -0.75, -0.625, -0.5, -0.375, -0.25, -0.125, 0.0,
    0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0,
);
const BLUR_WEIGHTS: array<f32, 17> = array(
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
    var color = vec4(0.0);
    for (var tap = 0u; tap < 17u; tap += 1u) {
        let sample_uv = uv + direction * BLUR_OFFSETS[tap] * radius / dimensions;
        color += textureSampleLevel(
            effect_input,
            effect_sampler,
            sample_uv,
            0.0,
        ) * BLUR_WEIGHTS[tap];
    }
    store(global_id.xy, color);
}
