//! Paired texture and metadata bindings shared by generation and GPU encoding.

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
