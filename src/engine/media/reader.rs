use std::{
    collections::HashMap,
    error::Error,
    fmt, fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use crate::{
    domain::{
        media::{ImportedMedia, MediaAsset, MediaKind},
        plugin::{MediaType, PluginManifest},
    },
    engine::frame::RgbaFrame,
};

use super::ffmpeg::{FfmpegMediaReader, READER_ID as FFMPEG_READER_ID};

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MediaStreamDurations {
    pub video: Option<Duration>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MediaProbe {
    pub duration: Duration,
    pub kind: MediaKind,
    pub streams: MediaStreamDurations,
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
    fn stream_duration(&self) -> Duration;

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

pub(crate) trait AudioDecoderSession: Send {
    fn stream_duration(&self) -> Duration;

    /// Decodes `sample_frames` frames. One frame contains one sample per channel.
    fn decode_sample_frames(
        &mut self,
        start_seconds: f64,
        sample_frames: usize,
        format: AudioFormat,
    ) -> Result<DecodedAudioBlock, MediaError>;
}

pub(crate) trait MediaReader: Send + Sync {
    fn probe(&self, path: &Path, media_type: MediaType) -> Result<Option<MediaProbe>, MediaError>;

    fn open_video_decoder(
        &self,
        asset: &MediaAsset,
    ) -> Result<Box<dyn VideoDecoderSession>, MediaError> {
        let _ = asset;
        Err(MediaError::unsupported(
            "このメディアリーダーは動画デコードに対応していません",
        ))
    }

    fn open_audio_decoder(
        &self,
        asset: &MediaAsset,
    ) -> Result<Box<dyn AudioDecoderSession>, MediaError> {
        let _ = asset;
        Err(MediaError::unsupported(
            "このメディアリーダーは音声デコードに対応していません",
        ))
    }

    fn create_video_proxy(
        &self,
        asset: &MediaAsset,
        request: VideoProxyRequest,
    ) -> Result<VideoProxy, MediaError> {
        let _ = (asset, request);
        Err(MediaError::unsupported(
            "このメディアリーダーはプロキシ生成に対応していません",
        ))
    }
}

#[derive(Clone)]
struct RegisteredFileItem {
    plugin_id: String,
    item_id: String,
    item_label: String,
    input: crate::domain::plugin::FileCapability,
}

pub(crate) struct MediaReaderRegistry {
    readers: HashMap<String, Arc<dyn MediaReader>>,
    file_items: Vec<RegisteredFileItem>,
}

impl MediaReaderRegistry {
    pub(crate) fn new() -> Self {
        Self {
            readers: HashMap::new(),
            file_items: Vec::new(),
        }
    }

    pub(crate) fn register_reader(
        &mut self,
        id: impl Into<String>,
        reader: Arc<dyn MediaReader>,
    ) -> Result<(), MediaError> {
        let id = id.into();
        if id.is_empty() {
            return Err(MediaError::invalid_input("メディアリーダーIDが空です"));
        }
        if self.readers.contains_key(&id) {
            return Err(MediaError::invalid_input(format!(
                "メディアリーダー'{id}'はすでに登録されています"
            )));
        }
        self.readers.insert(id, reader);
        Ok(())
    }

    pub(crate) fn register_plugin(&mut self, manifest: &PluginManifest) -> Result<(), MediaError> {
        for item in manifest.items() {
            for input in item.files() {
                if self.file_items.iter().any(|registered| {
                    registered.plugin_id == manifest.id()
                        && registered.item_id == item.id()
                        && registered.input.id() == input.id()
                }) {
                    return Err(MediaError::external(format!(
                        "ファイル入力'{}:{}:{}'はすでに登録されています",
                        manifest.id(),
                        item.id(),
                        input.id()
                    )));
                }
                self.file_items.push(RegisteredFileItem {
                    plugin_id: manifest.id().to_owned(),
                    item_id: item.id().to_owned(),
                    item_label: item.label().to_owned(),
                    input: input.clone(),
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
        input_id: &str,
    ) -> Result<ImportedMedia, MediaError> {
        let path = path.as_ref();
        let metadata = fs::metadata(path).map_err(|error| {
            MediaError::external(format!(
                "メディアファイル '{}' を開けません: {error}",
                path.display()
            ))
        })?;
        if !metadata.is_file() {
            return Err(MediaError::external(format!(
                "'{}' はファイルではありません",
                path.display()
            )));
        }
        let definition = self
            .file_items
            .iter()
            .find(|definition| {
                definition.plugin_id == plugin_id
                    && definition.item_id == item_id
                    && definition.input.id() == input_id
            })
            .ok_or_else(|| {
                MediaError::invalid_input(format!(
                    "ファイル対応アイテム'{plugin_id}:{item_id}'が登録されていません"
                ))
            })?;
        let file = &definition.input;
        if !file.extensions().is_empty()
            && !path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    file.extensions()
                        .iter()
                        .any(|allowed| allowed.eq_ignore_ascii_case(extension))
                })
        {
            return Err(MediaError::external(format!(
                "'{}' はアイテム'{}'で対応していないファイル形式です",
                path.display(),
                definition.item_label
            )));
        }
        let reader = self.readers.get(file.reader()).ok_or_else(|| {
            MediaError::ReaderUnavailable(format!(
                "メディアリーダー'{}'が登録されていません",
                file.reader()
            ))
        })?;
        let probe = reader.probe(path, file.media_type())?.ok_or_else(|| {
            MediaError::external(format!(
                "'{}'はこのアイテムで読み込めません",
                path.display()
            ))
        })?;
        if probe.kind.media_type() != file.media_type() {
            return Err(MediaError::external(format!(
                "選択したファイルの種類がアイテム'{}'と一致しません",
                definition.item_label
            )));
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Media")
            .to_owned();
        Ok(ImportedMedia {
            plugin_id: definition.plugin_id.clone(),
            item_id: definition.item_id.clone(),
            input_id: definition.input.id().to_owned(),
            asset: MediaAsset {
                reader_id: file.reader().to_owned(),
                path: path.to_path_buf(),
                name,
                duration: probe.duration,
                kind: probe.kind,
            },
        })
    }

    pub(crate) fn open_video_decoder(
        &self,
        asset: &MediaAsset,
    ) -> Result<Box<dyn VideoDecoderSession>, MediaError> {
        let reader = self.readers.get(&asset.reader_id).ok_or_else(|| {
            MediaError::external(format!(
                "メディアリーダー'{}'が登録されていません",
                asset.reader_id
            ))
        })?;
        reader.open_video_decoder(asset)
    }

    pub(crate) fn open_audio_decoder(
        &self,
        asset: &MediaAsset,
    ) -> Result<Box<dyn AudioDecoderSession>, MediaError> {
        let reader = self.readers.get(&asset.reader_id).ok_or_else(|| {
            MediaError::external(format!(
                "メディアリーダー'{}'が登録されていません",
                asset.reader_id
            ))
        })?;
        reader.open_audio_decoder(asset)
    }

    pub(crate) fn create_video_proxy(
        &self,
        asset: &MediaAsset,
        request: VideoProxyRequest,
    ) -> Result<VideoProxy, MediaError> {
        let reader = self.readers.get(&asset.reader_id).ok_or_else(|| {
            MediaError::external(format!(
                "メディアリーダー'{}'が登録されていません",
                asset.reader_id
            ))
        })?;
        reader.create_video_proxy(asset, request)
    }
}

pub(crate) fn bundled_media_readers(
    plugins: &crate::domain::plugin::PluginRegistry,
) -> Result<Arc<MediaReaderRegistry>, MediaError> {
    let mut registry = MediaReaderRegistry::new();
    registry.register_reader(FFMPEG_READER_ID, Arc::new(FfmpegMediaReader))?;
    for manifest in plugins.manifests() {
        registry.register_plugin(manifest)?;
    }
    Ok(Arc::new(registry))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MediaError {
    Unsupported(String),
    InvalidInput(String),
    ReaderUnavailable(String),
    External(String),
    Cancelled,
}

impl MediaError {
    pub(super) fn external(message: impl Into<String>) -> Self {
        Self::External(message.into())
    }

    fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported(message.into())
    }

    fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }
}

impl fmt::Display for MediaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("キャンセルされました"),
            Self::Unsupported(message)
            | Self::InvalidInput(message)
            | Self::ReaderUnavailable(message)
            | Self::External(message) => formatter.write_str(message),
        }
    }
}

impl Error for MediaError {}
