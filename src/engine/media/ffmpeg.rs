use std::{
    collections::hash_map::DefaultHasher,
    env, fs,
    hash::{Hash, Hasher},
    io,
    path::{Path, PathBuf},
    sync::{
        atomic::AtomicBool,
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::domain::{
    media::{MediaAsset, MediaKind, VideoFrameRate},
    plugin::MediaType,
};

use super::{
    atomic_file::AtomicFileTransaction,
    ffmpeg_encoder::{FfmpegFileEncoder, VideoColorSpec, VideoEncoderSettings, VideoOutputSpec},
    ffmpeg_next::{self, FfmpegAudioDecoder, FfmpegVideoDecoder},
    reader::{
        AudioDecoderSession, MediaError, MediaProbe, MediaReader, VideoDecodeSize,
        VideoDecoderSession, VideoProxy, VideoProxyRequest,
    },
};

pub(super) const READER_ID: &str = "zerium.ffmpeg";
const VIDEO_PROXY_FORMAT_VERSION: u8 = 5;
const PROXY_CACHE_BUDGET_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const PROXY_LOCK_STALE_AFTER: Duration = Duration::from_secs(10 * 60);
const PROXY_LOCK_RETRY_DELAY: Duration = Duration::from_millis(50);
const PROXY_LOCK_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const PROXY_LOCK_RETRIES: usize = 200;
const PROXY_TEMP_STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

pub(super) struct FfmpegMediaReader;

impl MediaReader for FfmpegMediaReader {
    fn probe(&self, path: &Path, media_type: MediaType) -> Result<Option<MediaProbe>, MediaError> {
        ffmpeg_next::probe(path, media_type).map(Some)
    }

    fn open_video_decoder(
        &self,
        asset: &MediaAsset,
    ) -> Result<Box<dyn VideoDecoderSession>, MediaError> {
        if asset.kind.dimensions().is_none() {
            return Err(MediaError::external(
                "音声素材から映像デコーダーは作成できません",
            ));
        }
        Ok(Box::new(FfmpegVideoDecoder::open(asset.clone())?))
    }

    fn open_audio_decoder(
        &self,
        asset: &MediaAsset,
    ) -> Result<Box<dyn AudioDecoderSession>, MediaError> {
        if !asset.kind.has_audio() {
            return Err(MediaError::external(
                "このメディア素材に音声ストリームはありません",
            ));
        }
        Ok(Box::new(FfmpegAudioDecoder::open(asset.clone())?))
    }

    fn create_video_proxy(
        &self,
        asset: &MediaAsset,
        request: VideoProxyRequest,
    ) -> Result<VideoProxy, MediaError> {
        create_video_proxy_in(asset, request, &video_proxy_cache_dir())
    }
}

pub(super) fn fit_dimensions(
    source_width: u32,
    source_height: u32,
    max_width: u32,
    max_height: u32,
) -> Option<(u32, u32)> {
    if source_width == 0 || source_height == 0 || max_width == 0 || max_height == 0 {
        return None;
    }
    let scale = (f64::from(max_width) / f64::from(source_width))
        .min(f64::from(max_height) / f64::from(source_height))
        .min(1.);
    Some((
        (f64::from(source_width) * scale).round().max(1.) as u32,
        (f64::from(source_height) * scale).round().max(1.) as u32,
    ))
}

fn create_video_proxy_in(
    asset: &MediaAsset,
    request: VideoProxyRequest,
    cache_dir: &Path,
) -> Result<VideoProxy, MediaError> {
    if request.max_width == 0
        || request.max_height == 0
        || request.max_frames_per_second == 0
        || request.source_duration.is_zero()
    {
        return Err(MediaError::external("プロキシの生成条件が不正です"));
    }
    let (source_width, source_height, source_frame_rate) = match asset.kind {
        MediaKind::Video {
            width,
            height,
            frame_rate,
            ..
        } => (width, height, frame_rate),
        MediaKind::Image { .. } => {
            return Err(MediaError::external(
                "画像素材から映像プロキシは作成できません",
            ));
        }
        MediaKind::Audio { .. } => {
            return Err(MediaError::external(
                "音声素材から映像プロキシは作成できません",
            ));
        }
    };
    let (mut width, mut height) = fit_dimensions(
        source_width,
        source_height,
        request.max_width,
        request.max_height,
    )
    .ok_or_else(|| MediaError::external("プロキシの映像サイズが不正です"))?;
    width -= width % 2;
    height -= height % 2;
    if width == 0 || height == 0 {
        return Err(MediaError::external(
            "プロキシの映像サイズは2ピクセル以上である必要があります",
        ));
    }
    let video_stream_duration = ffmpeg_next::probe(&asset.path, MediaType::Video)?
        .streams
        .video
        .unwrap_or(asset.duration);
    let remaining_duration = video_stream_duration
        .checked_sub(request.source_start)
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| MediaError::external("プロキシの開始位置が素材の範囲外です"))?;
    let duration = request.source_duration.min(remaining_duration);
    let frame_rate = capped_frame_rate(source_frame_rate, request.max_frames_per_second);

    fs::create_dir_all(cache_dir).map_err(|error| {
        MediaError::external(format!(
            "プロキシキャッシュ'{}'を作成できません: {error}",
            cache_dir.display()
        ))
    })?;
    cleanup_stale_proxy_temporary_files(cache_dir)?;
    let target = cache_dir.join(format!(
        "v{VIDEO_PROXY_FORMAT_VERSION}-{:016x}-{:x}-{:x}-{width}x{height}-{}x{}.mkv",
        proxy_cache_key(asset)?,
        request.source_start.as_nanos(),
        duration.as_nanos(),
        frame_rate.numerator(),
        frame_rate.denominator(),
    ));
    if let Some(proxy) = load_cached_proxy(asset, &target, request.source_start)? {
        return Ok(proxy);
    }

    let _lock = CacheFileLock::acquire(&target)?;
    if let Some(proxy) = load_cached_proxy(asset, &target, request.source_start)? {
        return Ok(proxy);
    }

    let transaction = AtomicFileTransaction::new(&target).map_err(|error| {
        MediaError::external(format!("プロキシ一時ファイルを作成できません: {error}"))
    })?;
    create_proxy_file(
        asset,
        transaction.temporary_path(),
        width,
        height,
        frame_rate,
        request.source_start,
        duration,
    )?;
    ffmpeg_next::probe(transaction.temporary_path(), MediaType::Video)?;
    transaction.commit().map_err(|error| {
        MediaError::external(format!(
            "プロキシをキャッシュ'{}'へ確定できません: {error}",
            target.display()
        ))
    })?;
    enforce_proxy_cache_budget(cache_dir, &target)?;
    Ok(VideoProxy {
        asset: proxy_asset(asset, target)?,
        source_start: request.source_start,
    })
}

fn load_cached_proxy(
    asset: &MediaAsset,
    target: &Path,
    source_start: Duration,
) -> Result<Option<VideoProxy>, MediaError> {
    let metadata = match target.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(MediaError::external(format!(
                "プロキシキャッシュ'{}'の情報を取得できません: {error}",
                target.display()
            )));
        }
    };
    if !metadata.is_file() || metadata.len() == 0 {
        fs::remove_file(target).map_err(|error| {
            MediaError::external(format!(
                "不正なプロキシキャッシュ'{}'を削除できません: {error}",
                target.display()
            ))
        })?;
        return Ok(None);
    }
    match proxy_asset(asset, target.to_path_buf()) {
        Ok(asset) => {
            let _ = fs::OpenOptions::new()
                .write(true)
                .open(target)
                .and_then(|file| {
                    file.set_times(fs::FileTimes::new().set_modified(SystemTime::now()))
                });
            Ok(Some(VideoProxy {
                asset,
                source_start,
            }))
        }
        Err(_) => {
            fs::remove_file(target).map_err(|error| {
                MediaError::external(format!(
                    "破損したプロキシキャッシュ'{}'を削除できません: {error}",
                    target.display()
                ))
            })?;
            Ok(None)
        }
    }
}

#[derive(Debug)]
struct CacheFileLock {
    path: PathBuf,
    heartbeat_stop: Option<SyncSender<()>>,
    heartbeat: Option<thread::JoinHandle<()>>,
}

impl CacheFileLock {
    fn try_acquire(target: &Path) -> io::Result<Self> {
        let path = target.with_extension("lock");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let heartbeat_path = path.clone();
        let (heartbeat_stop, stop_receiver) = mpsc::sync_channel(1);
        let heartbeat = match thread::Builder::new()
            .name("zerium-proxy-lock".to_owned())
            .spawn(move || {
                loop {
                    match stop_receiver.recv_timeout(PROXY_LOCK_HEARTBEAT_INTERVAL) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            let _ = fs::OpenOptions::new()
                                .write(true)
                                .open(&heartbeat_path)
                                .and_then(|file| {
                                    file.set_times(
                                        fs::FileTimes::new().set_modified(SystemTime::now()),
                                    )
                                });
                        }
                    }
                }
            }) {
            Ok(heartbeat) => heartbeat,
            Err(error) => {
                let _ = fs::remove_file(&path);
                return Err(error);
            }
        };
        Ok(Self {
            path,
            heartbeat_stop: Some(heartbeat_stop),
            heartbeat: Some(heartbeat),
        })
    }

    fn acquire(target: &Path) -> Result<Self, MediaError> {
        let path = target.with_extension("lock");
        for _ in 0..PROXY_LOCK_RETRIES {
            match Self::try_acquire(target) {
                Ok(lock) => return Ok(lock),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let stale = path
                        .metadata()
                        .ok()
                        .and_then(|metadata| metadata.modified().ok())
                        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                        .is_some_and(|age| age >= PROXY_LOCK_STALE_AFTER);
                    if stale {
                        let still_stale = path
                            .metadata()
                            .ok()
                            .and_then(|metadata| metadata.modified().ok())
                            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                            .is_some_and(|age| age >= PROXY_LOCK_STALE_AFTER);
                        if still_stale {
                            let _ = fs::remove_file(&path);
                            continue;
                        }
                    }
                    thread::sleep(PROXY_LOCK_RETRY_DELAY);
                }
                Err(error) => {
                    return Err(MediaError::external(format!(
                        "プロキシキャッシュロック'{}'を作成できません: {error}",
                        path.display()
                    )));
                }
            }
        }
        Err(MediaError::external(format!(
            "プロキシキャッシュ'{}'は別の処理が生成中です",
            target.display()
        )))
    }
}

impl Drop for CacheFileLock {
    fn drop(&mut self) {
        if let Some(stop) = self.heartbeat_stop.take() {
            let _ = stop.send(());
        }
        if let Some(heartbeat) = self.heartbeat.take() {
            let _ = heartbeat.join();
        }
        let _ = fs::remove_file(&self.path);
    }
}

fn cleanup_stale_proxy_temporary_files(cache_dir: &Path) -> Result<(), MediaError> {
    let entries = fs::read_dir(cache_dir).map_err(|error| {
        MediaError::external(format!(
            "プロキシキャッシュ'{}'を読み取れません: {error}",
            cache_dir.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            MediaError::external(format!(
                "プロキシキャッシュ'{}'の項目を読み取れません: {error}",
                cache_dir.display()
            ))
        })?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("tmp") {
            continue;
        }
        let metadata = entry.metadata().map_err(|error| {
            MediaError::external(format!(
                "プロキシ一時ファイル'{}'の情報を取得できません: {error}",
                path.display()
            ))
        })?;
        let stale = metadata
            .modified()
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age >= PROXY_TEMP_STALE_AFTER);
        if stale
            && let Err(error) = fs::remove_file(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(MediaError::external(format!(
                "古いプロキシ一時ファイル'{}'を削除できません: {error}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn enforce_proxy_cache_budget(cache_dir: &Path, protected: &Path) -> Result<(), MediaError> {
    let entries = fs::read_dir(cache_dir).map_err(|error| {
        MediaError::external(format!(
            "プロキシキャッシュ'{}'を読み取れません: {error}",
            cache_dir.display()
        ))
    })?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            MediaError::external(format!(
                "プロキシキャッシュ'{}'の項目を読み取れません: {error}",
                cache_dir.display()
            ))
        })?;
        let path = entry.path();
        if path == protected || path.extension().and_then(|value| value.to_str()) != Some("mkv") {
            continue;
        }
        let metadata = entry.metadata().map_err(|error| {
            MediaError::external(format!(
                "プロキシキャッシュ'{}'の情報を取得できません: {error}",
                path.display()
            ))
        })?;
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        files.push((modified, metadata.len(), path));
    }
    let protected_size = protected
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let mut total = files
        .iter()
        .map(|(_, size, _)| *size)
        .sum::<u64>()
        .saturating_add(protected_size);
    files.sort_by_key(|(modified, _, _)| *modified);
    for (_, size, path) in files {
        if total <= PROXY_CACHE_BUDGET_BYTES {
            break;
        }
        match fs::remove_file(&path) {
            Ok(()) => total = total.saturating_sub(size),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(MediaError::external(format!(
                    "プロキシキャッシュ'{}'を削除できません: {error}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

fn create_proxy_file(
    asset: &MediaAsset,
    output_path: &Path,
    width: u32,
    height: u32,
    frame_rate: VideoFrameRate,
    source_start: Duration,
    duration: Duration,
) -> Result<(), MediaError> {
    if !matches!(asset.kind, MediaKind::Video { .. }) {
        return Err(MediaError::external("動画以外はプロキシへ変換できません"));
    }
    let target_frame_count = (duration.as_secs_f64() * frame_rate.frames_per_second())
        .ceil()
        .clamp(1., u64::MAX as f64) as u64;
    let mut decoder = FfmpegVideoDecoder::open(asset.clone())?;
    let mut encoder = FfmpegFileEncoder::create(
        output_path,
        VideoEncoderSettings {
            output: VideoOutputSpec {
                width,
                height,
                color: VideoColorSpec::BT709_LIMITED,
            },
            frame_rate,
            preset: "ultrafast",
            crf: 23,
            gop: 15,
            audio: None,
            container: Some("matroska"),
            fast_start: false,
        },
    )
    .map_err(MediaError::external)?;
    let cancelled = AtomicBool::new(false);
    for target_frame in 0..target_frame_count {
        let target_time = source_start
            .checked_add(
                Duration::try_from_secs_f64(frame_rate.frame_to_seconds(target_frame)).map_err(
                    |_| MediaError::external("プロキシ映像の presentation time が不正です"),
                )?,
            )
            .ok_or_else(|| {
                MediaError::external("プロキシ映像の presentation time が大きすぎます")
            })?;
        let decoded = decoder.decode_at(
            target_time,
            VideoDecodeSize {
                max_width: width,
                max_height: height,
            },
            &cancelled,
        )?;
        encoder
            .encode_video(&decoded.frame, target_frame)
            .map_err(MediaError::external)?;
    }
    encoder.finish().map_err(MediaError::external)
}

fn capped_frame_rate(source: VideoFrameRate, max_frames_per_second: u32) -> VideoFrameRate {
    if u64::from(source.numerator())
        > u64::from(max_frames_per_second) * u64::from(source.denominator())
    {
        VideoFrameRate::new(max_frames_per_second, 1)
            .expect("the proxy request requires a non-zero frame rate")
    } else {
        source
    }
}

fn proxy_asset(asset: &MediaAsset, path: PathBuf) -> Result<MediaAsset, MediaError> {
    let probe = ffmpeg_next::probe(&path, MediaType::Video)?;
    Ok(MediaAsset {
        reader_id: asset.reader_id.clone(),
        path,
        name: asset.name.clone(),
        duration: probe.duration,
        kind: probe.kind,
    })
}

fn proxy_cache_key(asset: &MediaAsset) -> Result<u64, MediaError> {
    let metadata = fs::metadata(&asset.path).map_err(|error| {
        MediaError::external(format!(
            "メディアファイル'{}'の情報を取得できません: {error}",
            asset.path.display()
        ))
    })?;
    let mut hasher = DefaultHasher::new();
    asset.reader_id.hash(&mut hasher);
    asset.path.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .hash(&mut hasher);
    Ok(hasher.finish())
}

fn video_proxy_cache_dir() -> PathBuf {
    if let Some(path) = env::var_os("XDG_CACHE_HOME").filter(|path| !path.is_empty()) {
        return PathBuf::from(path).join("zerium/proxies");
    }
    if cfg!(target_os = "windows")
        && let Some(path) = env::var_os("LOCALAPPDATA").filter(|path| !path.is_empty())
    {
        return PathBuf::from(path).join("Zerium/proxies");
    }
    if let Some(path) = env::var_os("HOME").filter(|path| !path.is_empty()) {
        let path = PathBuf::from(path);
        if cfg!(target_os = "macos") {
            return path.join("Library/Caches/Zerium/proxies");
        }
        return path.join(".cache/zerium/proxies");
    }
    env::temp_dir().join("zerium/proxies")
}
