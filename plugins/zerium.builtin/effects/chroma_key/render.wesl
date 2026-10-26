import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, sample_input, straight};
import package::generated::chroma_key::props;
import package::helpers::color::display_rgb;
import package::generated::support::util::srgb;

fn chroma(rgb: vec3<f32>) -> vec2<f32> {
    return vec2(dot(rgb, vec3(-0.114572, -0.385428, 0.5)), dot(rgb, vec3(0.5, -0.454153, -0.045847)));
}

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let position = uv_to_position(input.uv);
    let source = sample_input(position);
    if source.a <= 1e-5 {
        return vec4(0.0);
    }
    let rgb = display_rgb(straight(source), false);
    let key = p.color.rgb;
    let difference = length(vec3(
        chroma(rgb) - chroma(key),
        dot(rgb - key, vec3(0.2126, 0.7152, 0.0722)) * 0.5,
    ));
    let softness = max(p.softness / 100.0, 1e-5);
    let keep = smoothstep(p.tolerance / 100.0, p.tolerance / 100.0 + softness, difference);
    let key_chroma = key - vec3(dot(key, vec3(0.2126, 0.7152, 0.0722)));
    let projection = max(dot(rgb, key_chroma), 0.0) / max(dot(key_chroma, key_chroma), 1e-5);
    let neutral = clamp(rgb - key_chroma * projection * (1.0 - keep) * p.spill / 100.0, vec3(0.0), vec3(1.0));
    let alpha = source.a * keep;
    return vec4(srgb(neutral) * alpha, alpha);
}
