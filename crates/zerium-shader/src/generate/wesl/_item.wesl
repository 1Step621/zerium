// Internal GPU ABI. Plugin shaders use the typed generated property API.

struct _ZeriumItem {
    property_offset: u32,
    property_size: u32,
    output_size: vec2<f32>,
    composition_size: vec2<f32>,
    surface_min: vec2<f32>,
    surface_size: vec2<f32>,
};

@group(0) @binding(0)
var<storage, read> _items: array<_ZeriumItem>;

@group(0) @binding(1)
var<storage, read> _property_words: array<u32>;

fn item_props(instance_index: u32) -> ZeriumRawProps {
    let item = _items[instance_index];
    return ZeriumRawProps(item.property_offset, item.property_size);
}
