import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_position, sample_input};
import package::generated::grunge::props;
import package::helpers::noise::fractal_noise;
import package::generated::support::util::{rotate, DEG2RAD, scene_color};

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let position = uv_to_position(input.uv);
    let source = sample_input(position);
    if p.amount <= 0.0 {
        return source;
    }
    let local = rotate(position - input_position(vec2(0.0)), -p.angle * DEG2RAD) / p.scale;
    let coarse = fractal_noise(local, p.phase, p.detail, p.seed);
    let grain = fractal_noise(local * 8.0, p.phase, 2u, p.seed + 7919u);
    let scratch = fractal_noise(local * vec2(30.0, 0.3), p.phase, 2u, p.seed + 3571u);
    let speckled = mix(coarse, max(coarse, smoothstep(0.55, 0.75, grain)), p.speckles / 100.0);
    let texture = mix(speckled, max(speckled, smoothstep(0.55, 0.75, scratch)), p.scratches / 100.0);
    let edge = (1.0 - p.density / 100.0) * 1.16 - 0.08;
    var damage = smoothstep(edge - 0.08, edge + 0.08, texture);
    if p.density <= 0.0 {
        damage = 0.0;
    }
    if p.density >= 100.0 {
        damage = 1.0;
    }
    if p.mode == 1u {
        return source * (1.0 - damage * p.amount / 100.0);
    }
    let fill = mix(scene_color(p.light), scene_color(p.dark), damage) * source.a;
    return mix(source, fill, p.amount / 100.0);
}
