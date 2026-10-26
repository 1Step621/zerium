import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, sample_input};
import package::generated::halation::props;
import package::helpers::light::{highlight, add_light};
import package::helpers::color::luminance;
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
    if p.radius <= 0.0 || p.amount <= 0.0 {
        return source;
    }
    var halo = 0.0;
    var coverage = 0.0;
    for (var index = 0u; index < 64u; index += 1u) {
        let angle = f32(index) * 2.39996323;
        let r = sqrt((f32(index) + 0.5) / 64.0);
        let sample = highlight(
            sample_input(position + vec2(cos(angle), sin(angle)) * p.radius * r),
            p.threshold,
        );
        halo += luminance(sample.rgb);
        coverage += sample.a;
    }
    let local = highlight(source, p.threshold);
    let intensity = max(halo / 64.0 - luminance(local.rgb), 0.0);
    let alpha = max(coverage / 64.0 - local.a, 0.0);
    let tint = scene_color(p.color);
    return add_light(source, vec4(tint.rgb * intensity, tint.a * alpha), p.amount / 100.0);
}
