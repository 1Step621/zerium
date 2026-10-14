import package::generated::host::effect::{ZeriumVertex, effect_input, effect_sampler, vertex};
import package::generated::host::util::srgb;
import package::generated::color_temperature::props;

// Approximate the black-body white point in encoded RGB, normalized at 6500 K.
fn white_point(kelvin: f32) -> vec3<f32> {
    let t = kelvin / 100.0;
    var r = 255.0;
    var g = 99.4708025861 * log(t) - 161.1195681661;
    var b = 0.0;
    if t > 19.0 {
        b = 138.5177312231 * log(t - 10.0) - 305.0447927307;
    }
    if t > 66.0 {
        r = 329.698727446 * pow(t - 60.0, -0.1332047592);
        g = 288.1221695283 * pow(t - 60.0, -0.0755148492);
        b = 255.0;
    }
    return srgb(clamp(vec3(r, g, b) / 255.0, vec3(0.0), vec3(1.0)));
}

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = textureSample(effect_input, effect_sampler, input.uv);
    if properties.temperature == 6500.0 || properties.amount <= 0.0 {
        return source;
    }
    let balance = white_point(properties.temperature) / white_point(6500.0);
    return vec4(source.rgb * mix(vec3(1.0), balance, properties.amount / 100.0), source.a);
}
