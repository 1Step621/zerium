import package::generated::host::compute::{effect_input, effect_sampler};

// Shared 17-tap Gaussian kernel. Pass shaders decide radius limits, small-radius
// bypass, channel selection and edge handling for their own effect.
const OFFSETS: array<f32, 17> = array(
    -1.0, -0.875, -0.75, -0.625, -0.5, -0.375, -0.25, -0.125, 0.0,
    0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0,
);
const WEIGHTS: array<f32, 17> = array(
    0.01396019, 0.02230832, 0.03348875, 0.04722671, 0.06256523,
    0.07786368, 0.09103187, 0.09997895, 0.10315262, 0.09997895,
    0.09103187, 0.07786368, 0.06256523, 0.04722671, 0.03348875,
    0.02230832, 0.01396019,
);

fn gaussian_rgba(
    uv: vec2<f32>,
    dimensions: vec2<f32>,
    direction: vec2<f32>,
    radius: f32,
    transparent_border: bool,
) -> vec4<f32> {
    var color = vec4(0.0);
    for (var tap = 0u; tap < 17u; tap += 1u) {
        let sample_uv = uv + direction * OFFSETS[tap] * radius / dimensions;
        let sample = textureSampleLevel(effect_input, effect_sampler, sample_uv, 0.0);
        if transparent_border {
            let inside = all(sample_uv >= vec2(0.0)) && all(sample_uv <= vec2(1.0));
            color += sample * WEIGHTS[tap] * select(0.0, 1.0, inside);
        } else {
            color += sample * WEIGHTS[tap];
        }
    }
    return color;
}

fn gaussian_alpha(uv: vec2<f32>, dimensions: vec2<f32>, direction: vec2<f32>, radius: f32) -> f32 {
    var alpha = 0.0;
    for (var tap = 0u; tap < 17u; tap += 1u) {
        let sample_uv = uv + direction * OFFSETS[tap] * radius / dimensions;
        alpha += textureSampleLevel(effect_input, effect_sampler, sample_uv, 0.0).a * WEIGHTS[tap];
    }
    return alpha;
}
