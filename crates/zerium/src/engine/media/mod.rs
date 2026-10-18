//! Media reading, metadata refresh, and playback preparation.
mod audio;
mod image;
mod metadata;
mod reader;
mod video;
mod visual;

pub(crate) use audio::{
    AudioClipId, AudioGainEvaluation, AudioTimelineError, AudioTimelineGraph, sample_boundary,
};
pub(super) use metadata::read_file_revision;
pub(crate) use metadata::{MediaMetadataUpdater, item_readings, prepare_import, refresh_files};
pub(super) use reader::{AudioDecoder, DecodedAudioBlock, VideoDecoder};
pub(crate) use reader::{
    AudioFormat, DecodedFrame, MediaError, MediaInputId, MediaReaderRegistry, RasterSize,
    bundled_media_readers,
};
pub(crate) use visual::{MediaFrameRequest, VisualDecodeSession};
