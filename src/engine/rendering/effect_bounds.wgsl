struct EffectBounds {
    // Zero initialization is enough: the minimum is stored from the right edge.
    from_right: atomic<u32>,
    from_bottom: atomic<u32>,
    max_x: atomic<u32>,
    max_y: atomic<u32>,
};

@group(0) @binding(0)
var source: texture_2d<f32>;

@group(0) @binding(1)
var<storage, read_write> effect_bounds: EffectBounds;

var<workgroup> tile_min_x: atomic<u32>;
var<workgroup> tile_min_y: atomic<u32>;
var<workgroup> tile_max_x: atomic<u32>;
var<workgroup> tile_max_y: atomic<u32>;

@compute @workgroup_size(8, 8)
fn main(
    @builtin(workgroup_id) group_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let size = textureDimensions(source);
    if all(local_id.xy == vec2(0u)) {
        atomicStore(&tile_min_x, size.x);
        atomicStore(&tile_min_y, size.y);
        atomicStore(&tile_max_x, 0u);
        atomicStore(&tile_max_y, 0u);
    }
    workgroupBarrier();

    let base = group_id.xy * 32u + local_id.xy * 4u;
    var min_position = size;
    var max_position = vec2(0u);
    for (var y = 0u; y < 4u; y += 1u) {
        for (var x = 0u; x < 4u; x += 1u) {
            let position = base + vec2(x, y);
            if all(position < size) && textureLoad(source, vec2<i32>(position), 0).a != 0.0 {
                min_position = min(min_position, position);
                max_position = max(max_position, position + 1u);
            }
        }
    }
    if max_position.x > 0u {
        atomicMin(&tile_min_x, min_position.x);
        atomicMin(&tile_min_y, min_position.y);
        atomicMax(&tile_max_x, max_position.x);
        atomicMax(&tile_max_y, max_position.y);
    }
    workgroupBarrier();

    if all(local_id.xy == vec2(0u)) && atomicLoad(&tile_max_x) > 0u {
        atomicMax(&effect_bounds.from_right, size.x - atomicLoad(&tile_min_x));
        atomicMax(&effect_bounds.from_bottom, size.y - atomicLoad(&tile_min_y));
        atomicMax(&effect_bounds.max_x, atomicLoad(&tile_max_x));
        atomicMax(&effect_bounds.max_y, atomicLoad(&tile_max_y));
    }
}
