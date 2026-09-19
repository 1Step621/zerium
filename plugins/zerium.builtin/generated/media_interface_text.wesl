struct ZeriumMedia {
    source_sizes: array<vec4<f32>, 1>,
    target_size: vec2<f32>,
    padding: vec2<f32>,
};

@group(0) @binding(2)
var slot_0: texture_2d<f32>;

fn slot_0_size() -> vec2<f32> {
    return media_inputs.source_sizes[0].xy;
}

@group(0) @binding(3)
var media_sampler: sampler;

@group(0) @binding(4)
var<uniform> media_inputs: ZeriumMedia;
