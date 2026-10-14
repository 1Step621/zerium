import package::generated::host::_effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    angle: f32,
    distance: f32,
    strength: f32,
    preserve_color: bool,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(read_f32(raw, 0u), read_f32(raw, 4u), read_f32(raw, 8u), read_bool(raw, 12u));
}
