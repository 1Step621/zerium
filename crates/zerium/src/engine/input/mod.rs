//! Audio and visual media discovery and reading.

use self::{
    audio::{AudioReader, READER_ID as AUDIO_READER_ID},
    visual::{
        IMAGE_READER_ID, ImageDecoder, ImageReader, ImageSequenceReader, SEQUENCE_READER_ID,
        SVG_READER_ID, SvgMediaReader, VIDEO_READER_ID, VideoReader,
    },
};
use super::{GpuContext, GstreamerPipeline, initialize_gstreamer};
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app::AppSink;
use gstreamer_pbutils::{
    Discoverer, DiscovererInfo, DiscovererResult, prelude::DiscovererStreamInfoExt,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fmt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use thiserror::Error;
use zerium_core::{
    media::{
        FileMediaMetadata, ImportedFile, MediaAsset, MediaDependency, MediaKind, MediaMetadata,
        MediaMetadataCache, MediaRevision, MediaTarget, ProbedFile, VideoFrameRate,
    },
    plugin::PluginRegistry,
    timeline::{ProjectId, TimelineSnapshot, TimelineView},
};

mod audio;
mod visual;

pub(super) use self::visual::VideoDecoder;
pub(crate) use self::{
    audio::{AudioDecoder, AudioFormat},
    visual::{
        DecodeStreamKey, DecodedFrame, FrameCache, FrameData, MediaFrameRequest, MediaInputId,
        PixelPlane, RASTER_CACHE_BYTES, RasterSize, SamplingBranch, VideoFrame, VideoPixels,
        VisualDecodeSession, VisualFrame, YuvFormat, YuvMatrix, yuv_format,
    },
};

trait MediaReader: Send + Sync {
    /// Readers observe changes to their source format.
    fn revision(&self, path: &Path) -> std::io::Result<MediaRevision> {
        read_file_revision(path)
    }

    fn probe(&self, path: &Path, target: MediaTarget) -> Result<Option<MediaMetadata>, MediaError>;

    /// GPU decoding uses the consumer's device and queue. Without them, or when
    /// the GPU cannot decode the stream, the decoder produces CPU pixels.
    fn open_video_decoder(
        &self,
        asset: &MediaAsset,
        gpu: Option<&GpuContext>,
    ) -> Result<Box<dyn VideoDecoder>, MediaError> {
        let _ = (asset, gpu);
        Err(MediaError::unsupported(
            "This media reader does not support video decoding",
        ))
    }

    fn open_image_decoder(&self, asset: &MediaAsset) -> Result<Box<dyn ImageDecoder>, MediaError> {
        let _ = asset;
        Err(MediaError::unsupported(
            "This media reader does not support still image decoding",
        ))
    }

    fn open_audio_decoder(&self, asset: &MediaAsset) -> Result<Box<dyn AudioDecoder>, MediaError> {
        let _ = asset;
        Err(MediaError::unsupported(
            "This media reader does not support audio decoding",
        ))
    }
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
    #[error("Media decoder worker terminated unexpectedly")]
    WorkerStopped,
    #[error("Operation was cancelled")]
    Cancelled,
}

impl MediaError {
    pub(crate) fn external(message: impl fmt::Display) -> Self {
        Self::External(message.to_string())
    }

    fn unsupported(message: impl fmt::Display) -> Self {
        Self::Unsupported(message.to_string())
    }

    fn invalid_input(message: impl fmt::Display) -> Self {
        Self::InvalidInput(message.to_string())
    }
}

fn reader_for(reader_id: &str) -> Result<&'static dyn MediaReader, MediaError> {
    match reader_id {
        IMAGE_READER_ID => Ok(&ImageReader),
        SEQUENCE_READER_ID => Ok(&ImageSequenceReader),
        VIDEO_READER_ID => Ok(&VideoReader),
        AUDIO_READER_ID => Ok(&AudioReader),
        SVG_READER_ID => Ok(&SvgMediaReader),
        _ => Err(MediaError::ReaderUnavailable(format!(
            "Media reader '{reader_id}' is not registered"
        ))),
    }
}

pub(super) fn open_audio_decoder(asset: &MediaAsset) -> Result<Box<dyn AudioDecoder>, MediaError> {
    reader_for(&asset.reader_id)?.open_audio_decoder(asset)
}

/// Default observation for readers backed by a single regular file.
fn read_file_revision(path: &Path) -> std::io::Result<MediaRevision> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Not a regular file",
        ));
    }
    Ok(MediaRevision::File {
        size: metadata.len(),
        modified: metadata.modified()?,
    })
}

#[derive(Default)]
pub(crate) struct MediaReadings {
    sources: HashSet<MediaDependency>,
    /// Suppress repeated failures until the source changes or is explicitly retried.
    failures: HashMap<MediaDependency, Option<MediaRevision>>,
    retries: HashSet<PathBuf>,
}

#[derive(Default)]
pub(crate) struct MediaRefresh {
    pub(crate) files: Vec<ProbedFile>,
    pub(crate) invalidated: Vec<MediaDependency>,
    pub(crate) errors: Vec<(MediaDependency, MediaError)>,
}

#[derive(Default)]
pub(crate) struct MediaMetadataUpdater {
    revision: Option<(ProjectId, u64)>,
    readings: MediaReadings,
}

impl MediaMetadataUpdater {
    pub(crate) fn retry(&mut self, path: &Path) {
        self.readings.retries.insert(path.to_owned());
    }

    pub(crate) fn refresh(&mut self, snapshot: &TimelineSnapshot) -> MediaRefresh {
        let revision = (
            snapshot.project_id(),
            snapshot.media_dependencies_revision(),
        );
        if self.revision != Some(revision) {
            if self
                .revision
                .is_some_and(|previous| previous.0 != revision.0)
            {
                self.readings = MediaReadings::default();
            }
            self.readings.sources = snapshot.project_media_dependencies();
            self.readings
                .failures
                .retain(|source, _| self.readings.sources.contains(source));
            self.revision = Some(revision);
        }
        refresh_files(&mut self.readings, snapshot.media_cache())
    }
}

/// Discover every interpretation before committing an imported source. Import
/// errors have the same semantics regardless of the property's picker UI.
pub(crate) fn prepare_import(
    path: impl AsRef<Path>,
    plugins: &PluginRegistry,
    plugin_id: &str,
    item_id: &str,
    property_id: &str,
) -> Result<ImportedFile, MediaError> {
    let path = path.as_ref();
    let item = plugins.item(plugin_id, item_id).ok_or_else(|| {
        MediaError::invalid_input(format!("Item '{plugin_id}:{item_id}' is not registered"))
    })?;
    if item.file_property(property_id).is_none() {
        return Err(MediaError::invalid_input(format!(
            "File property '{property_id}' was not found"
        )));
    }
    let mut readings = dependency_readings(
        item.media_sources()
            .filter(|source| source.file == property_id)
            .map(|source| MediaDependency {
                path: path.to_owned(),
                reader_id: source.reader.to_owned(),
                target: source.input.target(),
            }),
    );
    let refresh = refresh_files(&mut readings, &MediaMetadataCache::default());
    if let Some((_, error)) = refresh.errors.into_iter().next() {
        return Err(error);
    }
    let file = refresh
        .files
        .into_iter()
        .next()
        .unwrap_or_else(|| ProbedFile {
            path: path.to_owned(),
            media: Vec::new(),
        });
    Ok(ImportedFile {
        plugin_id: plugin_id.to_owned(),
        source_id: item_id.to_owned(),
        property_id: property_id.to_owned(),
        file,
    })
}

pub(crate) fn refresh_files(
    readings: &mut MediaReadings,
    cache: &MediaMetadataCache,
) -> MediaRefresh {
    let retries = std::mem::take(&mut readings.retries);
    let mut files: BTreeMap<PathBuf, ProbedFile> = BTreeMap::new();
    let mut refresh = MediaRefresh::default();
    for source in &readings.sources {
        let forced = retries.contains(&source.path);
        let previous = cache.reading(source);
        let reader = reader_for(&source.reader_id);
        let revision = reader
            .as_ref()
            .map_err(|error| (*error).clone())
            .and_then(|reader| {
                reader.revision(&source.path).map_err(|error| {
                    MediaError::external(format!("'{}': {error}", source.path.display()))
                })
            });
        let observed = revision.as_ref().ok().copied();
        if !forced
            && (readings.failures.get(source) == Some(&observed)
                || observed.is_some()
                    && previous.is_some_and(|reading| Some(reading.revision) == observed))
        {
            continue;
        }
        let result = revision.and_then(|revision| {
            let reader = reader
                .as_ref()
                .expect("revision requires an available reader");
            let metadata = reader.probe(&source.path, source.target)?;
            if metadata
                .as_ref()
                .is_some_and(|metadata| !metadata.accepts(source.target))
            {
                return Err(MediaError::invalid_input(format!(
                    "Media reader '{}' returned invalid metadata",
                    source.reader_id
                )));
            }
            let after = reader.revision(&source.path).ok();
            if after != Some(revision) {
                return Err(MediaError::external(format!(
                    "'{}' changed while it was being read",
                    source.path.display()
                )));
            }
            Ok(FileMediaMetadata {
                reader: source.reader_id.clone(),
                target: source.target,
                revision,
                metadata,
            })
        });
        match result {
            Ok(reading) => {
                readings.failures.remove(source);
                if previous != Some(&reading) {
                    files
                        .entry(source.path.clone())
                        .or_insert_with(|| ProbedFile {
                            path: source.path.clone(),
                            media: Vec::new(),
                        })
                        .media
                        .push(reading);
                }
            }
            Err(error) => {
                readings.failures.insert(source.clone(), observed);
                // An observed replacement that cannot be interpreted must not
                // keep displaying decoded pixels from the previous revision.
                // Missing files retain their observation until they reappear.
                if observed.is_some()
                    && previous.is_some()
                    && reader
                        .as_ref()
                        .is_ok_and(|reader| reader.revision(&source.path).is_ok())
                {
                    refresh.invalidated.push(source.clone());
                }
                refresh.errors.push((source.clone(), error));
            }
        }
    }
    refresh.files = files.into_values().collect();
    refresh
}

pub(crate) fn dependency_readings(
    dependencies: impl IntoIterator<Item = MediaDependency>,
) -> MediaReadings {
    MediaReadings {
        sources: dependencies.into_iter().collect(),
        ..Default::default()
    }
}

fn probe_streams(path: &Path, target: MediaTarget) -> Result<Option<MediaMetadata>, MediaError> {
    let info = discover(path)?;
    let kind = match target {
        MediaTarget::Visual => {
            let Some(video) = info
                .video_streams()
                .into_iter()
                .find(|video| !video.is_image())
            else {
                return Ok(None);
            };
            let rate = video.framerate();
            let frame_rate = u32::try_from(rate.numer())
                .ok()
                .zip(u32::try_from(rate.denom()).ok())
                .and_then(|(numerator, denominator)| VideoFrameRate::new(numerator, denominator));
            let duration = duration(&info)?;
            MediaKind::Video {
                width: video.width(),
                height: video.height(),
                codec: video.caps().and_then(|caps| {
                    caps.structure(0)
                        .map(|structure| structure.name().to_string())
                }),
                frame_rate,
                // Discoverer can report 0/1 for variable-rate video and has no
                // exact frame count. Preserve unknown cadence instead of failing
                // discovery or inventing a constant frame rate.
                frame_count: frame_rate.map(|rate| {
                    (duration.as_secs_f64() * rate.frames_per_second())
                        .ceil()
                        .clamp(1., u64::MAX as f64) as u64
                }),
            }
        }
        MediaTarget::Audio => {
            let Some(audio) = info.audio_streams().into_iter().next() else {
                return Ok(None);
            };
            MediaKind::Audio {
                channels: Some(audio.channels()).filter(|channels| *channels > 0),
                sample_rate: Some(audio.sample_rate()).filter(|rate| *rate > 0),
            }
        }
    };
    Ok(Some(MediaMetadata {
        duration: duration(&info)?,
        kind,
    }))
}

const DISCOVERY_CACHE_ENTRIES: usize = 128;

type Discovery = Arc<OnceLock<Result<DiscovererInfo, String>>>;

struct CachedDiscovery {
    path: PathBuf,
    revision: MediaRevision,
    discovery: Discovery,
}

fn discover(path: &Path) -> Result<DiscovererInfo, MediaError> {
    initialize_gstreamer().map_err(MediaError::external)?;
    let path = std::fs::canonicalize(path).map_err(MediaError::external)?;
    let revision = read_file_revision(&path).map_err(MediaError::external)?;
    static CACHE: OnceLock<Mutex<VecDeque<CachedDiscovery>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Mutex::default);
    let discovery = {
        let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
        let previous = cache
            .iter()
            .position(|entry| entry.path == path)
            .and_then(|index| cache.remove(index))
            .filter(|entry| entry.revision == revision);
        let discovery = previous.map_or_else(|| Arc::new(OnceLock::new()), |entry| entry.discovery);
        cache.push_back(CachedDiscovery {
            path: path.clone(),
            revision,
            discovery: discovery.clone(),
        });
        if cache.len() > DISCOVERY_CACHE_ENTRIES {
            cache.pop_front();
        }
        discovery
    };
    // Concurrent audio/video requests share one discovery without holding the
    // cache lock while GStreamer opens and prerolls the file.
    let result = discovery.get_or_init(|| {
        let started = Instant::now();
        let discoverer =
            Discoverer::new(gst::ClockTime::from_seconds(10)).map_err(|error| error.to_string())?;
        discoverer.connect_source_setup(|_, source| {
            // Discoverer owns the uridecodebin containing this URI source.
            // Metadata probing must not initialize a separate hardware decoder.
            if let Some(bin) = source
                .parent()
                .and_then(|parent| parent.downcast::<gst::Bin>().ok())
            {
                configure_decode_source(bin.upcast_ref(), false);
                configure_video_decoders(&bin);
            }
        });
        let info = discoverer
            .discover_uri(file_uri(&path).map_err(|error| error.to_string())?.as_str())
            .map_err(|error| error.to_string())?;
        if info.result() != DiscovererResult::Ok {
            return Err(format!(
                "Failed to discover '{}': {:?}",
                path.display(),
                info.result()
            ));
        }
        log::info!(
            "Media discovery completed in {:?}: {}",
            started.elapsed(),
            path.display()
        );
        Ok(info)
    });
    match result {
        Ok(info) => Ok(info.clone()),
        Err(error) => {
            // Retry transient failures on the next request, even if unchanged.
            cache
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .retain(|entry| !Arc::ptr_eq(&entry.discovery, &discovery));
            Err(MediaError::external(error.clone()))
        }
    }
}

/// Keep demuxing and parsing in GStreamer; only the CPU route opens video decoders.
fn configure_decode_source(source: &gst::Element, gpu: bool) {
    let Some(signal) = gst::glib::subclass::SignalId::lookup("autoplug-select", source.type_())
    else {
        return;
    };
    let result = gst::glib::EnumClass::with_type(signal.query().return_type().type_())
        .expect("autoplug-select returns an enum");
    source.connect("autoplug-select", false, move |values| {
        let factory = values[3]
            .get::<gst::ElementFactory>()
            .expect("decoder factory");
        let video_decoder = factory
            .has_type(gst::ElementFactoryType::DECODER | gst::ElementFactoryType::MEDIA_VIDEO);
        let skip = video_decoder && (gpu || factory.has_type(gst::ElementFactoryType::HARDWARE));
        result.to_value_by_nick(if skip { "skip" } else { "try" })
    });
}

/// Bound decoder concurrency during both metadata discovery and frame decoding.
fn configure_video_decoders(bin: &gst::Bin) {
    bin.connect_deep_element_added(|_, _, element| {
        if element.factory().is_some_and(|factory| {
            factory
                .has_type(gst::ElementFactoryType::DECODER | gst::ElementFactoryType::MEDIA_VIDEO)
        }) && element.find_property("max-threads").is_some()
        {
            element.set_property("max-threads", 4i32);
        }
    });
}

fn duration(info: &gstreamer_pbutils::DiscovererInfo) -> Result<Duration, MediaError> {
    info.duration()
        .filter(|duration| !duration.is_zero())
        .map(|duration| Duration::from_nanos(duration.nseconds()))
        .ok_or_else(|| MediaError::external("Failed to get media duration"))
}

fn file_uri(path: &Path) -> Result<url::Url, MediaError> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(MediaError::external)?
            .join(path)
    };
    url::Url::from_file_path(path).map_err(|_| MediaError::invalid_input("Invalid media path"))
}

struct DecodePipeline {
    pipeline: GstreamerPipeline,
    sink: AppSink,
}

impl DecodePipeline {
    fn new() -> Self {
        Self {
            pipeline: GstreamerPipeline::new(gst::Pipeline::new()),
            sink: AppSink::builder()
                .sync(false)
                .max_buffers(2)
                .enable_last_sample(false)
                .wait_on_eos(false)
                .build(),
        }
    }

    fn pull(
        &self,
        preroll: bool,
        cancelled: Option<&AtomicBool>,
    ) -> Result<Option<gst::Sample>, MediaError> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Relaxed)) {
                return Err(MediaError::Cancelled);
            }
            let wait = gst::ClockTime::from_mseconds(10);
            let sample = if preroll {
                self.sink.try_pull_preroll(wait)
            } else {
                self.sink.try_pull_sample(wait)
            };
            if sample.is_some() {
                return Ok(sample);
            }
            if let Some(bus) = self.pipeline.bus() {
                while let Some(message) = bus.pop() {
                    if let gst::MessageView::Error(error) = message.view() {
                        return Err(MediaError::external(error.error()));
                    }
                }
            }
            if self.sink.is_eos() {
                return Ok(None);
            }
            if Instant::now() >= deadline {
                return Err(MediaError::external("Timed out waiting for a media sample"));
            }
        }
    }
}
