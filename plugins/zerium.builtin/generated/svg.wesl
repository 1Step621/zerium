import package::generated::host::_props::{ZeriumRawProps, item_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumPropsTuple0 {
    v0: f32,
    v1: f32,
};

struct ZeriumPropsTuple1 {
    v0: f32,
    v1: f32,
};

struct ZeriumProps {
    position: ZeriumPropsTuple0,
    size: ZeriumPropsTuple1,
};

fn props(instance_index: u32) -> ZeriumProps {
    let raw = item_props(instance_index);
    return ZeriumProps(ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)), ZeriumPropsTuple1(read_f32(raw, 8u), read_f32(raw, 8u + 4u)));
}

@group(1) @binding(0)
var source: texture_2d<f32>;

@group(1) @binding(8)
var capability_sampler: sampler;
