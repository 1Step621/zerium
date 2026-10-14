import package::generated::host::_effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumPropsTuple0 {
    v0: f32,
    v1: f32,
};

struct ZeriumProps {
    center: ZeriumPropsTuple0,
    radius: f32,
    strength: f32,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)), read_f32(raw, 8u), read_f32(raw, 12u));
}
