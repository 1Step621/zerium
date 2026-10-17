import package::generated::_internal::image::{
    ZeriumImage,
    _image_sampler,
    _image_position,
    _sample_image,
    _load_image
};

@group(1) @binding(18)
var _texture: texture_2d<f32>;

@group(1) @binding(19)
var<uniform> _info: ZeriumImage;

fn info() -> ZeriumImage {
    return _info;
}

fn position(uv: vec2<f32>) -> vec2<f32> {
    return _image_position(_info, uv);
}

fn reference_uv(position: vec2<f32>) -> vec2<f32> {
    return (position - _info.bounds_min) / max(_info.bounds_size, vec2(0.000001));
}

fn sample(position: vec2<f32>) -> vec4<f32> {
    return _sample_image(_texture, _info, _image_sampler, position);
}

fn sample_uv(uv: vec2<f32>) -> vec4<f32> {
    return sample(position(uv));
}

fn load(position: vec2<f32>) -> vec4<f32> {
    return _load_image(_texture, _info, position);
}
