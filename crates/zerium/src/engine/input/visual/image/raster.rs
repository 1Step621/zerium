//! Still raster images, independent of video pipelines and seeking.
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use image::{
    DynamicImage, ImageDecoder as RasterImageDecoder, ImageReader as RasterReader,
    imageops::FilterType, metadata::Orientation,
};
use zerium_core::media::{MediaAsset, MediaKind, MediaMetadata, MediaTarget};

use crate::engine::input::{
    FrameCache, FrameData, ImageDecoder, MediaError, MediaReader, RASTER_CACHE_BYTES, RasterSize,
    VisualFrame,
};

pub(in crate::engine::input) const READER_ID: &str = "zerium.image";

pub(in crate::engine::input) struct ImageReader;

impl MediaReader for ImageReader {
    fn probe(&self, path: &Path, target: MediaTarget) -> Result<Option<MediaMetadata>, MediaError> {
        if target != MediaTarget::Visual {
            return Ok(None);
        }
        let (width, height) = image_dimensions(path)?;
        Ok(Some(MediaMetadata {
            duration: Duration::ZERO,
            kind: MediaKind::Image { width, height },
        }))
    }

    fn open_image_decoder(&self, asset: &MediaAsset) -> Result<Box<dyn ImageDecoder>, MediaError> {
        let MediaKind::Image { width, height } = asset.kind else {
            return Err(MediaError::invalid_input(
                "Image reader requires image media",
            ));
        };
        Ok(Box::new(RasterDecodeSession {
            path: asset.path.clone(),
            source_size: (width, height),
            source: None,
            cached: FrameCache::new(RASTER_CACHE_BYTES),
        }))
    }
}

fn open_decoder(path: &Path) -> Result<impl RasterImageDecoder, MediaError> {
    RasterReader::open(path)
        .and_then(RasterReader::with_guessed_format)
        .map_err(MediaError::external)?
        .into_decoder()
        .map_err(MediaError::external)
}

/// Header inspection shared by standalone images and directory sequences.
pub(super) fn image_dimensions(path: &Path) -> Result<(u32, u32), MediaError> {
    let mut decoder = open_decoder(path)?;
    let (mut width, mut height) = decoder.dimensions();
    if let Orientation::Rotate90
    | Orientation::Rotate270
    | Orientation::Rotate90FlipH
    | Orientation::Rotate270FlipH = decoder.orientation().map_err(MediaError::external)?
    {
        std::mem::swap(&mut width, &mut height);
    }
    Ok((width, height))
}

/// Decode the pixels once, with the same orientation used during discovery.
pub(super) fn decode_image(
    path: &Path,
    cancelled: &AtomicBool,
) -> Result<DynamicImage, MediaError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(MediaError::Cancelled);
    }
    let mut decoder = open_decoder(path)?;
    let orientation = decoder.orientation().map_err(MediaError::external)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(MediaError::external)?;
    if cancelled.load(Ordering::Relaxed) {
        return Err(MediaError::Cancelled);
    }
    image.apply_orientation(orientation);
    Ok(image)
}

pub(super) fn rgba_frame(
    image: image::RgbaImage,
    cancelled: &AtomicBool,
) -> Result<Arc<VisualFrame>, MediaError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(MediaError::Cancelled);
    }
    Ok(Arc::new(VisualFrame::rgba(
        image.width(),
        image.height(),
        image.into_raw(),
    )))
}

struct RasterDecodeSession {
    path: PathBuf,
    source_size: (u32, u32),
    source: Option<RasterSource>,
    cached: FrameCache<(u32, u32)>,
}

enum RasterSource {
    Image(DynamicImage),
    // Share native RGBA8 pixels with full-size frames, including images that
    // exceed the resized-frame cache budget.
    Rgba(Arc<VisualFrame>),
}

impl RasterSource {
    fn decode(path: &Path, cancelled: &AtomicBool) -> Result<Self, MediaError> {
        match decode_image(path, cancelled)? {
            DynamicImage::ImageRgba8(image) => rgba_frame(image, cancelled).map(Self::Rgba),
            image => Ok(Self::Image(image)),
        }
    }

    fn render(
        &self,
        width: u32,
        height: u32,
        cancelled: &AtomicBool,
    ) -> Result<Arc<VisualFrame>, MediaError> {
        let rgba = match self {
            Self::Rgba(frame) => {
                if (frame.width, frame.height) == (width, height) {
                    return Ok(frame.clone());
                }
                let FrameData::Rgba(bytes) = &frame.data else {
                    unreachable!("raster source contains RGBA pixels");
                };
                let image = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(
                    frame.width,
                    frame.height,
                    bytes.as_ref(),
                )
                .expect("decoded RGBA image dimensions");
                image::imageops::resize(&image, width, height, FilterType::Triangle)
            }
            Self::Image(image) if (image.width(), image.height()) == (width, height) => {
                image.to_rgba8()
            }
            Self::Image(image) => image
                .resize_exact(width, height, FilterType::Triangle)
                .into_rgba8(),
        };
        rgba_frame(rgba, cancelled)
    }
}

impl ImageDecoder for RasterDecodeSession {
    fn render(
        &mut self,
        size: RasterSize,
        cancelled: &AtomicBool,
    ) -> Result<Arc<VisualFrame>, MediaError> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        let (width, height) = size
            .fit(self.source_size)
            .ok_or_else(|| MediaError::invalid_input("Invalid image dimensions"))?;
        if let Some(frame) = self.cached.get(&(width, height)) {
            return Ok(frame);
        }
        if self.source.is_none() {
            self.source = Some(RasterSource::decode(&self.path, cancelled)?);
        }
        let source = self
            .source
            .as_ref()
            .expect("image source was decoded above");
        let frame = source.render(width, height, cancelled)?;
        // Native RGBA pixels are already retained by the source itself.
        if let RasterSource::Rgba(original) = source
            && Arc::ptr_eq(original, &frame)
        {
            return Ok(frame);
        }
        self.cached.insert((width, height), frame.clone());
        Ok(frame)
    }
}
