// Internal GPU ABI. Plugin shaders use the typed generated property API.

struct _ZeriumTemporal {
    property_offset: u32,
    property_size: u32,
    sample_index: u32,
    sample_count: u32,
    frame_offset: f32,
    sample_progress: f32,
    composition_size: vec2<f32>,
    surface_min: vec2<f32>,
    surface_size: vec2<f32>,
};

@group(0) @binding(3)
var<uniform> _temporal_state: _ZeriumTemporal;

@group(0) @binding(4)
var<storage, read> _property_words: array<u32>;

fn effect_props() -> ZeriumRawProps {
    return ZeriumRawProps(
        _temporal_state.property_offset,
        _temporal_state.property_size,
    );
}
