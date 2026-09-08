use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use gpui::{AppContext as _, Context, Entity, Subscription, Task};

use crate::{
    domain::{
        media::{MediaAsset, MediaKind, MediaSourceId},
        timeline::{Frame, FrameRate, ItemId, LayerId, TimelineItem},
    },
    engine::{
        cache::BudgetedTimestampCache,
        frame::RgbaFrame,
        media::{
            MediaError, MediaReaderRegistry, VideoDecodeSize, VideoDecoderSession, VideoProxy,
            VideoProxyRequest,
        },
    },
    ui::{
        session::{ProjectSession, ProjectSessionId, UiNotifications},
        transport::PreviewPlaybackMode,
    },
};

pub(super) struct VideoPlaybackRequest<'a> {
    pub active_items: &'a [(LayerId, TimelineItem)],
    pub playhead: Frame,
    pub frame_rate: FrameRate,
    pub playback_seconds: Option<f64>,
    pub mode: PreviewPlaybackMode,
    pub size: VideoDecodeSize,
}

struct InFlightVideoDecode {
    generation: u64,
    cancel: Arc<AtomicBool>,
    source: MediaAsset,
    presentation_time: Duration,
    size: VideoDecodeSize,
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
    ) -> (u64, Arc<AtomicBool>) {
        if let Some(active) = self.active.get(&input) {
            active.cancel.store(true, Ordering::Release);
        }
        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = self.next_generation;
        let cancel = Arc::new(AtomicBool::new(false));
        self.active.insert(
            input,
            InFlightVideoDecode {
                generation,
                cancel: cancel.clone(),
                source,
                presentation_time,
                size,
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

    fn cancel_orphans(&mut self, active_inputs: &HashSet<VideoInputId>) {
        self.active.retain(|input, decode| {
            if active_inputs.contains(input) {
                true
            } else {
                decode.cancel.store(true, Ordering::Release);
                false
            }
        });
    }
}

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
pub(super) struct VideoInputId {
    pub item_id: ItemId,
    pub input_id: String,
}

#[derive(Clone)]
struct RequestedVideoFrame {
    presentation_time: Duration,
    asset: MediaAsset,
    source: MediaAsset,
    source_start: Duration,
    size: VideoDecodeSize,
}

struct PresentedVideoFrame {
    asset: MediaAsset,
    source: MediaAsset,
    presentation_time: Duration,
    duration: Duration,
    frame: Arc<RgbaFrame>,
}

struct VideoDecoderState {
    source: MediaAsset,
    decoder: Box<dyn VideoDecoderSession>,
}

pub(super) struct VideoPlaybackSnapshot {
    pub revision: u64,
    pub frames: HashMap<VideoInputId, Arc<RgbaFrame>>,
    pub error: Option<String>,
}

pub(super) struct VideoPlaybackEngine {
    media_readers: Arc<MediaReaderRegistry>,
    frame_cache: BudgetedTimestampCache<VideoFrameSequence, Arc<RgbaFrame>>,
    requested_frames: HashMap<VideoInputId, RequestedVideoFrame>,
    last_presented_frames: HashMap<VideoInputId, PresentedVideoFrame>,
    in_flight: InFlightVideoDecodes,
    decoders: HashMap<VideoInputId, VideoDecoderState>,
    proxies: HashMap<VideoProxyKey, VideoProxy>,
    queued_proxies: VecDeque<VideoProxyJob>,
    generating_proxy: Option<VideoProxyJob>,
    failed_proxy_attempts: HashMap<VideoProxyKey, u8>,
    failed_frames: HashMap<VideoInputId, (MediaAsset, Duration)>,
    session: Entity<ProjectSession>,
    session_id: ProjectSessionId,
    notifications: Entity<UiNotifications>,
    decode_tasks: HashMap<VideoInputId, Task<()>>,
    proxy_tasks: Vec<Task<()>>,
    decode_mode: PreviewPlaybackMode,
    revision: u64,
    error: Option<String>,
    _session_subscription: Subscription,
}

impl VideoPlaybackEngine {
    // Interactive seeks decode only the requested frame. During playback,
    // bounded batches keep frames ahead of the playhead without making a seek
    // wait for a large obsolete batch or allocating hundreds of MiB at once.
    const PREFETCH_BATCH_FRAMES: u64 = 24;

    const MAX_DECODE_BATCH_BYTES: u64 = 64 * 1024 * 1024;
    const FRAME_CACHE_BUDGET_BYTES: usize = 512 * 1024 * 1024;
    const PROXY_MAX_WIDTH: u32 = 960;
    const PROXY_MAX_HEIGHT: u32 = 540;
    const PROXY_MAX_FRAMES_PER_SECOND: u32 = 60;
    // Keeping only the current and following short chunk in the work queue
    // avoids eagerly transcoding an entire source while still staying ahead.
    const PROXY_CHUNK_SECONDS: u64 = 10;
    const MAX_PROXY_ATTEMPTS: u8 = 3;

    pub(super) fn new(
        media_readers: Arc<MediaReaderRegistry>,
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
        cx: &mut Context<Self>,
    ) -> Self {
        let session_id = session.read(cx).id();
        let session_subscription = cx.observe(&session, |this, _, cx| {
            let session_id = this.session.read(cx).id();
            if session_id == this.session_id {
                return;
            }
            this.session_id = session_id;
            this.reset_for_project_change();
            cx.notify();
        });
        Self {
            media_readers,
            frame_cache: BudgetedTimestampCache::new(Self::FRAME_CACHE_BUDGET_BYTES),
            requested_frames: HashMap::new(),
            last_presented_frames: HashMap::new(),
            in_flight: InFlightVideoDecodes::default(),
            decoders: HashMap::new(),
            proxies: HashMap::new(),
            queued_proxies: VecDeque::new(),
            generating_proxy: None,
            failed_proxy_attempts: HashMap::new(),
            failed_frames: HashMap::new(),
            session,
            session_id,
            notifications,
            decode_tasks: HashMap::new(),
            proxy_tasks: Vec::new(),
            decode_mode: PreviewPlaybackMode::Idle,
            revision: 0,
            error: None,
            _session_subscription: session_subscription,
        }
    }

    pub(super) fn prepare(
        &mut self,
        request: VideoPlaybackRequest<'_>,
        cx: &mut Context<Self>,
    ) -> VideoPlaybackSnapshot {
        let requests = self.current_requests(
            request.active_items,
            request.playhead,
            request.frame_rate,
            request.playback_seconds,
        );
        let mode = request.mode;
        self.ensure_frames(requests, mode, request.size, cx);
        let requested = self
            .requested_frames
            .iter()
            .map(|(input, request)| (input.clone(), request.clone()))
            .collect::<Vec<_>>();
        let mut frames = HashMap::new();
        for (input, request) in requested {
            let sequence = VideoFrameSequence::new(
                &request.asset,
                &request.source,
                request.source_start,
                request.size,
            );
            if let Some(cached) = self
                .frame_cache
                .get_for_time(&sequence, request.presentation_time)
            {
                let presentation_changed =
                    self.last_presented_frames
                        .get(&input)
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
                        asset: request.asset,
                        source: request.source,
                        presentation_time: cached.presentation_time,
                        duration: cached.duration,
                        frame: cached.value,
                    },
                );
                frames.insert(input, frame);
                if presentation_changed {
                    self.revision = self.revision.saturating_add(1);
                }
            } else if (mode == PreviewPlaybackMode::Playing
                || !self
                    .failed_frames
                    .get(&input)
                    .is_some_and(|(failed_source, failed_time)| {
                        failed_source == &request.source
                            && *failed_time == request.presentation_time
                    }))
                && let Some(presented) = self
                    .last_presented_frames
                    .get(&input)
                    .filter(|presented| presented.asset == request.asset)
            {
                frames.insert(input, presented.frame.clone());
            }
        }
        VideoPlaybackSnapshot {
            revision: self.revision,
            frames,
            error: self.error.clone(),
        }
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

    fn proxy_needed(asset: &MediaAsset) -> bool {
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
            > u64::from(Self::PROXY_MAX_FRAMES_PER_SECOND) * u64::from(frame_rate.denominator());
        width > Self::PROXY_MAX_WIDTH || height > Self::PROXY_MAX_HEIGHT || exceeds_frame_rate
    }

    fn proxy_job(request: &VideoDecodeRequest, chunk: u64) -> Option<VideoProxyJob> {
        if !Self::proxy_needed(&request.asset) {
            return None;
        }
        let start_seconds = chunk.checked_mul(Self::PROXY_CHUNK_SECONDS)?;
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
                max_width: Self::PROXY_MAX_WIDTH,
                max_height: Self::PROXY_MAX_HEIGHT,
                max_frames_per_second: Self::PROXY_MAX_FRAMES_PER_SECOND,
                source_start,
                source_duration: Duration::from_secs(Self::PROXY_CHUNK_SECONDS),
            },
        })
    }

    fn update_proxy_queue(&mut self, requests: &[VideoDecodeRequest]) {
        let mut desired = Vec::new();
        for offset in 0..=1 {
            for request in requests {
                let chunk = request
                    .proxy_key(Self::PROXY_CHUNK_SECONDS)
                    .chunk
                    .saturating_add(offset);
                if let Some(job) = Self::proxy_job(request, chunk) {
                    desired.push(job);
                }
            }
        }
        let desired_keys = desired
            .iter()
            .map(|job| job.key.clone())
            .collect::<HashSet<_>>();
        self.queued_proxies
            .retain(|job| desired_keys.contains(&job.key));
        for job in desired {
            if self.proxies.contains_key(&job.key)
                || self
                    .failed_proxy_attempts
                    .get(&job.key)
                    .is_some_and(|attempts| *attempts >= Self::MAX_PROXY_ATTEMPTS)
                || self
                    .generating_proxy
                    .as_ref()
                    .is_some_and(|current| current.key == job.key)
                || self
                    .queued_proxies
                    .iter()
                    .any(|queued| queued.key == job.key)
            {
                continue;
            }
            self.queued_proxies.push_back(job);
        }
    }

    fn start_next_proxy(&mut self, cx: &mut Context<Self>) {
        if self.generating_proxy.is_some() {
            return;
        }
        let Some(job) = self.queued_proxies.pop_front() else {
            return;
        };
        self.generating_proxy = Some(job.clone());
        let key = job.key.clone();
        let media_readers = self.media_readers.clone();
        let session = self.session.clone();
        let session_id = session.read(cx).id();
        let task = cx.spawn(async move |playback, cx| {
            let result = cx
                .background_spawn(async move {
                    media_readers.create_video_proxy(&job.source, job.request)
                })
                .await;
            if !session.update(cx, |session, _| session.is_current(session_id)) {
                return;
            }
            if let Some(playback) = playback.upgrade() {
                playback.update(cx, |playback, cx| {
                    if playback
                        .generating_proxy
                        .as_ref()
                        .is_none_or(|current| current.key != key)
                    {
                        return;
                    }
                    playback.generating_proxy = None;
                    match result {
                        Ok(proxy) => {
                            playback.failed_proxy_attempts.remove(&key);
                            playback.proxies.insert(key, proxy);
                        }
                        Err(error) => {
                            let attempts = playback.failed_proxy_attempts.entry(key).or_insert(0);
                            *attempts = attempts.saturating_add(1);
                            let message = format!(
                                "動画proxyの生成に失敗しました ({}/{}): {error}",
                                *attempts,
                                Self::MAX_PROXY_ATTEMPTS,
                            );
                            playback.error = Some(message.clone());
                            playback.notifications.update(cx, |notifications, cx| {
                                notifications.push(message, cx);
                            });
                        }
                    }
                    playback.start_next_proxy(cx);
                    cx.notify();
                });
            }
        });
        self.proxy_tasks.push(task);
    }

    fn ensure_frames(
        &mut self,
        requests: Vec<VideoDecodeRequest>,
        mode: PreviewPlaybackMode,
        size: VideoDecodeSize,
        cx: &mut Context<Self>,
    ) {
        self.decode_mode = mode;
        let active_inputs = requests
            .iter()
            .map(|request| request.input.clone())
            .collect::<HashSet<_>>();
        self.requested_frames
            .retain(|input, _| active_inputs.contains(input));
        self.last_presented_frames
            .retain(|input, _| active_inputs.contains(input));
        self.failed_frames
            .retain(|input, _| active_inputs.contains(input));
        self.in_flight.cancel_orphans(&active_inputs);
        self.decode_tasks
            .retain(|input, _| active_inputs.contains(input));
        self.decoders
            .retain(|input, _| active_inputs.contains(input));

        self.update_proxy_queue(&requests);
        for request in requests {
            let proxy = matches!(
                mode,
                PreviewPlaybackMode::Playing | PreviewPlaybackMode::Scrubbing
            )
            .then(|| {
                self.proxies
                    .get(&request.proxy_key(Self::PROXY_CHUNK_SECONDS))
            })
            .flatten();
            let (source, source_start) = proxy.map_or_else(
                || (request.asset.clone(), Duration::ZERO),
                |proxy| (proxy.asset.clone(), proxy.source_start),
            );
            let presentation_time = request.presentation_time_in(source_start);
            self.requested_frames.insert(
                request.input.clone(),
                RequestedVideoFrame {
                    presentation_time,
                    asset: request.asset.clone(),
                    source: source.clone(),
                    source_start,
                    size,
                },
            );
            self.decode_input_if_needed(&request.input, cx);
        }
        self.start_next_proxy(cx);
    }

    fn decode_input_if_needed(&mut self, input: &VideoInputId, cx: &mut Context<Self>) {
        let Some(requested) = self.requested_frames.get(input).cloned() else {
            return;
        };
        let sequence = VideoFrameSequence::new(
            &requested.asset,
            &requested.source,
            requested.source_start,
            requested.size,
        );
        if self
            .frame_cache
            .contains_time(&sequence, requested.presentation_time)
        {
            return;
        }
        if self
            .failed_frames
            .get(input)
            .is_some_and(|(failed_source, failed_time)| {
                failed_source == &requested.source && *failed_time == requested.presentation_time
            })
        {
            return;
        }
        if let Some(active) = self.in_flight.get(input) {
            if active.source != requested.source
                || active.presentation_time != requested.presentation_time
                || active.size != requested.size
            {
                active.cancel.store(true, Ordering::Release);
            }
            return;
        }
        let source = requested.source.clone();
        let presentation_time = requested.presentation_time;
        let size = requested.size;
        let frame_count = match self.decode_mode {
            PreviewPlaybackMode::Playing => usize::try_from(Self::decode_batch_frames(
                size,
                Self::PREFETCH_BATCH_FRAMES,
                Self::PREFETCH_BATCH_FRAMES,
            ))
            .unwrap_or(1),
            PreviewPlaybackMode::Scrubbing | PreviewPlaybackMode::Idle => 1,
        };
        let decoder = self
            .decoders
            .remove(input)
            .filter(|decoder| decoder.source == source);
        let (generation, cancel) =
            self.in_flight
                .spawn(input.clone(), source.clone(), presentation_time, size);
        let media_readers = self.media_readers.clone();
        let source_for_open = source.clone();
        let session = self.session.clone();
        let session_id = session.read(cx).id();
        let task_input = input.clone();
        let task = cx.spawn(async move |playback, cx| {
            let (decoder, result) = cx
                .background_spawn(async move {
                    let mut decoder = match decoder {
                        Some(decoder) => decoder,
                        None => match media_readers.open_video_decoder(&source_for_open) {
                            Ok(decoder) => VideoDecoderState {
                                source: source_for_open,
                                decoder,
                            },
                            Err(error) => return (None, Err(error)),
                        },
                    };
                    let result =
                        decoder
                            .decoder
                            .decode_from(presentation_time, frame_count, size, &cancel);
                    (Some(decoder), result)
                })
                .await;
            if !session.update(cx, |session, _| session.is_current(session_id)) {
                return;
            }
            if let Some(playback) = playback.upgrade() {
                playback.update(cx, |playback, cx| {
                    let matched = playback.in_flight.complete(&task_input, generation);
                    if let Some(decoder) = decoder {
                        let reuse = playback
                            .requested_frames
                            .get(&task_input)
                            .is_some_and(|request| request.source == decoder.source);
                        if reuse {
                            playback.decoders.insert(task_input.clone(), decoder);
                        }
                    }
                    if !matched {
                        return;
                    }
                    match result {
                        Err(MediaError::Cancelled) => {}
                        Ok(frames) => {
                            for decoded in frames {
                                let cost = decoded.frame.rgba.len();
                                playback.frame_cache.insert(
                                    sequence.clone(),
                                    decoded.presentation_time,
                                    decoded.duration,
                                    Arc::new(decoded.frame),
                                    cost,
                                );
                            }
                            playback.frame_cache.evict_to_budget(
                                playback.requested_frames.values().map(|request| {
                                    (
                                        VideoFrameSequence::new(
                                            &request.asset,
                                            &request.source,
                                            request.source_start,
                                            request.size,
                                        ),
                                        request.presentation_time,
                                    )
                                }),
                            );
                            playback.failed_frames.remove(&task_input);
                            if playback
                                .requested_frames
                                .get(&task_input)
                                .is_some_and(|request| {
                                    request.source == source
                                        && request.presentation_time == presentation_time
                                        && request.size == size
                                })
                            {
                                playback.error = None;
                            }
                        }
                        Err(error) => {
                            let current =
                                playback
                                    .requested_frames
                                    .get(&task_input)
                                    .is_some_and(|request| {
                                        request.source == source
                                            && request.presentation_time == presentation_time
                                            && request.size == size
                                    });
                            playback
                                .failed_frames
                                .insert(task_input.clone(), (source, presentation_time));
                            if current {
                                playback.error = Some(error.to_string());
                                playback.notifications.update(cx, |notifications, cx| {
                                    notifications.push(
                                        format!("動画フレームの読み込みに失敗しました: {error}"),
                                        cx,
                                    );
                                });
                            }
                        }
                    }
                    playback.revision = playback.revision.saturating_add(1);
                    playback.decode_input_if_needed(&task_input, cx);
                    cx.notify();
                });
            }
        });
        self.decode_tasks.insert(input.clone(), task);
    }

    fn reset_for_project_change(&mut self) {
        self.decode_tasks.clear();
        self.proxy_tasks.clear();
        self.frame_cache = BudgetedTimestampCache::new(Self::FRAME_CACHE_BUDGET_BYTES);
        self.requested_frames.clear();
        self.last_presented_frames.clear();
        self.in_flight = InFlightVideoDecodes::default();
        self.decoders.clear();
        self.proxies.clear();
        self.queued_proxies.clear();
        self.generating_proxy = None;
        self.failed_proxy_attempts.clear();
        self.failed_frames.clear();
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::domain::media::{MediaAsset, MediaKind};

    use super::*;

    fn test_asset() -> MediaAsset {
        MediaAsset {
            reader_id: "test".to_owned(),
            path: PathBuf::from("test.mp4"),
            name: "test".to_owned(),
            duration: Duration::from_secs(60),
            kind: MediaKind::Image {
                width: 64,
                height: 64,
            },
        }
    }

    fn test_input() -> VideoInputId {
        VideoInputId {
            item_id: ItemId(1),
            input_id: "video".to_owned(),
        }
    }

    fn test_size() -> VideoDecodeSize {
        VideoDecodeSize {
            max_width: 64,
            max_height: 64,
        }
    }

    #[test]
    fn newer_seek_supersedes_in_flight_decode() {
        let mut in_flight = InFlightVideoDecodes::default();
        let input = test_input();
        let (first, _) = in_flight.spawn(
            input.clone(),
            test_asset(),
            Duration::from_secs(1),
            test_size(),
        );
        let (second, _) = in_flight.spawn(
            input.clone(),
            test_asset(),
            Duration::from_secs(10),
            test_size(),
        );
        let (third, _) = in_flight.spawn(
            input.clone(),
            test_asset(),
            Duration::from_secs(30),
            test_size(),
        );
        assert!(!in_flight.complete(&input, first));
        assert!(!in_flight.complete(&input, second));
        assert!(in_flight.complete(&input, third));
        assert!(!in_flight.complete(&input, third));
    }

    #[test]
    fn superseded_decode_requests_cancellation() {
        use std::sync::atomic::Ordering;

        let mut in_flight = InFlightVideoDecodes::default();
        let input = test_input();
        let (_, first_cancel) = in_flight.spawn(
            input.clone(),
            test_asset(),
            Duration::from_secs(1),
            test_size(),
        );
        assert!(!first_cancel.load(Ordering::Acquire));
        in_flight.spawn(
            input.clone(),
            test_asset(),
            Duration::from_secs(30),
            test_size(),
        );
        assert!(first_cancel.load(Ordering::Acquire));
    }

    #[test]
    fn orphaned_inputs_are_cancelled() {
        use std::sync::atomic::Ordering;

        let mut in_flight = InFlightVideoDecodes::default();
        let input = test_input();
        let (_, cancel) = in_flight.spawn(
            input.clone(),
            test_asset(),
            Duration::from_secs(1),
            test_size(),
        );
        in_flight.cancel_orphans(&HashSet::new());
        assert!(cancel.load(Ordering::Acquire));
        assert!(in_flight.get(&input).is_none());
    }
}
