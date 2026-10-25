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

use super::super::reader::{ImageDecoder, MediaError, MediaReader, RasterSize};
use crate::engine::frame::{FrameCache, RASTER_CACHE_BYTES, VisualFrame};

pub(in crate::engine::media) const READER_ID: &str = "zerium.image";

pub(in crate::engine::media) struct ImageReader;

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
    source: Option<DynamicImage>,
    cached: FrameCache<(u32, u32)>,
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
            self.source = Some(decode_image(&self.path, cancelled)?);
        }
        let source = self
            .source
            .as_ref()
            .expect("image source was decoded above");
        let rgba = if (source.width(), source.height()) == (width, height) {
            source.to_rgba8()
        } else {
            source
                .resize_exact(width, height, FilterType::Triangle)
                .into_rgba8()
        };
        let frame = rgba_frame(rgba, cancelled)?;
        self.cached.insert((width, height), frame.clone());
        Ok(frame)
    }
}
