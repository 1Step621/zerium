//! Still raster images, independent of video pipelines and seeking.
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use image::{
    DynamicImage, ImageDecoder, ImageReader as RasterReader, imageops::FilterType,
    metadata::Orientation,
};
use zerium_core::media::{MediaAsset, MediaKind, MediaMetadata, MediaTarget};

use super::super::reader::{
    ImageDecoderSession, MediaError, MediaReader, VideoDecodeSize, fit_dimensions,
};
use crate::engine::frame::PixelFrame;

pub(in crate::engine::media) const READER_ID: &str = "zerium.image";

pub(in crate::engine::media) struct ImageReader;

impl MediaReader for ImageReader {
    fn probe(&self, path: &Path, target: MediaTarget) -> Result<Option<MediaMetadata>, MediaError> {
        if target != MediaTarget::Visual {
            return Ok(None);
        }
        let mut decoder = open_decoder(path)?;
        let (mut width, mut height) = decoder.dimensions();
        if matches!(
            decoder.orientation().map_err(MediaError::external)?,
            Orientation::Rotate90
                | Orientation::Rotate270
                | Orientation::Rotate90FlipH
                | Orientation::Rotate270FlipH
        ) {
            std::mem::swap(&mut width, &mut height);
        }
        Ok(Some(MediaMetadata {
            duration: Duration::from_secs(5),
            kind: MediaKind::Image { width, height },
        }))
    }

    fn open_image_decoder(
        &self,
        asset: &MediaAsset,
    ) -> Result<Box<dyn ImageDecoderSession>, MediaError> {
        let MediaKind::Image { width, height } = asset.kind else {
            return Err(MediaError::invalid_input(
                "Image reader requires image media",
            ));
        };
        Ok(Box::new(RasterDecoder {
            path: asset.path.clone(),
            source_size: (width, height),
            cached: None,
        }))
    }
}

fn open_decoder(path: &Path) -> Result<impl ImageDecoder, MediaError> {
    RasterReader::open(path)
        .and_then(RasterReader::with_guessed_format)
        .map_err(MediaError::external)?
        .into_decoder()
        .map_err(MediaError::external)
}

struct RasterDecoder {
    path: PathBuf,
    source_size: (u32, u32),
    cached: Option<PixelFrame>,
}

impl ImageDecoderSession for RasterDecoder {
    fn render(
        &mut self,
        size: VideoDecodeSize,
        cancelled: &AtomicBool,
    ) -> Result<PixelFrame, MediaError> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        let (width, height) = fit_dimensions(
            self.source_size.0,
            self.source_size.1,
            size.max_width,
            size.max_height,
        )
        .ok_or_else(|| MediaError::invalid_input("Invalid image dimensions"))?;
        if let Some(frame) = &self.cached
            && (frame.width, frame.height) == (width, height)
        {
            return Ok(frame.clone());
        }
        // Keep only the displayed resolution; decode the source again on resize.
        self.cached = None;
        let mut decoder = open_decoder(&self.path)?;
        let orientation = decoder.orientation().map_err(MediaError::external)?;
        let mut image = DynamicImage::from_decoder(decoder).map_err(MediaError::external)?;
        if cancelled.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        image.apply_orientation(orientation);
        if (image.width(), image.height()) != (width, height) {
            image = image.resize_exact(width, height, FilterType::Triangle);
        }
        if cancelled.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        let frame = PixelFrame::rgba(width, height, image.into_rgba8().into_raw());
        self.cached = Some(frame.clone());
        Ok(frame)
    }
}
