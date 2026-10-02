// Internal GPU ABI. Plugin shaders use the typed generated property API.

struct _ZeriumEffect {
    property_offset: u32,
    property_size: u32,
    sample_index: u32,
    sample_count: u32,
    frame_offset: f32,
    sample_progress: f32,
    composition_size: vec2<f32>,
    surface_min: vec2<f32>,
    surface_size: vec2<f32>,
    input_min: vec2<f32>,
    input_size: vec2<f32>,
};

@group(0) @binding(2)
var<uniform> _effect_instance: _ZeriumEffect;

@group(0) @binding(3)
var<storage, read> _property_words: array<u32>;

fn effect_props() -> ZeriumRawProps {
    return ZeriumRawProps(
        _effect_instance.property_offset,
        _effect_instance.property_size,
    );
}
