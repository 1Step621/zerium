import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{context, pixel_position, over, load_input, load_source};
import package::generated::support::util::srgb;
import package::generated::glow::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let dimensions = vec2<i32>(vec2<u32>(context().output_size));
    let position = clamp(
        vec2<i32>(input.uv * vec2<f32>(dimensions)),
        vec2(0),
        dimensions - vec2(1),
    );
    let source = load_source(pixel_position(position));
    let properties = props();

    if properties.width <= 0.0 || properties.intensity <= 0.0 {
        return source;
    }

    let blurred_alpha = load_input(pixel_position(position)).a;
    let spread = clamp(properties.spread / 100.0, 0.0, 1.0);
    let base_coverage = clamp(blurred_alpha * 2.0, 0.0, 1.0);

    // Expand the bright part of the halo while letting the faint tail reach
    // zero smoothly before the finite blur kernel's rectangular boundary.
    let coverage = mix(base_coverage, smoothstep(0.0, 0.18, base_coverage), spread);
    let energy = coverage * (properties.intensity / 100.0) * properties.color.a * (1.0 - source.a);
    let glow_alpha = 1.0 - exp(-energy);
    let glow = vec4(srgb(properties.color.rgb) * glow_alpha, glow_alpha);
    return over(source, glow);
}
