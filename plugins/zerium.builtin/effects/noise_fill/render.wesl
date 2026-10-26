import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_position, sample_input};
import package::generated::noise_fill::props;
import package::helpers::noise::{cell_hash, fractal_noise, cellular_noise};
import package::generated::support::util::scene_color;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let position = uv_to_position(input.uv);
    let source = sample_input(position);
    let local = (position - input_position(vec2(0.0))) / p.scale;
    var noise = fractal_noise(local, p.phase, p.detail, p.seed);
    if p.mode == 0u {
        noise = cell_hash(vec2<i32>(floor(local)), p.seed + bitcast<u32>(i32(floor(p.phase * 60.0))) * 1013u);
    } else if p.mode == 2u {
        noise = cellular_noise(local + p.phase * vec2(0.37, 0.53), p.seed);
    }
    noise = clamp((noise - 0.5) * p.contrast / 100.0 + 0.5, 0.0, 1.0);
    let texture = mix(scene_color(p.dark), scene_color(p.light), noise) * source.a;
    return mix(source, texture, p.amount / 100.0);
}
