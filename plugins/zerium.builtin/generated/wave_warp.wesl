import package::generated::host::_effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumPropsTuple0 {
    v0: f32,
    v1: f32,
};

struct ZeriumPropsTuple1 {
    v0: f32,
    v1: f32,
};

struct ZeriumProps {
    amplitude: ZeriumPropsTuple0,
    wavelength: ZeriumPropsTuple1,
    phase: f32,
    waveform: u32,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)), ZeriumPropsTuple1(read_f32(raw, 8u), read_f32(raw, 8u + 4u)), read_f32(raw, 16u), read_u32(raw, 20u));
}
