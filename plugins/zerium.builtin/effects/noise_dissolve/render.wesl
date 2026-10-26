import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_position, sample_input};
import package::generated::noise_dissolve::props;
import package::helpers::noise::fractal_noise;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let position = uv_to_position(input.uv);
    let source = sample_input(position);
    if p.progress <= 0.0 {
        return vec4(0.0);
    }
    if p.progress >= 100.0 {
        return source;
    }
    let noise = fractal_noise((position - input_position(vec2(0.0))) / p.scale, p.phase, p.detail, p.seed);
    let soft = max(p.softness / 200.0, 1e-5);
    let edge = p.progress / 100.0 * (1.0 + 2.0 * soft) - soft;
    return source * (1.0 - smoothstep(edge - soft, edge + soft, noise));
}
