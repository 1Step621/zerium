import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, over, input_position, sample_input};
import package::generated::tile_layers::props;

fn tile_range(position: f32, size: f32, pitch: f32, count: f32) -> vec2<i32> {
    if pitch == 0.0 {
        return vec2(0, i32(count) - 1);
    }

    let edges = vec2(position - size, position) / pitch;
    return vec2<i32>(
        clamp(
            vec2(ceil(min(edges.x, edges.y)), floor(max(edges.x, edges.y))),
            vec2(0.0),
            vec2(count - 1.0),
        ),
    );
}

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let count = vec2<f32>(max(vec2(properties.count.v0, properties.count.v1), vec2(1u)));
    let source_size = input_position(vec2(1.0)) - input_position(vec2(0.0));
    let tile_size = source_size * vec2(properties.scale.v0, properties.scale.v1) / 100.0;

    if any(tile_size <= vec2(0.0)) {
        return vec4(0.0);
    }

    let padding = vec2(properties.padding.v0, properties.padding.v1);
    let pitch = tile_size + padding;
    let repeat_span = abs(pitch) * (count - vec2(1.0));
    let grid_size = tile_size + repeat_span;
    let origin = vec2<f32>(vec2(properties.origin.v0, properties.origin.v1));

    // Keep the source tile at the selected origin as the grid grows.
    let center = input_position(vec2(0.5)) + vec2(properties.offset.v0, properties.offset.v1)
        + (vec2(1.0) - origin) * repeat_span * 0.5;
    let position = uv_to_position(input.uv) - center + grid_size * 0.5;

    if any(position < vec2(0.0)) || any(position >= grid_size) {
        return vec4(0.0);
    }

    if all(padding >= vec2(0.0)) {
        let tile_position = position - floor(position / pitch) * pitch;

        if any(tile_position >= tile_size) {
            return vec4(0.0);
        }

        return sample_input(input_position(tile_position / tile_size));
    }

    // Composite overlapping tiles from front to back, with later rows and columns on top.
    let first_position = uv_to_position(input.uv) - center + tile_size * 0.5
        + pitch * (count - vec2(1.0)) * 0.5;
    let columns = tile_range(first_position.x, tile_size.x, pitch.x, count.x);
    let rows = tile_range(first_position.y, tile_size.y, pitch.y, count.y);
    var result = vec4(0.0);

    for(var row = rows.y; row >= rows.x; row -= 1) {
        for(var column = columns.y; column >= columns.x; column -= 1) {
            let uv = (first_position - vec2<f32>(f32(column), f32(row)) * pitch) / tile_size;

            if all(uv >= vec2(0.0)) && all(uv < vec2(1.0)) {
                result = over(result, sample_input(input_position(uv)));

                if result.a >= 1.0 {
                    return result;
                }
            }
        }
    }

    return result;
}
