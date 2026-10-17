import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, sample_input};
import package::generated::wave_warp::props;
import package::helpers::geometry::TAU;

fn waveform(phase: vec2<f32>, kind: u32) -> vec2<f32> {
    if kind == 1u {
        return 1.0 - 4.0 * abs(fract(phase + vec2(0.25)) - vec2(0.5));
    }

    return sin(phase * TAU);
}

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let position = uv_to_position(input.uv);
    let phase = position.yx / vec2(properties.wavelength.v0, properties.wavelength.v1)
        + vec2(properties.phase);
    let offset = waveform(phase, properties.waveform)
        * vec2(properties.amplitude.v0, properties.amplitude.v1);
    return sample_input(position - offset);
}
