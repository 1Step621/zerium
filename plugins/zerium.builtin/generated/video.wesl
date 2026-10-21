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

struct ZeriumPropsTuple1 {
    v0: u32,
    v1: u32,
}

struct ZeriumPropsTuple2 {
    v0: f32,
    v1: f32,
}

struct ZeriumProps {
    position: ZeriumPropsTuple0,
    origin: ZeriumPropsTuple1,
    size: ZeriumPropsTuple2,
    source_start: f32,
    source_duration: f32,
    playback_speed: f32,
    end_behavior: u32,
}

fn props(instance_index: u32) -> ZeriumProps {
    let raw = item_props(instance_index);
    return ZeriumProps(
        ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)),
        ZeriumPropsTuple1(read_u32(raw, 8u), read_u32(raw, 8u + 4u)),
        ZeriumPropsTuple2(read_f32(raw, 16u), read_f32(raw, 16u + 4u)),
        read_f32(raw, 24u),
        read_f32(raw, 28u),
        read_f32(raw, 32u),
        read_u32(raw, 36u),
    );
}
