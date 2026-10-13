//! Audio streams from sound and video files.
mod source;
mod timeline;

pub(crate) use timeline::{
    AudioClipId, AudioGainEvaluation, AudioTimelineError, AudioTimelineGraph, sample_boundary,
};

use std::path::Path;

use zerium_core::media::{MediaAsset, MediaKind, MediaMetadata, MediaTarget};

use super::reader::{AudioDecoder, MediaError, MediaReader};
use crate::engine::gstreamer::input;

pub(super) const READER_ID: &str = "zerium.audio";

pub(super) struct AudioReader;

impl MediaReader for AudioReader {
    fn probe(&self, path: &Path, target: MediaTarget) -> Result<Option<MediaMetadata>, MediaError> {
        if target != MediaTarget::Audio {
            return Ok(None);
        }
        input::probe(path, target)
    }

    fn open_audio_decoder(&self, asset: &MediaAsset) -> Result<Box<dyn AudioDecoder>, MediaError> {
        if !matches!(asset.kind, MediaKind::Audio { .. }) {
            return Err(MediaError::invalid_input(
                "Audio reader requires audio media",
            ));
        }
        Ok(Box::new(input::AudioDecodeSession::open(asset.clone())?))
    }
}
