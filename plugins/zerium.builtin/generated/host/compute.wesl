import package::generated::_internal::effect_state::_pass_state;

@group(0) @binding(2)
var _output_image: texture_storage_2d<rgba16float, write>;

fn store(pixel: vec2<u32>, color: vec4<f32>) {
    if all(pixel < _pass_state.output_size) {
        textureStore(_output_image, pixel, color);
    }
}
