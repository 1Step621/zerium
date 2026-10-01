import package::generated::host::_props::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    width: f32,
    intensity: f32,
    spread: f32,
    color: vec4<f32>,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(read_f32(raw, 0u), read_f32(raw, 4u), read_f32(raw, 8u), vec4(read_f32(raw, 12u), read_f32(raw, 12u + 4u), read_f32(raw, 12u + 8u), read_f32(raw, 12u + 12u)));
}
