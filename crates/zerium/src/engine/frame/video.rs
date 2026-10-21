//! Decoded video storage and borrowed CPU planes shared by rendering consumers.
use crate::engine::frame::{ChromaLocation, PixelPlane, VideoPixels, YuvFormat, YuvMatrix};
use crate::engine::media::MediaError;
use gstreamer as gst;
use gstreamer_video as video;
use std::sync::Mutex;
use video::prelude::*;

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
