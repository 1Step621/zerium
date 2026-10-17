import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, straight, sample_input};
import package::generated::support::util::scene_color;
import package::generated::monochrome::props;
import package::helpers::color::luminance;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = sample_input(uv_to_position(input.uv));
    let luminance = luminance(straight(source));
    let tint = scene_color(properties.color) * source.a;
    return vec4(tint.rgb * select(1.0, luminance, properties.use_luminance), tint.a);
}
