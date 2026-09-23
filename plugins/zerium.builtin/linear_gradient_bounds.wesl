import package::generated::compute::{effect_input, context, store};
import constants::phase;

// Encode two pixel coordinates as four bytes. The pooled effect textures use
// rgba16float, so storing coordinates directly would round large positions.
fn encode_pair(first: u32, second: u32) -> vec4<f32> {
    let a = first + 1u;
    let b = second + 1u;
    return vec4<f32>(
        f32(a & 255u),
        f32((a >> 8u) & 255u),
        f32(b & 255u),
        f32((b >> 8u) & 255u),
    ) / 255.0;
}

fn decode_pair(value: vec4<f32>) -> vec2<u32> {
    let bytes = vec4<u32>(round(clamp(value, vec4(0.0), vec4(1.0)) * 255.0));
    return vec2<u32>(
        bytes.x | (bytes.y << 8u),
        bytes.z | (bytes.w << 8u),
    );
}

var<workgroup> minimum_x: array<u32, 256>;
var<workgroup> maximum_x: array<u32, 256>;
var<workgroup> minimum_y: array<u32, 256>;
var<workgroup> maximum_y: array<u32, 256>;

@compute @workgroup_size(256, 1, 1)
fn compute_main(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
) {
    let size = vec2<u32>(context().output_size);
    var min_x = size.x;
    var max_x = 0u;
    var min_y = size.y;
    var max_y = 0u;

    if phase == 0u {
        // One workgroup per row; each lane examines every 256th source pixel.
        let y = group.y;
        for (var x = lane; x < size.x; x += 256u) {
            if textureLoad(effect_input, vec2<i32>(i32(x), i32(y)), 0).a > 0.0 {
                min_x = min(min_x, x);
                max_x = max(max_x, x);
            }
        }
    } else {
        // Reduce the occupied row bounds to one box for the entire input.
        for (var y = lane; y < size.y; y += 256u) {
            let row = decode_pair(textureLoad(effect_input, vec2<i32>(0, i32(y)), 0));
            if row.x != 0u {
                min_x = min(min_x, row.x - 1u);
                max_x = max(max_x, row.y - 1u);
                min_y = min(min_y, y);
                max_y = max(max_y, y);
            }
        }
    }

    minimum_x[lane] = min_x;
    maximum_x[lane] = max_x;
    minimum_y[lane] = min_y;
    maximum_y[lane] = max_y;
    workgroupBarrier();

    for (var stride = 128u; stride > 0u; stride >>= 1u) {
        if lane < stride {
            minimum_x[lane] = min(minimum_x[lane], minimum_x[lane + stride]);
            maximum_x[lane] = max(maximum_x[lane], maximum_x[lane + stride]);
            minimum_y[lane] = min(minimum_y[lane], minimum_y[lane + stride]);
            maximum_y[lane] = max(maximum_y[lane], maximum_y[lane + stride]);
        }
        workgroupBarrier();
    }

    if lane == 0u {
        let occupied = minimum_x[0] < size.x;
        let x_bounds = select(vec4(0.0), encode_pair(minimum_x[0], maximum_x[0]), occupied);
        if phase == 0u {
            store(vec2(0u, group.y), x_bounds);
        } else {
            store(vec2(0u, 0u), x_bounds);
            let y_bounds = select(
                vec4(0.0),
                encode_pair(minimum_y[0], maximum_y[0]),
                occupied,
            );
            if size.x > 1u {
                store(vec2(1u, 0u), y_bounds);
            } else if size.y > 1u {
                store(vec2(0u, 1u), y_bounds);
            }
        }
    }
}
