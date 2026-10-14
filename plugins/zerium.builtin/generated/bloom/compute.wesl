import package::generated::host::_compute::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    radius: f32,
    threshold: f32,
    intensity: f32,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(read_f32(raw, 0u), read_f32(raw, 4u), read_f32(raw, 8u));
}
