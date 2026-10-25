//! Persistent video decoding shared by preview and export.
use crate::engine::{
    GpuContext,
    input::{MediaError, MediaReader, VideoDecoder, probe_streams},
};
use gstreamer as gst;
use gstreamer_video as video;
use std::{path::Path, sync::Mutex};
use video::prelude::*;
use zerium_core::media::{MediaAsset, MediaKind, MediaMetadata, MediaTarget};

mod decoder;

pub(in crate::engine::input) const READER_ID: &str = "zerium.video";

pub(in crate::engine::input) struct VideoReader;

impl MediaReader for VideoReader {
    fn probe(&self, path: &Path, target: MediaTarget) -> Result<Option<MediaMetadata>, MediaError> {
        if target != MediaTarget::Visual {
            return Ok(None);
        }
        probe_streams(path, target)
    }

    fn open_video_decoder(
        &self,
        asset: &MediaAsset,
        gpu: Option<&GpuContext>,
    ) -> Result<Box<dyn VideoDecoder>, MediaError> {
        let MediaKind::Video { .. } = asset.kind else {
            return Err(MediaError::invalid_input(
                "Video reader requires video media",
            ));
        };
        Ok(Box::new(decoder::VideoDecodeSession::new(
            asset.clone(),
            gpu,
        )))
    }
}

/// CPU pixels or GPU-decoded planes until first upload, then an owned RGB texture.
/// Conversion releases the decoder's output once the GPU has finished reading it.
pub(crate) struct VideoFrame {
    info: video::VideoInfo,
    storage: Mutex<FrameStorage>,
}

enum FrameStorage {
    Cpu(gst::Buffer),
    GpuPlanes {
        device: wgpu::Device,
        planes: gpu_video::VideoTexture,
        format: YuvFormat,
    },
    RgbaTexture {
        device: wgpu::Device,
        texture: wgpu::Texture,
    },
}

impl std::fmt::Debug for VideoFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoFrame")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl PartialEq for VideoFrame {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for VideoFrame {}

impl VideoFrame {
    pub(crate) fn dimensions(&self) -> [u32; 2] {
        [self.info.width(), self.info.height()]
    }

    pub(crate) fn from_cpu(info: video::VideoInfo, buffer: gst::Buffer) -> Self {
        Self {
            info,
            storage: Mutex::new(FrameStorage::Cpu(buffer)),
        }
    }

    pub(crate) fn from_gpu(
        info: video::VideoInfo,
        device: &wgpu::Device,
        planes: gpu_video::VideoTexture,
        format: YuvFormat,
    ) -> Self {
        Self {
            info,
            storage: Mutex::new(FrameStorage::GpuPlanes {
                device: device.clone(),
                planes,
                format,
            }),
        }
    }

    pub(crate) fn gpu_planes(
        &self,
        device: &wgpu::Device,
    ) -> Result<Option<(gpu_video::VideoTexture, YuvFormat)>, MediaError> {
        match &*self.storage.lock().unwrap_or_else(|e| e.into_inner()) {
            FrameStorage::GpuPlanes {
                device: owner,
                planes,
                format,
            } if owner == device => Ok(Some((planes.clone(), *format))),
            FrameStorage::GpuPlanes { .. } => Err(MediaError::external(
                "A video frame cannot be reused on a different GPU device",
            )),
            _ => Ok(None),
        }
    }

    fn buffer(&self) -> Option<gst::Buffer> {
        match &*self.storage.lock().unwrap_or_else(|e| e.into_inner()) {
            FrameStorage::Cpu(buffer) => Some(buffer.clone()),
            FrameStorage::GpuPlanes { .. } | FrameStorage::RgbaTexture { .. } => None,
        }
    }

    pub(crate) fn rgba_texture(
        &self,
        device: &wgpu::Device,
    ) -> Result<Option<wgpu::Texture>, MediaError> {
        match &*self.storage.lock().unwrap_or_else(|e| e.into_inner()) {
            FrameStorage::Cpu(_) | FrameStorage::GpuPlanes { .. } => Ok(None),
            FrameStorage::RgbaTexture {
                device: owner,
                texture,
            } if owner == device => Ok(Some(texture.clone())),
            FrameStorage::RgbaTexture { .. } => Err(MediaError::external(
                "A video frame cannot be reused on a different GPU device",
            )),
        }
    }

    pub(crate) fn store_rgba_texture(&self, device: &wgpu::Device, texture: &wgpu::Texture) {
        *self.storage.lock().unwrap_or_else(|e| e.into_inner()) = FrameStorage::RgbaTexture {
            device: device.clone(),
            texture: texture.clone(),
        };
    }

    pub(crate) fn map_cpu_pixels(&self) -> Result<MappedVideoFrame, MediaError> {
        let buffer = self
            .buffer()
            .ok_or_else(|| MediaError::external("Video pixels are already GPU resident"))?;
        map_pixels(buffer, &self.info)
    }

    pub(crate) fn byte_len(&self) -> usize {
        // Include the owned RGBA texture as well as source padding.
        self.info
            .size()
            .max(self.info.width() as usize * self.info.height() as usize * 4)
    }
}

/// Owns the map so borrowed plane slices remain valid through queue.write_texture.
pub(crate) struct MappedVideoFrame(video::VideoFrame<video::video_frame::Readable>);

impl MappedVideoFrame {
    pub(crate) fn pixels(&self) -> Result<VideoPixels<'_>, MediaError> {
        let frame = &self.0;
        if let Some((subsampling, bit_depth)) = yuv_layout(frame.format()) {
            let format = yuv_format(frame.info(), subsampling, bit_depth)
                .ok_or_else(|| MediaError::external("Unsupported YUV color metadata"))?;
            let plane = |index| {
                let [width, height] = format.plane_size(frame.width(), frame.height(), index);
                self.plane(index, width * format.bytes_per_sample(), height)
            };
            Ok(VideoPixels::Yuv {
                format,
                planes: [plane(0)?, plane(1)?, plane(2)?],
            })
        } else {
            Ok(VideoPixels::Rgba(self.plane(
                0,
                frame.width() * 4,
                frame.height(),
            )?))
        }
    }

    fn plane(&self, index: usize, row_bytes: u32, rows: u32) -> Result<PixelPlane<'_>, MediaError> {
        let stride = u32::try_from(self.0.plane_stride()[index]).map_err(MediaError::external)?;
        let bytes = self
            .0
            .plane_data(index as u32)
            .map_err(MediaError::external)?;
        let required = (rows.saturating_sub(1) as usize)
            .checked_mul(stride as usize)
            .and_then(|size| size.checked_add(row_bytes as usize))
            .ok_or_else(|| MediaError::external("Video plane size overflow"))?;
        if row_bytes > stride || bytes.len() < required {
            return Err(MediaError::external("Invalid video plane layout"));
        }
        Ok(PixelPlane { bytes, stride })
    }
}

fn yuv_layout(format: video::VideoFormat) -> Option<([u32; 2], u32)> {
    use video::VideoFormat as F;
    match format {
        F::I420 => Some(([2, 2], 8)),
        F::I42010le => Some(([2, 2], 10)),
        F::I42012le => Some(([2, 2], 12)),
        F::Y42b => Some(([2, 1], 8)),
        F::I42210le => Some(([2, 1], 10)),
        F::I42212le => Some(([2, 1], 12)),
        F::Y444 => Some(([1, 1], 8)),
        F::Y44410le => Some(([1, 1], 10)),
        F::Y44412le => Some(([1, 1], 12)),
        _ => None,
    }
}

fn map_pixels(
    buffer: gst::Buffer,
    info: &video::VideoInfo,
) -> Result<MappedVideoFrame, MediaError> {
    use video::VideoFormat as F;
    let mapped = video::VideoFrame::from_buffer_readable(buffer, info)
        .map_err(|_| MediaError::external("Could not map decoded video pixels"))?;
    let info = mapped.info();
    if info.format() == F::Rgba
        || yuv_layout(info.format()).is_some_and(|(subsampling, bit_depth)| {
            yuv_format(info, subsampling, bit_depth).is_some()
        })
    {
        return Ok(MappedVideoFrame(mapped));
    }
    let format = match info.format() {
        F::Nv12 => F::I420,
        F::P01010le => F::I42010le,
        _ => F::Rgba,
    };
    let mut builder = video::VideoInfo::builder(format, mapped.width(), mapped.height());
    if format != F::Rgba {
        builder = builder
            .colorimetry(&info.colorimetry())
            .chroma_site(info.chroma_site());
    }
    let info = builder.build().map_err(MediaError::external)?;
    let buffer = gst::Buffer::with_size(info.size()).map_err(MediaError::external)?;
    let mut output = video::VideoFrame::from_buffer_writable(buffer, &info)
        .map_err(|_| MediaError::external("Could not map converted video pixels"))?;
    video::VideoConverter::new(mapped.info(), &info, None)
        .map_err(MediaError::external)?
        .frame(&mapped, &mut output);
    map_pixels(output.into_buffer(), &info)
}

/// Color metadata shared by CPU and GPU decoded planes.
pub(crate) fn yuv_format(
    info: &video::VideoInfo,
    subsampling: [u32; 2],
    bit_depth: u32,
) -> Option<YuvFormat> {
    let color = info.colorimetry();
    let matrix = match color.matrix() {
        video::VideoColorMatrix::Bt709 => YuvMatrix::Bt709,
        video::VideoColorMatrix::Bt2020 => YuvMatrix::Bt2020,
        video::VideoColorMatrix::Bt601 | video::VideoColorMatrix::Unknown => YuvMatrix::Bt601,
        _ => return None,
    };
    let chroma = info.chroma_site();
    Some(YuvFormat {
        subsampling,
        bit_depth,
        matrix,
        full_range: color.range() == video::VideoColorRange::Range0_255,
        chroma_location: match (
            chroma.contains(video::VideoChromaSite::H_COSITED),
            chroma.contains(video::VideoChromaSite::V_COSITED),
        ) {
            (true, true) => ChromaLocation::TopLeft,
            (true, false) => ChromaLocation::Left,
            (false, true) => ChromaLocation::Top,
            (false, false) => ChromaLocation::Center,
        },
    })
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
