//! Device discovery, decoder opening and prebuffering, outside the UI thread.
use super::{AudioPlaybackError, MIX_BLOCK_SAMPLE_FRAMES};
use crate::engine::media::{
    AudioCalculationPolicy, AudioFormat, AudioTimelineGraph, MediaReaderRegistry,
};
use cpal::traits::{DeviceTrait as _, HostTrait as _};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};
use zerium_core::timeline::{AudioTimelineSnapshot, Frame, FrameRate};

const AUDIO_BUFFER_MILLIS: u64 = 200;

/// Initial playback starts at an exact video-frame boundary. Starts after edits
/// use the continuous timeline position, rounded down to a sample.
pub(super) enum PlaybackPosition {
    Frame(Frame),
    Seconds(f64),
}

impl PlaybackPosition {
    pub(super) fn sample_frame(self, frame_rate: FrameRate, sample_rate: u32) -> u64 {
        match self {
            Self::Frame(frame) => frame_rate.sample_boundary(frame, sample_rate),
            Self::Seconds(seconds) => (seconds * f64::from(sample_rate)).floor() as u64,
        }
    }
}

pub(super) struct AudioPreparation {
    timeline: AudioTimelineSnapshot,
    handle: thread::JoinHandle<Result<Option<PreparedAudio>, AudioPlaybackError>>,
    stop: Arc<AtomicBool>,
}

pub(super) struct PreparedAudio {
    pub(super) device: cpal::Device,
    pub(super) supported: cpal::SupportedStreamConfig,
    pub(super) graph: AudioTimelineGraph,
    pub(super) producer: rtrb::Producer<f32>,
    pub(super) consumer: rtrb::Consumer<f32>,
    pub(super) start_sample_frame: u64,
    pub(super) render_cursor: u64,
}

fn prepare_audio(
    timeline: AudioTimelineSnapshot,
    position: PlaybackPosition,
    media_readers: Arc<MediaReaderRegistry>,
    stop: &AtomicBool,
) -> Result<Option<PreparedAudio>, AudioPlaybackError> {
    if stop.load(Ordering::Acquire) {
        return Ok(None);
    }
    if timeline.sources().is_empty() {
        return Ok(None);
    }
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or(AudioPlaybackError::DeviceUnavailable)?;
    let supported = device.default_output_config().map_err(|error| {
        AudioPlaybackError::Configuration(format!(
            "Failed to get audio output configuration: {error}"
        ))
    })?;
    let format = AudioFormat {
        sample_rate: supported.sample_rate(),
        channels: supported.channels(),
    };
    let mut graph = AudioTimelineGraph::new(
        timeline.clone(),
        format,
        &media_readers,
        AudioCalculationPolicy::Fallback,
    )
    .map_err(AudioPlaybackError::Timeline)?;
    if graph.is_empty() {
        return Ok(None);
    }
    let channels = usize::from(format.channels);
    let capacity = u64::from(format.sample_rate)
        .saturating_mul(channels as u64)
        .saturating_mul(AUDIO_BUFFER_MILLIS)
        .checked_div(1000)
        .and_then(|samples| usize::try_from(samples).ok())
        .unwrap_or(channels)
        .max(channels);
    let (mut producer, consumer) = rtrb::RingBuffer::<f32>::new(capacity);
    let start_sample_frame = position.sample_frame(timeline.frame_rate(), format.sample_rate);
    let mut render_cursor = start_sample_frame;
    let mut prebuffered_frames = 0u64;
    let capacity_frames = (capacity / channels.max(1)) as u64;
    while prebuffered_frames.saturating_add(MIX_BLOCK_SAMPLE_FRAMES as u64) <= capacity_frames {
        if stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        let block = graph
            .render(render_cursor, MIX_BLOCK_SAMPLE_FRAMES)
            .map_err(AudioPlaybackError::Timeline)?;
        if block.is_empty() {
            break;
        }
        for sample in block {
            producer.push(sample).map_err(|_| {
                AudioPlaybackError::Worker("Initial audio buffer is too small".to_owned())
            })?;
        }
        prebuffered_frames = prebuffered_frames.saturating_add(MIX_BLOCK_SAMPLE_FRAMES as u64);
        render_cursor = render_cursor.saturating_add(MIX_BLOCK_SAMPLE_FRAMES as u64);
    }

    if stop.load(Ordering::Acquire) {
        return Ok(None);
    }
    Ok(Some(PreparedAudio {
        device,
        supported,
        graph,
        producer,
        consumer,
        start_sample_frame,
        render_cursor,
    }))
}

impl AudioPreparation {
    pub(super) fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    pub(super) fn matches_timeline(&self, timeline: &AudioTimelineSnapshot) -> bool {
        self.timeline.same_content(timeline)
    }

    pub(super) fn finish(self) -> Result<Option<PreparedAudio>, AudioPlaybackError> {
        self.handle.join().unwrap_or_else(|_| {
            Err(AudioPlaybackError::Worker(
                "Audio preparation panicked".to_owned(),
            ))
        })
    }

    pub(super) fn cancel(self) {
        // The detached task owns its resources; its result can no longer be installed.
        self.stop.store(true, Ordering::Release);
    }

    pub(super) fn spawn(
        timeline: AudioTimelineSnapshot,
        position: PlaybackPosition,
        media_readers: Arc<MediaReaderRegistry>,
    ) -> Result<Self, AudioPlaybackError> {
        let prepared_timeline = timeline.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let handle = thread::Builder::new()
            .name("zerium-audio-prepare".to_owned())
            .spawn(move || prepare_audio(timeline, position, media_readers, &worker_stop))
            .map_err(|error| {
                AudioPlaybackError::Worker(format!("Failed to prepare audio: {error}"))
            })?;
        Ok(Self {
            timeline: prepared_timeline,
            handle,
            stop,
        })
    }
}
