import package::generated::_internal::properties::{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
};
import package::generated::_internal::item::item_props;

struct ZeriumPropsTuple0 {
    v0: f32,
    v1: f32,
}

struct ZeriumProps {
    position: ZeriumPropsTuple0,
    radius: f32,
    intensity: f32,
    color: vec4<f32>,
    streak: f32,
    halo: f32,
    ghosts: u32,
}

fn props(instance_index: u32) -> ZeriumProps {
    let raw = item_props(instance_index);
    return ZeriumProps(
        ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)),
        read_f32(raw, 8u),
        read_f32(raw, 12u),
        vec4(read_f32(raw, 16u), read_f32(raw, 16u + 4u), read_f32(raw, 16u + 8u), read_f32(raw, 16u + 12u)),
        read_f32(raw, 32u),
        read_f32(raw, 36u),
        read_u32(raw, 40u),
    );
}
