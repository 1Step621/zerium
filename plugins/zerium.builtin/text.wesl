import package::generated::util::{ZeriumContext, make_context, srgb_channel, srgb, scene_color, DEG2RAD};
import package::generated::item::{ZeriumItem, items, property_words, item_props, context, to_ndc, quad_corner, quad_uv, quad_position, rotate, ZeriumRawProps, read_word, read_u32, read_i32, read_f32, read_bool};
import package::generated::properties_text::{ZeriumStr, str_byte, ZeriumPropsTuple0, ZeriumPropsTuple1, ZeriumProps, props, get_font_family};
import package::generated::media_interface_text::{ZeriumMedia, slot_0, slot_0_size, media_sampler, media_inputs};

struct TextVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> TextVertexOutput {
    let properties = props(instance_index);
    let position = vec2(properties.position.v0, properties.position.v1);
    let size = vec2(properties.size.v0, properties.size.v1);
    let corner = quad_corner(vertex_index);
    var output: TextVertexOutput;
    output.position = quad_position(
        instance_index,
        position,
        corner,
        size,
    );
    output.uv = quad_uv(corner);
    return output;
}

@fragment
fn fragment_main(input: TextVertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(slot_0, media_sampler, input.uv);
    return vec4(color.rgb * color.a, color.a);
}
