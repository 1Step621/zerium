import package::generated::host::effect::{
    ZeriumVertex,
    context,
    effect_input,
    effect_sampler,
    vertex,
};
import package::generated::host::util::{DEG2RAD, scene_color};
import package::generated::noise_gradient::props;
import package::shared::noise::cell_hash;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = textureSample(effect_input, effect_sampler, input.uv);
    let size = context().surface_size;
    let cell = vec2<i32>(floor(input.uv * size / properties.grain_size));
    let point = (vec2<f32>(cell) + vec2(0.5)) * properties.grain_size - size * 0.5;
    let angle = properties.angle * DEG2RAD;
    let direction = vec2(cos(angle), sin(angle));
    let span = max(dot(abs(direction), size), 0.00001);
    let coverage = clamp(
        (dot(point, direction) / span - properties.offset / 100.0) * 100.0 / properties.width + 0.5,
        0.0,
        1.0,
    );
    let phase_seed = bitcast<u32>(i32(floor(properties.phase * 60.0)));
    let noise = cell_hash(cell, properties.seed + phase_seed * 1013u);
    // Density varies across the gradient; individual grains keep either endpoint
    // color instead of interpolating colors in the transition.
    let color = select(properties.start_color, properties.end_color, noise < coverage);
    let gradient = scene_color(color) * source.a;
    return mix(source, gradient, properties.amount / 100.0);
}
