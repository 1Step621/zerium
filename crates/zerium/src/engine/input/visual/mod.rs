//! Visual media: shared frames and decode sessions, with image and video implementations.
use super::{MediaError, MediaReaderRegistry};
use crate::engine::GpuContext;
use std::{
    collections::{BTreeMap, HashMap},
    hash::Hash,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use zerium_core::{
    media::{MediaAsset, MediaKind, MediaPlayback},
    timeline::{EffectInstanceId, ItemId},
};

mod image;
mod video;

pub(crate) use self::video::{
    PixelPlane, VideoFrame, VideoPixels, YuvFormat, YuvMatrix, yuv_format,
};
pub(super) use self::{
    image::{
        IMAGE_READER_ID, ImageReader, ImageSequenceReader, SEQUENCE_READER_ID, SVG_READER_ID,
        SvgMediaReader,
    },
    video::{READER_ID as VIDEO_READER_ID, VideoReader},
};

/// Planning identifies a source by `MediaInputId`; demand traversal attaches
/// consumer affinity as `DecodeStreamKey` before preview/export resolves it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MediaFrameRequest<S = DecodeStreamKey> {
    pub(crate) stream: S,
    pub(crate) asset: MediaAsset,
    pub(crate) presentation_time: Duration,
    pub(crate) size: RasterSize,
}

impl<S> MediaFrameRequest<S> {
    pub(crate) fn new(
        stream: S,
        asset: MediaAsset,
        playback: MediaPlayback,
        local_seconds: f64,
        size: RasterSize,
    ) -> Option<Self> {
        let presentation_time = if asset.kind.is_temporal() {
            playback.sample(local_seconds, asset.duration)?.time
        } else {
            Duration::ZERO
        };
        let size = match asset.kind {
            MediaKind::Video { width, height, .. }
            | MediaKind::ImageSequence { width, height, .. } => RasterSize {
                max_width: width,
                max_height: height,
            },
            _ => size,
        };
        Some(Self {
            stream,
            asset,
            presentation_time,
            size,
        })
    }
}

enum Decoder {
    Video(Box<dyn VideoDecoder>),
    Image(Box<dyn ImageDecoder>),
}

#[derive(Default)]
pub(crate) struct VisualDecodeSession {
    session: Option<(MediaAsset, Decoder)>,
}

impl VisualDecodeSession {
    pub(crate) fn decode(
        &mut self,
        readers: &MediaReaderRegistry,
        request: &MediaFrameRequest,
        gpu: Option<&GpuContext>,
        cancelled: &AtomicBool,
    ) -> Result<DecodedFrame, MediaError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(MediaError::Cancelled);
        }
        if self
            .session
            .as_ref()
            .is_none_or(|(asset, _)| *asset != request.asset)
        {
            self.session = None;
            let asset = &request.asset;
            let reader = readers.reader_for(&asset.reader_id)?;
            let decoder = match asset.kind {
                MediaKind::Video { .. } | MediaKind::ImageSequence { .. } => {
                    Decoder::Video(reader.open_video_decoder(asset, gpu)?)
                }
                MediaKind::Image { .. } => Decoder::Image(reader.open_image_decoder(asset)?),
                MediaKind::Audio { .. } => {
                    return Err(MediaError::unsupported(
                        "Cannot create a video decoder from audio media",
                    ));
                }
            };
            self.session = Some((asset.clone(), decoder));
        }
        let (asset, decoder) = self.session.as_mut().expect("decoder was opened above");
        match decoder {
            Decoder::Video(decoder) => decoder.decode_at(request.presentation_time, cancelled),
            Decoder::Image(decoder) => Ok(DecodedFrame {
                presentation_time: Duration::ZERO,
                duration: asset.duration,
                frame: decoder.render(request.size, cancelled)?,
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MediaInputId {
    pub(crate) item_id: ItemId,
    pub(crate) effect_id: Option<EffectInstanceId>,
    pub(crate) input_id: String,
}

/// Decoder affinity belongs to a consumer clock, not to the source's identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DecodeStreamKey {
    pub(crate) input: MediaInputId,
    pub(crate) branch: Vec<SamplingBranch>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SamplingBranch {
    pub(crate) path: Vec<ItemId>,
    pub(crate) effect: EffectInstanceId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RasterSize {
    pub max_width: u32,
    pub max_height: u32,
}

impl RasterSize {
    pub(super) fn fit(self, (source_width, source_height): (u32, u32)) -> Option<(u32, u32)> {
        if source_width == 0 || source_height == 0 || self.max_width == 0 || self.max_height == 0 {
            return None;
        }
        let scale = (f64::from(self.max_width) / f64::from(source_width))
            .min(f64::from(self.max_height) / f64::from(source_height))
            .min(1.);
        Some((
            (f64::from(source_width) * scale).round().max(1.) as u32,
            (f64::from(source_height) * scale).round().max(1.) as u32,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DecodedFrame {
    pub presentation_time: Duration,
    pub duration: Duration,
    /// Preserve the decoded image's identity through selection, preview and export.
    pub frame: Arc<VisualFrame>,
}

impl DecodedFrame {
    pub(crate) fn covers(&self, time: Duration) -> bool {
        let start = self.presentation_time;
        let duration = self.duration;
        if duration.is_zero() {
            return time == start;
        }
        let rounded = time.saturating_add(Duration::from_nanos(1));
        // Allow a rounded source time just below the interval's start without
        // excluding its final nanosecond (used by EOF frame holding).
        start <= rounded && time < start.saturating_add(duration)
    }
}

pub(crate) trait VideoDecoder: Send {
    /// Returns the frame whose presentation interval contains `presentation_time`,
    /// or the closest following frame when the stream has a timestamp gap.
    fn decode_at(
        &mut self,
        presentation_time: Duration,
        cancelled: &AtomicBool,
    ) -> Result<DecodedFrame, MediaError>;
}

pub(crate) trait ImageDecoder: Send {
    fn render(
        &mut self,
        size: RasterSize,
        cancelled: &AtomicBool,
    ) -> Result<Arc<VisualFrame>, MediaError>;
}

/// CPU pixels or GPU-decoded textures. Video stays in YUV or on the GPU
/// until upload; images and text use display-encoded, straight-alpha RGBA8.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VisualFrame {
    pub width: u32,
    pub height: u32,
    pub data: FrameData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FrameData {
    Rgba(Arc<[u8]>),
    Video(Arc<VideoFrame>),
}

impl VisualFrame {
    pub(crate) fn rgba(width: u32, height: u32, rgba: impl Into<Arc<[u8]>>) -> Self {
        Self {
            width,
            height,
            data: FrameData::Rgba(rgba.into()),
        }
    }

    pub(crate) fn byte_len(&self) -> usize {
        match &self.data {
            FrameData::Rgba(bytes) => bytes.len(),
            FrameData::Video(frame) => frame.byte_len(),
        }
    }
}

pub(crate) const RASTER_CACHE_BYTES: usize = 128 * 1024 * 1024;

struct CachedFrame {
    frame: Arc<VisualFrame>,
    last_used: u64,
}

pub(crate) struct FrameCache<K> {
    frames: HashMap<Arc<K>, CachedFrame>,
    recency: BTreeMap<u64, Arc<K>>,
    byte_budget: usize,
    resident_bytes: usize,
    clock: u64,
}

impl<K: Eq + Hash> FrameCache<K> {
    pub(crate) fn new(byte_budget: usize) -> Self {
        Self {
            frames: HashMap::new(),
            recency: BTreeMap::new(),
            byte_budget,
            resident_bytes: 0,
            clock: 0,
        }
    }

    pub(crate) fn byte_budget(&self) -> usize {
        self.byte_budget
    }

    fn next_tick(&mut self) -> u64 {
        if self.clock == u64::MAX {
            self.recency = std::mem::take(&mut self.recency)
                .into_values()
                .enumerate()
                .map(|(index, key)| {
                    let tick = index as u64;
                    self.frames.get_mut(&key).expect("cached frame").last_used = tick;
                    (tick, key)
                })
                .collect();
            self.clock = self.recency.len() as u64;
        }
        self.clock += 1;
        self.clock
    }

    pub(crate) fn get(&mut self, key: &K) -> Option<Arc<VisualFrame>> {
        let tick = self.next_tick();
        let cached = self.frames.get_mut(key)?;
        let key = self
            .recency
            .remove(&cached.last_used)
            .expect("cached recency");
        cached.last_used = tick;
        self.recency.insert(tick, key);
        Some(cached.frame.clone())
    }

    fn make_room(&mut self, bytes: usize) -> bool {
        let Some(available) = self.byte_budget.checked_sub(bytes) else {
            return false;
        };
        let mut remaining = self.resident_bytes;
        let mut evictions = Vec::new();
        // A live graph pins its exact pixel version, not all versions from the
        // same source. Never discard that identity to admit an unrelated raster.
        for (&tick, key) in &self.recency {
            if remaining <= available {
                break;
            }
            let cached = &self.frames[key];
            if Arc::strong_count(&cached.frame) == 1 {
                remaining -= cached.frame.byte_len();
                evictions.push(tick);
            }
        }
        for tick in evictions {
            let key = self.recency.remove(&tick).expect("cached recency");
            self.frames.remove(&key);
        }
        self.resident_bytes = remaining;
        remaining <= available
    }

    pub(crate) fn insert(&mut self, key: K, frame: Arc<VisualFrame>) {
        if let Some(previous) = self.frames.remove(&key) {
            self.recency.remove(&previous.last_used);
            self.resident_bytes -= previous.frame.byte_len();
        }
        let bytes = frame.byte_len();
        // A result larger than the budget, or competing with pinned results,
        // remains usable by its caller without expanding cache retention.
        if !self.make_room(bytes) {
            return;
        }
        let tick = self.next_tick();
        let key = Arc::new(key);
        self.frames.insert(
            key.clone(),
            CachedFrame {
                frame,
                last_used: tick,
            },
        );
        self.recency.insert(tick, key);
        self.resident_bytes += bytes;
    }
}
