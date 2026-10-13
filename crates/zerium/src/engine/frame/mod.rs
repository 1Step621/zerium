mod video;

pub(crate) use video::{VideoFrame, yuv_format};

use std::sync::Arc;

/// CPU pixels or GPU-decoded textures. Video stays in YUV or on the GPU
/// until upload; images and text use display-encoded, straight-alpha RGBA8.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VisualFrame {
    pub width: u32,
    pub height: u32,
    pub data: FrameData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FrameData {
    Rgba(Arc<[u8]>),
    Video(Arc<VideoFrame>),
}

/// Planar YUV layout and color metadata, independent of pixel storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct YuvFormat {
    pub subsampling: [u32; 2],
    pub bit_depth: u32,
    pub matrix: YuvMatrix,
    pub full_range: bool,
    pub chroma_location: ChromaLocation,
}

/// Borrowed pixels retain their producer's row padding until GPU upload.
pub(crate) struct PixelPlane<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) stride: u32,
}

pub(crate) enum VideoPixels<'a> {
    Rgba(PixelPlane<'a>),
    Yuv {
        format: YuvFormat,
        planes: [PixelPlane<'a>; 3],
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum YuvMatrix {
    Bt601,
    Bt709,
    Bt2020,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChromaLocation {
    Left,
    Center,
    TopLeft,
    Top,
}

impl ChromaLocation {
    pub(crate) fn position_256(self) -> [i64; 2] {
        match self {
            Self::Left => [0, 128],
            Self::Center => [128, 128],
            Self::TopLeft => [0, 0],
            Self::Top => [128, 0],
        }
    }
}

impl VisualFrame {
    pub(crate) fn rgba(width: u32, height: u32, rgba: impl Into<Arc<[u8]>>) -> Self {
        Self {
            width,
            height,
            data: FrameData::Rgba(rgba.into()),
        }
    }

    pub(crate) fn byte_len(&self) -> usize {
        match &self.data {
            FrameData::Rgba(bytes) => bytes.len(),
            FrameData::Video(frame) => frame.byte_len(),
        }
    }
}

impl YuvFormat {
    pub(crate) fn plane_size(&self, width: u32, height: u32, plane: usize) -> [u32; 2] {
        if plane == 0 {
            [width, height]
        } else {
            [
                width.div_ceil(self.subsampling[0]),
                height.div_ceil(self.subsampling[1]),
            ]
        }
    }

    pub(crate) fn bytes_per_sample(&self) -> u32 {
        if self.bit_depth > 8 { 2 } else { 1 }
    }

    pub(crate) fn conversion(&self) -> YuvConversion {
        let (kr, kb) = match self.matrix {
            YuvMatrix::Bt601 => (0.299, 0.114),
            YuvMatrix::Bt709 => (0.2126, 0.0722),
            YuvMatrix::Bt2020 => (0.2627, 0.0593),
        };
        let kg = 1. - kr - kb;
        let shift = (1u32 << (self.bit_depth - 8)) as f32;
        let max = ((1u32 << self.bit_depth) - 1) as f32;
        let (y_offset, y_scale, c_scale) = if self.full_range {
            (0., 1. / max, 1. / max)
        } else {
            (16. * shift, 1. / (219. * shift), 1. / (224. * shift))
        };
        let origin = self
            .chroma_location
            .position_256()
            .map(|position| 0.5 + position as f32 / 256.);
        YuvConversion {
            offset: [y_offset, 128. * shift, 128. * shift, 0.],
            scale: [y_scale, c_scale, c_scale, 0.],
            red: [1., 0., 2. * (1. - kr), 0.],
            green: [1., -2. * kb * (1. - kb) / kg, -2. * kr * (1. - kr) / kg, 0.],
            blue: [1., 2. * (1. - kb), 0., 0.],
            chroma: [
                self.subsampling[0] as f32,
                self.subsampling[1] as f32,
                if self.subsampling[0] == 1 {
                    0.5
                } else {
                    origin[0]
                },
                if self.subsampling[1] == 1 {
                    0.5
                } else {
                    origin[1]
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct YuvConversion {
    pub offset: [f32; 4],
    pub scale: [f32; 4],
    pub red: [f32; 4],
    pub green: [f32; 4],
    pub blue: [f32; 4],
    pub chroma: [f32; 4],
}
