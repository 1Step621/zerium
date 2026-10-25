//! Media reading, metadata refresh, and playback preparation.
mod audio;
mod image;
mod metadata;
mod reader;
mod video;
mod visual;

pub(crate) use audio::{
    AudioCalculationPolicy, AudioTimelineError, AudioTimelineGraph, sample_boundary,
};
pub(super) use metadata::read_file_revision;
pub(crate) use metadata::{
    MediaMetadataUpdater, dependency_readings, prepare_import, refresh_files,
};
pub(super) use reader::{AudioDecoder, DecodedAudioBlock, VideoDecoder};
pub(crate) use reader::{
    AudioFormat, DecodeStreamKey, DecodedFrame, MediaError, MediaInputId, MediaReaderRegistry,
    RasterSize, SamplingBranch,
};
pub(crate) use visual::{MediaFrameRequest, VisualDecodeSession};
