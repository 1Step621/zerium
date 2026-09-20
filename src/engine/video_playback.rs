use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::{
    domain::{
        media::{MediaAsset, MediaKind, MediaSourceId},
        timeline::{Frame, FrameRate, ItemId, LayerId, TimelineItem, TimelineTime},
    },
    engine::{
        cache::{BudgetedTimestampCache, TimestampCacheHit},
        frame::RgbaFrame,
        media::{
            DecodedVideoFrame, MediaError, MediaReaderRegistry, VideoDecodeSize,
            VideoDecoderSession, VideoProxy, VideoProxyRequest, estimate_max_keyframe_gap,
        },
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VideoPlaybackMode {
    Idle,
    Scrubbing,
    Playing,
}

struct InFlightVideoDecode {
    generation: u64,
    cancel: Arc<AtomicBool>,
    source: MediaAsset,
    presentation_time: Duration,
    size: VideoDecodeSize,
    mode: VideoPlaybackMode,
    expected_end: Duration,
}

impl InFlightVideoDecode {
    fn retain_for_requests(&self, requests: &[RequestedVideoFrame], mode: VideoPlaybackMode) {
        if !requests.iter().any(|request| self.serves(request, mode)) {
            self.cancel.store(true, Ordering::Release);
        }
    }

    fn serves(&self, request: &RequestedVideoFrame, mode: VideoPlaybackMode) -> bool {
        if self.cancel.load(Ordering::Acquire)
            || self.source != request.source
            || self.size != request.size
        {
            return false;
        }
        let in_range = request.presentation_time >= self.presentation_time
            && request.presentation_time < self.expected_end;
        match mode {
            VideoPlaybackMode::Playing => self.mode == mode && in_range,
            VideoPlaybackMode::Scrubbing | VideoPlaybackMode::Idle => {
                request.presentation_time == self.presentation_time || in_range
            }
        }
    }
}

#[derive(Default)]
struct InFlightVideoDecodes {
    active: HashMap<VideoInputId, InFlightVideoDecode>,
    next_generation: u64,
}

impl InFlightVideoDecodes {
    fn get(&self, input: &VideoInputId) -> Option<&InFlightVideoDecode> {
        self.active.get(input)
    }

    fn spawn(
        &mut self,
        input: VideoInputId,
        source: MediaAsset,
        presentation_time: Duration,
        size: VideoDecodeSize,
        mode: VideoPlaybackMode,
        frame_count: usize,
    ) -> (u64, Arc<AtomicBool>) {
        if let Some(active) = self.active.get(&input) {
            active.cancel.store(true, Ordering::Release);
        }
        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = self.next_generation;
        let cancel = Arc::new(AtomicBool::new(false));
        // This is the horizon of this memory-bounded batch in the decoded
        // source's native cadence (including a proxy's cadence), not timeline fps.
        // Actual cache coverage always comes from decoded PTS and durations.
        let expected_end = match source.kind {
            MediaKind::Video { frame_rate, .. } => {
                presentation_time.saturating_add(Duration::from_secs_f64(
                    frame_rate.frame_to_seconds(frame_count.saturating_sub(1).max(1) as u64),
                ))
            }
            _ => source
                .duration
                .max(presentation_time.saturating_add(Duration::from_nanos(1))),
        };
        self.active.insert(
            input,
            InFlightVideoDecode {
                generation,
                cancel: cancel.clone(),
                source,
                presentation_time,
                size,
                mode,
                expected_end,
            },
        );
        (generation, cancel)
    }

    fn complete(&mut self, input: &VideoInputId, generation: u64) -> bool {
        if self
            .active
            .get(input)
            .is_some_and(|active| active.generation == generation)
        {
            self.active.remove(input);
            true
        } else {
            false
        }
    }

    fn remove(&mut self, input: &VideoInputId) {
        self.active.remove(input);
    }

    fn cancel_orphans(&mut self, active_inputs: &HashSet<VideoInputId>) {
        for (input, decode) in &self.active {
            if !active_inputs.contains(input) {
                // Do not release the slot until the worker returns.
                decode.cancel.store(true, Ordering::Release);
            }
        }
    }

    fn reset(&mut self) {
        self.cancel_orphans(&HashSet::new());
        self.active.clear();
    }
}

#[derive(Clone)]
struct VideoDecodeRequest {
    input: VideoInputId,
    asset_time: Duration,
    asset: MediaAsset,
}

impl VideoDecodeRequest {
    fn presentation_time_in(&self, source_start: Duration) -> Duration {
        self.asset_time.saturating_sub(source_start)
    }

    fn proxy_key(&self, chunk_seconds: u64) -> VideoProxyKey {
        VideoProxyKey {
            source: self.asset.source_id(),
            chunk: self.asset_time.as_secs() / chunk_seconds,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct VideoProxyKey {
    source: MediaSourceId,
    chunk: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct VideoFrameSequence {
    original_source: MediaSourceId,
    decoded_source: MediaSourceId,
    source_start: Duration,
    size: VideoDecodeSize,
}

impl VideoFrameSequence {
    fn new(
        original: &MediaAsset,
        decoded: &MediaAsset,
        source_start: Duration,
        size: VideoDecodeSize,
    ) -> Self {
        Self {
            original_source: original.source_id(),
            decoded_source: decoded.source_id(),
            source_start,
            size,
        }
    }
}

#[derive(Clone)]
struct VideoProxyJob {
    key: VideoProxyKey,
    source: MediaAsset,
    request: VideoProxyRequest,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct VideoInputId {
    pub(crate) item_id: ItemId,
    pub(crate) input_id: String,
}

#[derive(Clone)]
pub(crate) struct RequestedVideoFrame {
    presentation_time: Duration,
    asset: MediaAsset,
    source: MediaAsset,
    source_start: Duration,
    size: VideoDecodeSize,
}

impl RequestedVideoFrame {
    fn sequence(&self) -> VideoFrameSequence {
        VideoFrameSequence::new(&self.asset, &self.source, self.source_start, self.size)
    }
}

struct PresentedVideoFrame {
    asset: MediaAsset,
    source: MediaAsset,
    source_start: Duration,
    presentation_time: Duration,
    duration: Duration,
    frame: Arc<RgbaFrame>,
}

struct VideoDecoderState {
    source: MediaAsset,
    decoder: Box<dyn VideoDecoderSession>,
}

struct VideoWorkerRequest {
    generation: u64,
    sequence: VideoFrameSequence,
    source: MediaAsset,
    presentation_time: Duration,
    size: VideoDecodeSize,
    frame_count: usize,
    cancel: Arc<AtomicBool>,
}

enum VideoWorkerDirective {
    Decode(Box<VideoWorkerRequest>),
    Shutdown,
}

struct VideoWorkerResult {
    generation: u64,
    sequence: VideoFrameSequence,
    source: MediaAsset,
    presentation_time: Duration,
    size: VideoDecodeSize,
    result: Result<Vec<DecodedVideoFrame>, MediaError>,
}

struct VideoWorkerHandle {
    requests: std::sync::mpsc::Sender<VideoWorkerDirective>,
}

pub(crate) struct VideoPlaybackEvent(VideoPlaybackEventKind);

enum VideoPlaybackEventKind {
    Decoded {
        input: VideoInputId,
        result: VideoWorkerResult,
    },
    ProxyGenerated {
        generation: u64,
        key: VideoProxyKey,
        result: Result<VideoProxy, MediaError>,
    },
}

fn video_worker_main(
    media_readers: Arc<MediaReaderRegistry>,
    requests: std::sync::mpsc::Receiver<VideoWorkerDirective>,
    input: VideoInputId,
    events: UnboundedSender<VideoPlaybackEvent>,
) {
    let mut decoder: Option<VideoDecoderState> = None;
    while let Ok(first) = requests.recv() {
        let mut directive = first;
        while let Ok(next) = requests.try_recv() {
            directive = next;
        }
        let request = match directive {
            VideoWorkerDirective::Shutdown => break,
            VideoWorkerDirective::Decode(request) => request,
        };
        if request.cancel.load(Ordering::Acquire) {
            let _ = events.unbounded_send(VideoPlaybackEvent(VideoPlaybackEventKind::Decoded {
                input: input.clone(),
                result: VideoWorkerResult {
                    generation: request.generation,
                    sequence: request.sequence,
                    source: request.source,
                    presentation_time: request.presentation_time,
                    size: request.size,
                    result: Err(MediaError::Cancelled),
                },
            }));
            continue;
        }
        if decoder
            .as_ref()
            .is_none_or(|decoder| decoder.source != request.source)
        {
            decoder = match media_readers.open_video_decoder(&request.source) {
                Ok(decoder) => Some(VideoDecoderState {
                    source: request.source.clone(),
                    decoder,
                }),
                Err(error) => {
                    let _ = events.unbounded_send(VideoPlaybackEvent(
                        VideoPlaybackEventKind::Decoded {
                            input: input.clone(),
                            result: VideoWorkerResult {
                                generation: request.generation,
                                sequence: request.sequence,
                                source: request.source,
                                presentation_time: request.presentation_time,
                                size: request.size,
                                result: Err(error),
                            },
                        },
                    ));
                    continue;
                }
            };
        }
        let result = decoder
            .as_mut()
            .expect("decoder was opened above")
            .decoder
            .decode_from(
                request.presentation_time,
                request.frame_count,
                request.size,
                &request.cancel,
            );
        if events
            .unbounded_send(VideoPlaybackEvent(VideoPlaybackEventKind::Decoded {
                input: input.clone(),
                result: VideoWorkerResult {
                    generation: request.generation,
                    sequence: request.sequence,
                    source: request.source,
                    presentation_time: request.presentation_time,
                    size: request.size,
                    result,
                },
            }))
            .is_err()
        {
            break;
        }
    }
}

struct InFlightVideoProxy {
    generation: u64,
    job: VideoProxyJob,
}

struct VideoProxyManager {
    media_readers: Arc<MediaReaderRegistry>,
    proxies: HashMap<VideoProxyKey, VideoProxy>,
    queued: VecDeque<VideoProxyJob>,
    generating: Option<InFlightVideoProxy>,
    next_generation: u64,
    failed_attempts: HashMap<VideoProxyKey, u8>,
    keyframe_gaps: HashMap<MediaSourceId, Option<u64>>,
    error: Option<String>,
    notifications: Vec<String>,
}

impl VideoProxyManager {
    const MAX_WIDTH: u32 = 960;
    const MAX_HEIGHT: u32 = 540;
    const MAX_FRAMES_PER_SECOND: u32 = 60;
    const CHUNK_SECONDS: u64 = 10;
    const MAX_KEYFRAME_GAP: u64 = 24;
    const MAX_ATTEMPTS: u8 = 3;

    fn new(media_readers: Arc<MediaReaderRegistry>) -> Self {
        Self {
            media_readers,
            proxies: HashMap::new(),
            queued: VecDeque::new(),
            generating: None,
            next_generation: 0,
            failed_attempts: HashMap::new(),
            keyframe_gaps: HashMap::new(),
            error: None,
            notifications: Vec::new(),
        }
    }

    fn proxy_for_asset_time(
        &self,
        asset: &MediaAsset,
        asset_time: Duration,
    ) -> Option<(MediaAsset, Duration)> {
        let key = VideoProxyKey {
            source: asset.source_id(),
            chunk: asset_time.as_secs() / Self::CHUNK_SECONDS,
        };
        self.proxies
            .get(&key)
            .map(|proxy| (proxy.asset.clone(), proxy.source_start))
    }

    fn interactive_source(&self, request: &VideoDecodeRequest) -> (MediaAsset, Duration) {
        self.proxies
            .get(&request.proxy_key(Self::CHUNK_SECONDS))
            .map_or_else(
                || (request.asset.clone(), Duration::ZERO),
                |proxy| (proxy.asset.clone(), proxy.source_start),
            )
    }

    fn max_keyframe_gap(&mut self, asset: &MediaAsset) -> Option<u64> {
        if let Some(cached) = self.keyframe_gaps.get(&asset.source_id()) {
            return *cached;
        }
        let gap = estimate_max_keyframe_gap(&asset.path);
        self.keyframe_gaps.insert(asset.source_id(), gap);
        gap
    }

    fn proxy_needed(&mut self, asset: &MediaAsset) -> bool {
        let MediaKind::Video {
            width,
            height,
            frame_rate,
            ..
        } = asset.kind
        else {
            return false;
        };
        let exceeds_frame_rate = u64::from(frame_rate.numerator())
            > u64::from(Self::MAX_FRAMES_PER_SECOND) * u64::from(frame_rate.denominator());
        if width > Self::MAX_WIDTH || height > Self::MAX_HEIGHT || exceeds_frame_rate {
            return true;
        }
        self.max_keyframe_gap(asset)
            .is_some_and(|gap| gap > Self::MAX_KEYFRAME_GAP)
    }

    fn proxy_job(&mut self, request: &VideoDecodeRequest, chunk: u64) -> Option<VideoProxyJob> {
        if !self.proxy_needed(&request.asset) {
            return None;
        }
        let start_seconds = chunk.checked_mul(Self::CHUNK_SECONDS)?;
        let source_start = Duration::from_secs(start_seconds);
        if source_start >= request.asset.duration {
            return None;
        }
        Some(VideoProxyJob {
            key: VideoProxyKey {
                source: request.asset.source_id(),
                chunk,
            },
            source: request.asset.clone(),
            request: VideoProxyRequest {
                max_width: Self::MAX_WIDTH,
                max_height: Self::MAX_HEIGHT,
                max_frames_per_second: Self::MAX_FRAMES_PER_SECOND,
                source_start,
                source_duration: Duration::from_secs(Self::CHUNK_SECONDS),
            },
        })
    }

    fn update_queue(&mut self, requests: &[VideoDecodeRequest]) {
        let mut desired = Vec::new();
        for offset in 0..=1 {
            for request in requests {
                let chunk = request
                    .proxy_key(Self::CHUNK_SECONDS)
                    .chunk
                    .saturating_add(offset);
                if let Some(job) = self.proxy_job(request, chunk) {
                    desired.push(job);
                }
            }
        }
        let desired_keys = desired
            .iter()
            .map(|job| job.key.clone())
            .collect::<HashSet<_>>();
        self.queued.retain(|job| desired_keys.contains(&job.key));
        for job in desired {
            if self.proxies.contains_key(&job.key)
                || self
                    .failed_attempts
                    .get(&job.key)
                    .is_some_and(|attempts| *attempts >= Self::MAX_ATTEMPTS)
                || self
                    .generating
                    .as_ref()
                    .is_some_and(|current| current.job.key == job.key)
                || self.queued.iter().any(|queued| queued.key == job.key)
            {
                continue;
            }
            self.queued.push_back(job);
        }
    }

    fn start_next(&mut self, events: &UnboundedSender<VideoPlaybackEvent>) {
        if self.generating.is_some() {
            return;
        }
        while let Some(job) = self.queued.pop_front() {
            self.next_generation = self.next_generation.wrapping_add(1);
            let generation = self.next_generation;
            let key = job.key.clone();
            let failure_key = key.clone();
            let media_readers = self.media_readers.clone();
            let worker_job = job.clone();
            let events = events.clone();
            self.generating = Some(InFlightVideoProxy { generation, job });
            let spawn = std::thread::Builder::new()
                .name("zerium-video-proxy".to_owned())
                .spawn(move || {
                    let result =
                        media_readers.create_video_proxy(&worker_job.source, worker_job.request);
                    let _ = events.unbounded_send(VideoPlaybackEvent(
                        VideoPlaybackEventKind::ProxyGenerated {
                            generation,
                            key,
                            result,
                        },
                    ));
                });
            if spawn.is_ok() {
                return;
            }
            self.generating = None;
            self.record_result(
                failure_key,
                Err(MediaError::External(
                    "動画proxy workerを開始できませんでした".to_owned(),
                )),
            );
        }
    }

    fn finish_current(
        &mut self,
        generation: u64,
        key: VideoProxyKey,
        result: Result<VideoProxy, MediaError>,
        events: &UnboundedSender<VideoPlaybackEvent>,
    ) -> bool {
        let Some(generating) = &self.generating else {
            return false;
        };
        if generating.generation != generation || generating.job.key != key {
            return false;
        }
        self.generating = None;
        self.record_result(key, result);
        self.start_next(events);
        true
    }

    fn record_result(&mut self, key: VideoProxyKey, result: Result<VideoProxy, MediaError>) {
        match result {
            Ok(proxy) => {
                self.failed_attempts.remove(&key);
                self.proxies.insert(key, proxy);
            }
            Err(error) => {
                let attempts = self.failed_attempts.entry(key).or_insert(0);
                *attempts = attempts.saturating_add(1);
                let message = format!(
                    "動画proxyの生成に失敗しました ({}/{}): {error}",
                    *attempts,
                    Self::MAX_ATTEMPTS,
                );
                self.error = Some(message.clone());
                self.notifications.push(message);
            }
        }
    }

    fn reset(&mut self) {
        self.proxies.clear();
        self.queued.clear();
        self.generating = None;
        self.failed_attempts.clear();
        self.keyframe_gaps.clear();
        self.error = None;
        self.notifications.clear();
    }
}

pub(crate) struct VideoPlaybackSnapshot {
    pub(crate) revision: u64,
    pub(crate) error: Option<String>,
    pub(crate) notifications: Vec<String>,
}

pub(crate) struct VideoPlaybackEngine {
    media_readers: Arc<MediaReaderRegistry>,
    frame_cache: BudgetedTimestampCache<VideoFrameSequence, Arc<RgbaFrame>>,
    requested_frames: HashMap<VideoInputId, Vec<RequestedVideoFrame>>,
    tick_decode_requests: Vec<VideoDecodeRequest>,
    tick_seen_times: HashSet<u64>,
    last_presented_frames: HashMap<VideoInputId, PresentedVideoFrame>,
    in_flight: InFlightVideoDecodes,
    workers: HashMap<VideoInputId, VideoWorkerHandle>,
    proxy: VideoProxyManager,
    events: UnboundedSender<VideoPlaybackEvent>,
    failed_frames: HashSet<(VideoFrameSequence, Duration)>,
    notifications: Vec<String>,
    decode_mode: VideoPlaybackMode,
    revision: u64,
    error: Option<String>,
}

impl VideoPlaybackEngine {
    // Interactive seeks decode only the requested frame. During playback,
    // bounded batches keep frames ahead of the playhead without making a seek
    // wait for a large obsolete batch or allocating hundreds of MiB at once.
    const PREFETCH_BATCH_FRAMES: u64 = 24;

    const MAX_DECODE_BATCH_BYTES: u64 = 64 * 1024 * 1024;
    const FRAME_CACHE_BUDGET_BYTES: usize = 512 * 1024 * 1024;
    pub(crate) fn new(
        media_readers: Arc<MediaReaderRegistry>,
    ) -> (Self, UnboundedReceiver<VideoPlaybackEvent>) {
        let (events, event_receiver) = unbounded();
        (
            Self {
                media_readers: media_readers.clone(),
                frame_cache: BudgetedTimestampCache::new(Self::FRAME_CACHE_BUDGET_BYTES),
                requested_frames: HashMap::new(),
                tick_decode_requests: Vec::new(),
                tick_seen_times: HashSet::new(),
                last_presented_frames: HashMap::new(),
                in_flight: InFlightVideoDecodes::default(),
                workers: HashMap::new(),
                proxy: VideoProxyManager::new(media_readers),
                events,
                failed_frames: HashSet::new(),
                notifications: Vec::new(),
                decode_mode: VideoPlaybackMode::Idle,
                revision: 0,
                error: None,
            },
            event_receiver,
        )
    }

    pub(crate) fn begin_frame_demand(&mut self, mode: VideoPlaybackMode) {
        self.decode_mode = mode;
        self.requested_frames.clear();
        self.tick_decode_requests.clear();
        self.tick_seen_times.clear();
    }

    pub(crate) fn record_media_requests(
        &mut self,
        time: TimelineTime,
        active_items: &[(LayerId, TimelineItem)],
        frame_rate: FrameRate,
        size: VideoDecodeSize,
    ) -> Vec<(VideoInputId, RequestedVideoFrame)> {
        if !self.tick_seen_times.insert(time.frames().to_bits()) {
            return Vec::new();
        }
        let playback_seconds = Some(time.seconds(frame_rate));
        let mut recorded = Vec::new();
        for request in self.current_requests(
            active_items,
            time.nearest_frame(),
            frame_rate,
            playback_seconds,
        ) {
            let (source, source_start) = match self.decode_mode {
                VideoPlaybackMode::Idle => self.idle_source(&request, size),
                VideoPlaybackMode::Playing | VideoPlaybackMode::Scrubbing => {
                    self.proxy.interactive_source(&request)
                }
            };
            let presentation_time = request.presentation_time_in(source_start);
            self.tick_decode_requests.push(request.clone());
            let requested = RequestedVideoFrame {
                presentation_time,
                asset: request.asset.clone(),
                source: source.clone(),
                source_start,
                size,
            };
            self.requested_frames
                .entry(request.input.clone())
                .or_default()
                .push(requested.clone());
            recorded.push((request.input, requested));
        }
        recorded
    }

    fn nearest_fallback_with_proxy(
        &mut self,
        request: &RequestedVideoFrame,
    ) -> Option<(MediaAsset, Duration, TimestampCacheHit<Arc<RgbaFrame>>)> {
        let requested_asset_time = request
            .presentation_time
            .saturating_add(request.source_start);
        let mut candidates = Vec::with_capacity(4);
        let sequence = request.sequence();
        candidates.push(
            self.frame_cache
                .get_nearest_at_or_before(&sequence, request.presentation_time)
                .map(|hit| (request.source.clone(), request.source_start, hit)),
        );
        candidates.push(
            self.frame_cache
                .get_nearest_at_or_after(&sequence, request.presentation_time)
                .map(|hit| (request.source.clone(), request.source_start, hit)),
        );
        if self.decode_mode == VideoPlaybackMode::Idle
            && request.source == request.asset
            && let Some((proxy_source, proxy_start)) = self
                .proxy
                .proxy_for_asset_time(&request.asset, requested_asset_time)
            && proxy_source != request.source
        {
            let proxy_sequence =
                VideoFrameSequence::new(&request.asset, &proxy_source, proxy_start, request.size);
            let proxy_presentation = requested_asset_time.saturating_sub(proxy_start);
            candidates.push(
                self.frame_cache
                    .get_nearest_at_or_before(&proxy_sequence, proxy_presentation)
                    .map(|hit| (proxy_source.clone(), proxy_start, hit)),
            );
            candidates.push(
                self.frame_cache
                    .get_nearest_at_or_after(&proxy_sequence, proxy_presentation)
                    .map(|hit| (proxy_source, proxy_start, hit)),
            );
        }
        candidates
            .into_iter()
            .flatten()
            .min_by_key(|(_, fallback_start, hit)| {
                let hit_asset_time = hit.presentation_time.saturating_add(*fallback_start);
                let distance = hit_asset_time.abs_diff(requested_asset_time);
                let past_rank = u8::from(hit_asset_time > requested_asset_time);
                (distance, past_rank)
            })
    }

    fn idle_proxy_exact(
        &mut self,
        request: &RequestedVideoFrame,
    ) -> Option<(MediaAsset, Duration, TimestampCacheHit<Arc<RgbaFrame>>)> {
        if self.decode_mode != VideoPlaybackMode::Idle || request.source != request.asset {
            return None;
        }
        let requested_asset_time = request
            .presentation_time
            .saturating_add(request.source_start);
        let (proxy_source, proxy_start) = self
            .proxy
            .proxy_for_asset_time(&request.asset, requested_asset_time)?;
        if proxy_source == request.source {
            return None;
        }
        let proxy_sequence =
            VideoFrameSequence::new(&request.asset, &proxy_source, proxy_start, request.size);
        let proxy_presentation = requested_asset_time.saturating_sub(proxy_start);
        self.frame_cache
            .get_for_time(&proxy_sequence, proxy_presentation)
            .map(|hit| (proxy_source, proxy_start, hit))
    }

    pub(crate) fn present_recorded_frame(
        &mut self,
        input: &VideoInputId,
        request: &RequestedVideoFrame,
    ) -> Option<Arc<RgbaFrame>> {
        let sequence = request.sequence();
        if let Some(cached) = self
            .frame_cache
            .get_for_time(&sequence, request.presentation_time)
        {
            let presentation_changed =
                self.last_presented_frames
                    .get(input)
                    .is_none_or(|presented| {
                        presented.asset != request.asset
                            || presented.source != request.source
                            || presented.presentation_time != cached.presentation_time
                            || presented.duration != cached.duration
                            || !Arc::ptr_eq(&presented.frame, &cached.value)
                    });
            let frame = cached.value.clone();
            self.last_presented_frames.insert(
                input.clone(),
                PresentedVideoFrame {
                    asset: request.asset.clone(),
                    source: request.source.clone(),
                    source_start: request.source_start,
                    presentation_time: cached.presentation_time,
                    duration: cached.duration,
                    frame: cached.value,
                },
            );
            if presentation_changed {
                self.revision = self.revision.saturating_add(1);
            }
            return Some(frame);
        } else if let Some((proxy_source, proxy_start, proxy_hit)) = self.idle_proxy_exact(request)
        {
            let presentation_changed =
                self.last_presented_frames
                    .get(input)
                    .is_none_or(|presented| {
                        presented.asset != request.asset
                            || presented.source != proxy_source
                            || presented.presentation_time != proxy_hit.presentation_time
                            || presented.duration != proxy_hit.duration
                            || !Arc::ptr_eq(&presented.frame, &proxy_hit.value)
                    });
            let frame = proxy_hit.value.clone();
            self.last_presented_frames.insert(
                input.clone(),
                PresentedVideoFrame {
                    asset: request.asset.clone(),
                    source: proxy_source,
                    source_start: proxy_start,
                    presentation_time: proxy_hit.presentation_time,
                    duration: proxy_hit.duration,
                    frame: proxy_hit.value,
                },
            );
            if presentation_changed {
                self.revision = self.revision.saturating_add(1);
            }
            return Some(frame);
        } else if let Some((fallback_source, fallback_start, nearby)) =
            self.nearest_fallback_with_proxy(request)
        {
            let requested_asset_time = request
                .presentation_time
                .saturating_add(request.source_start);
            let nearby_asset_time = nearby.presentation_time.saturating_add(fallback_start);
            let presentation_changed =
                self.last_presented_frames
                    .get(input)
                    .is_none_or(|presented| {
                        presented.asset != request.asset
                            || presented.source != fallback_source
                            || !Arc::ptr_eq(&presented.frame, &nearby.value)
                            || nearby_asset_time.abs_diff(requested_asset_time)
                                < presented
                                    .presentation_time
                                    .saturating_add(presented.source_start)
                                    .abs_diff(requested_asset_time)
                    });
            let frame = nearby.value.clone();
            self.last_presented_frames.insert(
                input.clone(),
                PresentedVideoFrame {
                    asset: request.asset.clone(),
                    source: fallback_source,
                    source_start: fallback_start,
                    presentation_time: nearby.presentation_time,
                    duration: nearby.duration,
                    frame: nearby.value,
                },
            );
            if presentation_changed {
                self.revision = self.revision.saturating_add(1);
            }
            return Some(frame);
        } else if let Some(presented) = self
            .last_presented_frames
            .get(input)
            .filter(|presented| presented.asset == request.asset)
        {
            return Some(presented.frame.clone());
        }
        None
    }

    fn current_requests(
        &self,
        active_items: &[(LayerId, TimelineItem)],
        playhead: Frame,
        timeline_rate: FrameRate,
        playback_seconds: Option<f64>,
    ) -> Vec<VideoDecodeRequest> {
        active_items
            .iter()
            .filter_map(|(_, item)| {
                let schema = item.schema()?;
                if !schema.is_media() {
                    return Some(Vec::new());
                }
                let local_frame = Frame::new(playhead.get().saturating_sub(item.start.get()));
                let local_seconds = if let Some(playback_seconds) = playback_seconds {
                    let item_start_seconds = timeline_rate.frame_to_seconds(item.start);
                    playback_seconds - item_start_seconds
                } else {
                    timeline_rate.frame_to_seconds(local_frame)
                };
                Some(
                    item.assets
                        .iter()
                        .filter_map(|(input_id, asset)| {
                            match asset.kind {
                                MediaKind::Video { .. } | MediaKind::Image { .. } => {}
                                MediaKind::Audio { .. } => return None,
                            }
                            let asset_time =
                                Duration::try_from_secs_f64(asset.looped_seconds(local_seconds))
                                    .unwrap_or_default();
                            Some(VideoDecodeRequest {
                                input: VideoInputId {
                                    item_id: item.id,
                                    input_id: input_id.clone(),
                                },
                                asset_time,
                                asset: asset.clone(),
                            })
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .flatten()
            .collect()
    }

    fn idle_source(
        &self,
        request: &VideoDecodeRequest,
        size: VideoDecodeSize,
    ) -> (MediaAsset, Duration) {
        let proxy = self
            .proxy
            .proxies
            .get(&request.proxy_key(VideoProxyManager::CHUNK_SECONDS));
        let Some(proxy) = proxy else {
            return (request.asset.clone(), Duration::ZERO);
        };
        let original_sequence =
            VideoFrameSequence::new(&request.asset, &request.asset, Duration::ZERO, size);
        if self
            .frame_cache
            .contains_time(&original_sequence, request.asset_time)
        {
            return (request.asset.clone(), Duration::ZERO);
        }
        let proxy_presentation = request.asset_time.saturating_sub(proxy.source_start);
        let proxy_sequence =
            VideoFrameSequence::new(&request.asset, &proxy.asset, proxy.source_start, size);
        if self
            .frame_cache
            .contains_time(&proxy_sequence, proxy_presentation)
            || self
                .failed_frames
                .contains(&(proxy_sequence, proxy_presentation))
        {
            return (request.asset.clone(), Duration::ZERO);
        }
        (proxy.asset.clone(), proxy.source_start)
    }

    fn protected_positions(&self) -> Vec<(VideoFrameSequence, Duration)> {
        let mut protected = Vec::new();
        for request in self.requested_frames.values().flatten() {
            protected.push((request.sequence(), request.presentation_time));
            if self.decode_mode != VideoPlaybackMode::Idle || request.source != request.asset {
                continue;
            }
            let asset_time = request
                .presentation_time
                .saturating_add(request.source_start);
            let Some((proxy_source, proxy_start)) =
                self.proxy.proxy_for_asset_time(&request.asset, asset_time)
            else {
                continue;
            };
            if proxy_source == request.source {
                continue;
            }
            protected.push((
                VideoFrameSequence::new(&request.asset, &proxy_source, proxy_start, request.size),
                asset_time.saturating_sub(proxy_start),
            ));
        }
        protected
    }

    pub(crate) fn finish_frame_demand(&mut self) -> VideoPlaybackSnapshot {
        let active_inputs = self
            .requested_frames
            .keys()
            .cloned()
            .collect::<HashSet<_>>();
        self.last_presented_frames
            .retain(|input, _| active_inputs.contains(input));
        self.in_flight.cancel_orphans(&active_inputs);
        self.shutdown_idle_workers(&active_inputs);

        let tick_requests = std::mem::take(&mut self.tick_decode_requests);
        self.proxy.update_queue(&tick_requests);
        let protected = self.protected_positions();
        self.failed_frames.retain(|entry| protected.contains(entry));
        for input in active_inputs {
            self.decode_input_if_needed(&input);
        }
        self.proxy.start_next(&self.events);
        self.snapshot()
    }

    pub(crate) fn handle_event(&mut self, event: VideoPlaybackEvent) -> VideoPlaybackSnapshot {
        match event.0 {
            VideoPlaybackEventKind::Decoded { input, result } => {
                if self.finish_decode(&input, result) {
                    self.decode_input_if_needed(&input);
                }
            }
            VideoPlaybackEventKind::ProxyGenerated {
                generation,
                key,
                result,
            } => {
                let events = self.events.clone();
                if self.proxy.finish_current(generation, key, result, &events) {
                    self.revision = self.revision.saturating_add(1);
                }
            }
        }
        self.snapshot()
    }

    fn snapshot(&mut self) -> VideoPlaybackSnapshot {
        let mut notifications = std::mem::take(&mut self.notifications);
        notifications.append(&mut self.proxy.notifications);
        VideoPlaybackSnapshot {
            revision: self.revision,
            error: self.error.clone().or_else(|| self.proxy.error.clone()),
            notifications,
        }
    }

    fn ensure_worker(&mut self, input: &VideoInputId) -> bool {
        if self.workers.contains_key(input) {
            return true;
        }
        let (request_tx, request_rx) = std::sync::mpsc::channel::<VideoWorkerDirective>();
        let media_readers = self.media_readers.clone();
        let worker_input = input.clone();
        let events = self.events.clone();
        if std::thread::Builder::new()
            .name("zerium-video-worker".to_owned())
            .spawn(move || video_worker_main(media_readers, request_rx, worker_input, events))
            .is_err()
        {
            return false;
        }
        self.workers.insert(
            input.clone(),
            VideoWorkerHandle {
                requests: request_tx,
            },
        );
        true
    }

    fn shutdown_idle_workers(&mut self, active_inputs: &HashSet<VideoInputId>) {
        self.workers.retain(|input, worker| {
            if active_inputs.contains(input) {
                return true;
            }
            let _ = worker.requests.send(VideoWorkerDirective::Shutdown);
            self.in_flight.remove(input);
            false
        });
    }

    fn decode_input_if_needed(&mut self, input: &VideoInputId) {
        let Some(requests) = self.requested_frames.get(input) else {
            return;
        };
        if let Some(active) = self.in_flight.get(input) {
            active.retain_for_requests(requests, self.decode_mode);
        }
        let Some(requested) = requests
            .iter()
            .find(|request| {
                let sequence = request.sequence();
                !self
                    .frame_cache
                    .contains_time(&sequence, request.presentation_time)
                    && !self
                        .failed_frames
                        .contains(&(sequence, request.presentation_time))
            })
            .cloned()
        else {
            return;
        };
        if self
            .in_flight
            .get(input)
            .is_some_and(|active| active.serves(&requested, self.decode_mode))
        {
            return;
        }
        let sequence = requested.sequence();
        let source = requested.source.clone();
        let presentation_time = requested.presentation_time;
        let size = requested.size;
        let frame_count = match self.decode_mode {
            VideoPlaybackMode::Playing => usize::try_from(Self::decode_batch_frames(
                size,
                Self::PREFETCH_BATCH_FRAMES,
                Self::PREFETCH_BATCH_FRAMES,
            ))
            .unwrap_or(1),
            VideoPlaybackMode::Scrubbing | VideoPlaybackMode::Idle => 1,
        };
        let (generation, cancel) = self.in_flight.spawn(
            input.clone(),
            source.clone(),
            presentation_time,
            size,
            self.decode_mode,
            frame_count,
        );
        if !self.ensure_worker(input) {
            self.in_flight.remove(input);
            self.failed_frames.insert((sequence, presentation_time));
            return;
        }
        let Some(worker) = self.workers.get(input) else {
            self.in_flight.remove(input);
            return;
        };
        if worker
            .requests
            .send(VideoWorkerDirective::Decode(Box::new(VideoWorkerRequest {
                generation,
                sequence,
                source,
                presentation_time,
                size,
                frame_count,
                cancel,
            })))
            .is_err()
        {
            self.workers.remove(input);
            self.in_flight.remove(input);
        }
    }

    fn finish_decode(&mut self, input: &VideoInputId, message: VideoWorkerResult) -> bool {
        if !self.in_flight.complete(input, message.generation) {
            return false;
        }
        let VideoWorkerResult {
            sequence,
            source,
            presentation_time,
            size,
            result,
            ..
        } = message;
        match result {
            Err(MediaError::Cancelled) => {}
            Ok(frames) => {
                for decoded in frames {
                    let cost = decoded.frame.rgba.len();
                    self.frame_cache.insert(
                        sequence.clone(),
                        decoded.presentation_time,
                        decoded.duration,
                        Arc::new(decoded.frame),
                        cost,
                    );
                }
                self.frame_cache.evict_to_budget(self.protected_positions());
                if self.frame_cache.contains_time(&sequence, presentation_time) {
                    self.failed_frames
                        .remove(&(sequence.clone(), presentation_time));
                } else {
                    self.failed_frames
                        .insert((sequence.clone(), presentation_time));
                }
                if self.requested_frames.get(input).is_some_and(|requests| {
                    requests.iter().any(|request| {
                        request.source == source
                            && request.presentation_time == presentation_time
                            && request.size == size
                    })
                }) {
                    self.error = None;
                }
            }
            Err(error) => {
                let current = self.requested_frames.get(input).is_some_and(|requests| {
                    requests.iter().any(|request| {
                        request.source == source
                            && request.presentation_time == presentation_time
                            && request.size == size
                    })
                });
                self.failed_frames
                    .insert((sequence.clone(), presentation_time));
                if current {
                    self.error = Some(error.to_string());
                    self.notifications
                        .push(format!("動画フレームの読み込みに失敗しました: {error}"));
                }
            }
        }
        self.revision = self.revision.saturating_add(1);
        true
    }

    pub(crate) fn reset(&mut self) {
        self.in_flight.reset();
        for (_, worker) in self.workers.drain() {
            let _ = worker.requests.send(VideoWorkerDirective::Shutdown);
        }
        self.frame_cache = BudgetedTimestampCache::new(Self::FRAME_CACHE_BUDGET_BYTES);
        self.requested_frames.clear();
        self.tick_decode_requests.clear();
        self.tick_seen_times.clear();
        self.last_presented_frames.clear();
        self.proxy.reset();
        self.failed_frames.clear();
        self.notifications.clear();
        self.error = None;
        self.revision = self.revision.saturating_add(1);
    }

    fn decode_batch_frames(size: VideoDecodeSize, desired: u64, available: u64) -> u64 {
        let frame_bytes = u64::from(size.max_width)
            .saturating_mul(u64::from(size.max_height))
            .saturating_mul(4);
        let memory_limited = Self::MAX_DECODE_BATCH_BYTES
            .checked_div(frame_bytes)
            .unwrap_or(1)
            .max(1);
        desired.min(memory_limited).min(available)
    }
}
