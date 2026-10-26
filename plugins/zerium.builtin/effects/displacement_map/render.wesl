import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_info, sample_input, straight};
import package::generated::displacement_map::props;
import package::generated::displacement_map::inputs::map::{sample as sample_map, sample_uv as sample_map_uv};
import package::helpers::color::display_rgb;

fn channel(rgb: vec3<f32>, alpha: f32, selection: u32) -> f32 {
    if selection == 0u {
        return rgb.r;
    }
    if selection == 1u {
        return rgb.g;
    }
    if selection == 2u {
        return rgb.b;
    }
    if selection == 3u {
        return dot(rgb, vec3(0.2126, 0.7152, 0.0722));
    }
    return alpha;
}

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let position = uv_to_position(input.uv);
    var map = sample_map(position);
    if p.fit_map {
        map = sample_map_uv((position - input_info().bounds_min) / max(input_info().bounds_size, vec2(1e-5)));
    }
    let rgb = display_rgb(straight(map), false);
    let value = vec2(channel(rgb, map.a, p.channel_x), channel(rgb, map.a, p.channel_y));
    let offset = (value * 2.0 - vec2(1.0)) * map.a * vec2(p.displacement.v0, p.displacement.v1);
    return sample_input(position + offset);
}
