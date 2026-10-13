//! Persistent video decoding shared by preview and export.
use crate::engine::gpu::GpuContext;
mod decoder;
mod gpu_decoder;

use std::path::Path;

use zerium_core::media::{MediaAsset, MediaKind, MediaMetadata, MediaTarget};

use super::reader::{MediaError, MediaReader, VideoDecoder};
use crate::engine::gstreamer::input;

pub(super) const READER_ID: &str = "zerium.video";

pub(super) struct VideoReader;

impl MediaReader for VideoReader {
    fn probe(&self, path: &Path, target: MediaTarget) -> Result<Option<MediaMetadata>, MediaError> {
        if target != MediaTarget::Visual {
            return Ok(None);
        }
        input::probe(path, target)
    }

    fn open_video_decoder(
        &self,
        asset: &MediaAsset,
        gpu: Option<&GpuContext>,
    ) -> Result<Box<dyn VideoDecoder>, MediaError> {
        if !matches!(asset.kind, MediaKind::Video { .. }) {
            return Err(MediaError::invalid_input(
                "Video reader requires video media",
            ));
        }
        Ok(Box::new(decoder::VideoDecodeSession::open(
            asset.clone(),
            gpu,
        )?))
    }
}
