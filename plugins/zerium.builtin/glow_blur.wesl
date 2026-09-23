import package::generated::compute::{effect_input, effect_sampler, context, store};
import package::generated::properties_glow_blur::props;
import constants::{direction_x, direction_y};

// A fixed tap count makes the cost independent of the requested glow width.
// Bilinear sampling fills the gaps as the sample radius grows.
const WEIGHTS: array<f32, 9> = array(
    0.01483945, 0.04981729, 0.11832251, 0.19882900, 0.23638351,
    0.19882900, 0.11832251, 0.04981729, 0.01483945,
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

    let dimensions = vec2<f32>(output_size);
    let uv = (vec2<f32>(global_id.xy) + vec2(0.5)) / dimensions;
    let radius = clamp(properties.width * context().composition_scale, 0.0, 256.0);
    let direction = vec2(direction_x, direction_y);
    var alpha = 0.0;
    for (var tap = 0u; tap < 9u; tap += 1u) {
        let offset = (f32(tap) - 4.0) * radius / 4.0;
        let sample_uv = uv + direction * offset / dimensions;
        alpha += textureSampleLevel(effect_input, effect_sampler, sample_uv, 0.0).a
            * WEIGHTS[tap];
    }
    store(global_id.xy, vec4(0.0, 0.0, 0.0, alpha));
}
