import package::generated::effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumPropsTuple0 {
    v0: f32,
    v1: f32,
};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    offset: ZeriumPropsTuple0,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(raw, ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)));
}
