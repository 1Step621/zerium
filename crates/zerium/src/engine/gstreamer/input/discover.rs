//! Container and stream metadata from GStreamer Discoverer.
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use gstreamer as gst;
use gstreamer_pbutils::{Discoverer, DiscovererInfo, DiscovererResult};
use zerium_core::media::{FileRevision, MediaKind, MediaMetadata, MediaTarget, VideoFrameRate};

use super::super::{initialize, pipeline::file_uri};
use crate::engine::media::{MediaError, read_file_revision};

pub(crate) fn probe(path: &Path, target: MediaTarget) -> Result<Option<MediaMetadata>, MediaError> {
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

const CACHE_ENTRIES: usize = 128;
type Discovery = Arc<OnceLock<Result<DiscovererInfo, String>>>;

struct CachedDiscovery {
    path: PathBuf,
    revision: FileRevision,
    discovery: Discovery,
}

fn discover(path: &Path) -> Result<DiscovererInfo, MediaError> {
    initialize().map_err(MediaError::external)?;
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
        if cache.len() > CACHE_ENTRIES {
            cache.pop_front();
        }
        discovery
    };
    // Concurrent audio/video requests share one discovery without holding the
    // cache lock while GStreamer opens and prerolls the file.
    let result = discovery.get_or_init(|| {
        let discoverer =
            Discoverer::new(gst::ClockTime::from_seconds(10)).map_err(|error| error.to_string())?;
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

fn duration(info: &gstreamer_pbutils::DiscovererInfo) -> Result<Duration, MediaError> {
    info.duration()
        .filter(|duration| !duration.is_zero())
        .map(|duration| Duration::from_nanos(duration.nseconds()))
        .ok_or_else(|| MediaError::external("Failed to get media duration"))
}
