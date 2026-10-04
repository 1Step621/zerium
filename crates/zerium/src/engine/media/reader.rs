use std::{
    collections::HashMap,
    fmt, fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use thiserror::Error;

use crate::engine::frame::RgbaFrame;
use zerium_core::{
    media::{
        ImportedFile, MediaAsset, MediaInputMetadata, MediaInputReference, MediaKind,
        MediaMetadata, MediaTarget,
    },
    plugin::PluginManifest,
    timeline::{EffectInstanceId, ItemId},
};

use super::ffmpeg::{FfmpegMediaReader, READER_ID as FFMPEG_READER_ID};
use super::svg::{READER_ID as SVG_READER_ID, SvgMediaReader};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MediaInputId {
    pub(crate) item_id: ItemId,
    pub(crate) effect_id: Option<EffectInstanceId>,
    pub(crate) input_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DecodedAudioBlock {
    pub format: AudioFormat,
    /// Interleaved `f32` samples in channel order.
    pub samples: Arc<[f32]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct VideoProxyRequest {
    pub max_width: u32,
    pub max_height: u32,
    pub max_frames_per_second: u32,
    pub source_start: Duration,
    pub source_duration: Duration,
}

/// A proxy asset whose local time zero corresponds to `source_start` in the source.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct VideoProxy {
    pub asset: MediaAsset,
    pub source_start: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct VideoDecodeSize {
    pub max_width: u32,
    pub max_height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DecodedVideoFrame {
    pub presentation_time: Duration,
    pub duration: Duration,
    pub frame: RgbaFrame,
}

pub(crate) trait VideoDecoderSession: Send {
    /// Returns the frame whose presentation interval contains `presentation_time`,
    /// or the closest following frame when the stream has a timestamp gap.
    fn decode_at(
        &mut self,
        presentation_time: Duration,
        size: VideoDecodeSize,
        cancelled: &AtomicBool,
    ) -> Result<DecodedVideoFrame, MediaError>;

    fn decode_from(
        &mut self,
        presentation_time: Duration,
        frame_count: usize,
        size: VideoDecodeSize,
        cancelled: &AtomicBool,
    ) -> Result<Vec<DecodedVideoFrame>, MediaError>;
}

pub(crate) trait ImageDecoderSession: Send {
    fn render(
        &mut self,
        size: VideoDecodeSize,
        cancelled: &AtomicBool,
    ) -> Result<RgbaFrame, MediaError>;
}

pub(crate) enum VisualDecoderSession {
    Video(Box<dyn VideoDecoderSession>),
    Image {
        decoder: Box<dyn ImageDecoderSession>,
        duration: Duration,
    },
}

impl VisualDecoderSession {
    pub(crate) fn decode_at(
        &mut self,
        presentation_time: Duration,
        size: VideoDecodeSize,
        cancelled: &AtomicBool,
    ) -> Result<DecodedVideoFrame, MediaError> {
        match self {
            Self::Video(decoder) => decoder.decode_at(presentation_time, size, cancelled),
            Self::Image { decoder, duration } => Ok(DecodedVideoFrame {
                presentation_time: Duration::ZERO,
                duration: *duration,
                frame: decoder.render(size, cancelled)?,
            }),
        }
    }

    pub(crate) fn decode_from(
        &mut self,
        presentation_time: Duration,
        frame_count: usize,
        size: VideoDecodeSize,
        cancelled: &AtomicBool,
    ) -> Result<Vec<DecodedVideoFrame>, MediaError> {
        if frame_count == 0 {
            return Ok(Vec::new());
        }
        match self {
            Self::Video(decoder) => {
                decoder.decode_from(presentation_time, frame_count, size, cancelled)
            }
            Self::Image { .. } => Ok(vec![self.decode_at(presentation_time, size, cancelled)?]),
        }
    }
}

pub(crate) trait AudioDecoderSession: Send {
    fn stream_duration(&self) -> Duration;

    /// Decodes `sample_frames` frames. One frame contains one sample per channel.
    /// A short or empty block indicates the end of the stream.
    fn decode_sample_frames(
        &mut self,
        start_seconds: f64,
        sample_frames: usize,
        format: AudioFormat,
    ) -> Result<DecodedAudioBlock, MediaError>;
}

pub(crate) trait MediaReader: Send + Sync {
    fn probe(&self, path: &Path, target: MediaTarget) -> Result<Option<MediaMetadata>, MediaError>;

    fn open_video_decoder(
        &self,
        asset: &MediaAsset,
    ) -> Result<Box<dyn VideoDecoderSession>, MediaError> {
        let _ = asset;
        Err(MediaError::unsupported(
            "This media reader does not support video decoding",
        ))
    }

    fn open_image_decoder(
        &self,
        asset: &MediaAsset,
    ) -> Result<Box<dyn ImageDecoderSession>, MediaError> {
        let _ = asset;
        Err(MediaError::unsupported(
            "This media reader does not support still image decoding",
        ))
    }

    fn open_audio_decoder(
        &self,
        asset: &MediaAsset,
    ) -> Result<Box<dyn AudioDecoderSession>, MediaError> {
        let _ = asset;
        Err(MediaError::unsupported(
            "This media reader does not support audio decoding",
        ))
    }

    fn create_video_proxy(
        &self,
        asset: &MediaAsset,
        request: VideoProxyRequest,
    ) -> Result<VideoProxy, MediaError> {
        let _ = (asset, request);
        Err(MediaError::unsupported(
            "This media reader does not support proxy generation",
        ))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FileSourceKind {
    Item,
    Effect,
}

#[derive(Clone)]
struct RegisteredFileSource {
    plugin_id: String,
    source_id: String,
    source_label: String,
    kind: FileSourceKind,
    property_id: String,
    extensions: Vec<String>,
    inputs: Vec<(MediaInputReference, String)>,
}

pub(crate) struct MediaReaderRegistry {
    readers: HashMap<String, Arc<dyn MediaReader>>,
    file_sources: Vec<RegisteredFileSource>,
}

impl MediaReaderRegistry {
    pub(crate) fn new() -> Self {
        Self {
            readers: HashMap::new(),
            file_sources: Vec::new(),
        }
    }

    pub(crate) fn register_reader(
        &mut self,
        id: impl Into<String>,
        reader: Arc<dyn MediaReader>,
    ) -> Result<(), MediaError> {
        let id = id.into();
        if id.is_empty() {
            return Err(MediaError::invalid_input("Media reader ID cannot be empty"));
        }
        if self.readers.contains_key(&id) {
            return Err(MediaError::invalid_input(format!(
                "Media reader '{id}' is already registered"
            )));
        }
        self.readers.insert(id, reader);
        Ok(())
    }

    pub(crate) fn register_plugin(&mut self, manifest: &PluginManifest) -> Result<(), MediaError> {
        for item in manifest.items() {
            for input in item.file_properties() {
                if self.file_sources.iter().any(|registered| {
                    registered.plugin_id == manifest.id()
                        && registered.source_id == item.id()
                        && registered.property_id == input.id()
                }) {
                    return Err(MediaError::external(format!(
                        "File input '{}:{}:{}' is already registered",
                        manifest.id(),
                        item.id(),
                        input.id()
                    )));
                }
                self.file_sources.push(RegisteredFileSource {
                    plugin_id: manifest.id().to_owned(),
                    source_id: item.id().to_owned(),
                    source_label: item.label().to_owned(),
                    kind: FileSourceKind::Item,
                    property_id: input.id().to_owned(),
                    extensions: input
                        .file_type()
                        .expect("file property")
                        .extensions()
                        .to_vec(),
                    inputs: item
                        .media_sources()
                        .filter(|source| source.file == input.id())
                        .map(|source| (source.input, source.reader.to_owned()))
                        .collect(),
                });
            }
        }
        for effect in manifest.effects() {
            for input in effect.file_properties() {
                self.file_sources.push(RegisteredFileSource {
                    plugin_id: manifest.id().to_owned(),
                    source_id: effect.id().to_owned(),
                    source_label: effect.label().to_owned(),
                    kind: FileSourceKind::Effect,
                    property_id: input.id().to_owned(),
                    extensions: input
                        .file_type()
                        .expect("file property")
                        .extensions()
                        .to_vec(),
                    inputs: effect
                        .media_sources()
                        .filter(|source| source.file == input.id())
                        .map(|source| (source.input, source.reader.to_owned()))
                        .collect(),
                });
            }
        }
        Ok(())
    }

    pub(crate) fn probe_for_item(
        &self,
        path: impl AsRef<Path>,
        plugin_id: &str,
        item_id: &str,
        property_id: &str,
    ) -> Result<ImportedFile, MediaError> {
        self.probe_registered(path, plugin_id, item_id, property_id, FileSourceKind::Item)
    }

    pub(crate) fn probe_for_effect(
        &self,
        path: impl AsRef<Path>,
        plugin_id: &str,
        effect_id: &str,
        property_id: &str,
    ) -> Result<ImportedFile, MediaError> {
        self.probe_registered(
            path,
            plugin_id,
            effect_id,
            property_id,
            FileSourceKind::Effect,
        )
    }

    fn probe_registered(
        &self,
        path: impl AsRef<Path>,
        plugin_id: &str,
        source_id: &str,
        property_id: &str,
        kind: FileSourceKind,
    ) -> Result<ImportedFile, MediaError> {
        let path = path.as_ref();
        let metadata = fs::metadata(path).map_err(|error| {
            MediaError::external(format!(
                "Failed to open media file '{}': {error}",
                path.display()
            ))
        })?;
        if !metadata.is_file() {
            return Err(MediaError::external(format!(
                "'{}' is not a file",
                path.display()
            )));
        }
        let definition = self
            .file_sources
            .iter()
            .find(|definition| {
                definition.plugin_id == plugin_id
                    && definition.source_id == source_id
                    && definition.kind == kind
                    && definition.property_id == property_id
            })
            .ok_or_else(|| {
                MediaError::invalid_input(format!(
                    "File input '{plugin_id}:{source_id}:{property_id}' is not registered"
                ))
            })?;
        if !definition.extensions.is_empty()
            && !path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    definition
                        .extensions
                        .iter()
                        .any(|allowed| allowed.eq_ignore_ascii_case(extension))
                })
        {
            return Err(MediaError::external(format!(
                "'{}' is an unsupported file format for '{}'",
                path.display(),
                definition.source_label
            )));
        }
        // Several inputs may share a file and a reader. Probe each reading target
        // once; a visual and an audio stream are deliberately separate results.
        let mut probes = HashMap::new();
        let mut inputs = Vec::new();
        for (input, reader_id) in &definition.inputs {
            let key = (reader_id, input.target());
            if let std::collections::hash_map::Entry::Vacant(entry) = probes.entry(key) {
                let reader = self.readers.get(reader_id).ok_or_else(|| {
                    MediaError::ReaderUnavailable(format!(
                        "Media reader '{reader_id}' is not registered"
                    ))
                })?;
                let metadata = reader.probe(path, input.target())?;
                if metadata
                    .as_ref()
                    .is_some_and(|metadata| !metadata.accepts(input.target()))
                {
                    return Err(MediaError::invalid_input(format!(
                        "Media reader '{reader_id}' returned invalid metadata for {input:?}"
                    )));
                }
                entry.insert(metadata);
            }
            inputs.push(MediaInputMetadata {
                input: input.clone(),
                metadata: probes[&key].clone(),
            });
        }
        if !inputs.is_empty() && inputs.iter().all(|input| input.metadata.is_none()) {
            return Err(MediaError::external(format!(
                "'{}' has no stream supported by '{}'",
                path.display(),
                definition.source_label
            )));
        }
        Ok(ImportedFile {
            plugin_id: definition.plugin_id.clone(),
            source_id: definition.source_id.clone(),
            property_id: definition.property_id.to_owned(),
            path: path.to_owned(),
            inputs,
        })
    }

    fn reader_for(&self, reader_id: &str) -> Result<&Arc<dyn MediaReader>, MediaError> {
        self.readers.get(reader_id).ok_or_else(|| {
            MediaError::external(format!("Media reader '{reader_id}' is not registered"))
        })
    }

    pub(crate) fn open_visual_decoder(
        &self,
        asset: &MediaAsset,
    ) -> Result<VisualDecoderSession, MediaError> {
        let reader = self.reader_for(&asset.reader_id)?;
        match asset.kind {
            MediaKind::Video { .. } => Ok(VisualDecoderSession::Video(
                reader.open_video_decoder(asset)?,
            )),
            MediaKind::Image { .. } => Ok(VisualDecoderSession::Image {
                decoder: reader.open_image_decoder(asset)?,
                duration: asset.duration,
            }),
            MediaKind::Audio { .. } => Err(MediaError::unsupported(
                "Cannot create a video decoder from audio media",
            )),
        }
    }

    pub(crate) fn open_audio_decoder(
        &self,
        asset: &MediaAsset,
    ) -> Result<Box<dyn AudioDecoderSession>, MediaError> {
        self.reader_for(&asset.reader_id)?.open_audio_decoder(asset)
    }

    pub(crate) fn create_video_proxy(
        &self,
        asset: &MediaAsset,
        request: VideoProxyRequest,
    ) -> Result<VideoProxy, MediaError> {
        self.reader_for(&asset.reader_id)?
            .create_video_proxy(asset, request)
    }
}

pub(crate) fn bundled_media_readers(
    plugins: &zerium_core::plugin::PluginRegistry,
) -> Result<Arc<MediaReaderRegistry>, MediaError> {
    let mut registry = MediaReaderRegistry::new();
    registry.register_reader(FFMPEG_READER_ID, Arc::new(FfmpegMediaReader))?;
    registry.register_reader(SVG_READER_ID, Arc::new(SvgMediaReader))?;
    for manifest in plugins.manifests() {
        registry.register_plugin(manifest)?;
    }
    Ok(Arc::new(registry))
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub(crate) enum MediaError {
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    ReaderUnavailable(String),
    #[error("{0}")]
    External(String),
    #[error("Operation was cancelled")]
    Cancelled,
}

impl MediaError {
    pub(super) fn external(message: impl fmt::Display) -> Self {
        Self::External(message.to_string())
    }

    fn unsupported(message: impl fmt::Display) -> Self {
        Self::Unsupported(message.to_string())
    }

    fn invalid_input(message: impl fmt::Display) -> Self {
        Self::InvalidInput(message.to_string())
    }
}
