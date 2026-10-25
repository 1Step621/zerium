//! Image interface declarations and the binding layout shared with the renderer.

use crate::MAX_TEXTURE_INPUTS;

const IMAGE_COUNT: usize = MAX_TEXTURE_INPUTS + 2;
pub const IMAGE_SAMPLER_BINDING: u32 = (IMAGE_COUNT * 2) as u32;

/// One image's texture and metadata always occupy adjacent bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageBinding {
    texture: u32,
}

impl ImageBinding {
    pub const INPUT: Self = Self::at(MAX_TEXTURE_INPUTS);
    pub const SOURCE: Self = Self::at(MAX_TEXTURE_INPUTS + 1);

    pub fn named(index: usize) -> Self {
        assert!(index < MAX_TEXTURE_INPUTS);
        Self::at(index)
    }

    pub fn all() -> impl ExactSizeIterator<Item = Self> {
        (0..IMAGE_COUNT).map(Self::at)
    }

    pub const fn texture(self) -> u32 {
        self.texture
    }

    pub const fn metadata(self) -> u32 {
        self.texture + 1
    }

    const fn at(index: usize) -> Self {
        Self {
            texture: (index * 2) as u32,
        }
    }
}

pub(super) fn shared_interface() -> String {
    format!(
        "{}\n@group(1) @binding({IMAGE_SAMPLER_BINDING})\nvar _image_sampler: sampler;\n",
        include_str!("wesl/internal/image.wesl"),
    )
}

pub(super) fn interface(binding: ImageBinding) -> String {
    let texture_binding = binding.texture();
    let metadata_binding = binding.metadata();

    format!(
        r#"import package::generated::_internal::image::{{
    ZeriumImage,
    _image_sampler,
    _image_position,
    _sample_image,
    _load_image
}};

@group(1) @binding({texture_binding})
var _texture: texture_2d<f32>;

@group(1) @binding({metadata_binding})
var<uniform> _info: ZeriumImage;

fn info() -> ZeriumImage {{
    return _info;
}}

fn position(uv: vec2<f32>) -> vec2<f32> {{
    return _image_position(_info, uv);
}}

fn reference_uv(position: vec2<f32>) -> vec2<f32> {{
    return (position - _info.bounds_min) / max(_info.bounds_size, vec2(0.000001));
}}

fn sample(position: vec2<f32>) -> vec4<f32> {{
    return _sample_image(_texture, _info, _image_sampler, position);
}}

fn sample_uv(uv: vec2<f32>) -> vec4<f32> {{
    return sample(position(uv));
}}

fn load(position: vec2<f32>) -> vec4<f32> {{
    return _load_image(_texture, _info, position);
}}
"#
    )
}
