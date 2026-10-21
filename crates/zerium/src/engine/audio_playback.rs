use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use thiserror::Error;

use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};

use crate::engine::media::{
    AudioCalculationPolicy, AudioFormat, AudioTimelineError, AudioTimelineGraph,
    MediaReaderRegistry,
};
use zerium_core::timeline::{AudioTimelineSnapshot, Frame, FrameRate};

const AUDIO_BUFFER_MILLIS: u64 = 200;
const MIX_BLOCK_SAMPLE_FRAMES: usize = 2_048;
const PREBUFFER_MILLIS: u64 = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaybackClock {
    Audio,
    Wall,
}

/// Initial playback starts at an exact video-frame boundary. Deferred starts
/// join the running wall clock at a continuous position instead.
enum PlaybackPosition {
    Frame(Frame),
    Seconds(f64),
}

impl PlaybackPosition {
    fn sample_frame(self, frame_rate: FrameRate, sample_rate: u32) -> u64 {
        match self {
            Self::Frame(frame) => frame_rate.sample_boundary(frame, sample_rate),
            Self::Seconds(seconds) => (seconds * f64::from(sample_rate)).floor() as u64,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AudioPlaybackEvent {
    Underrun { missing_sample_frames: usize },
    Recovered,
    DecodeFailed(AudioTimelineError),
    DeviceFailed(String),
    WorkerPanicked,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum AudioPlaybackError {
    #[error("No audio output device was found")]
    DeviceUnavailable,
    #[error("{0}")]
    Configuration(String),
    #[error("{0}")]
    Stream(String),
    #[error("{0}")]
    Worker(String),
    #[error(transparent)]
    Timeline(#[from] AudioTimelineError),
}

struct AudioUnderrunState {
    active: AtomicBool,
    pending: AtomicBool,
    missing: AtomicUsize,
}

#[derive(Clone, Default)]
struct SharedAudioLevels {
    values: Arc<[AtomicU32; 2]>,
}

impl SharedAudioLevels {
    fn values(&self) -> [f32; 2] {
        self.values
            .each_ref()
            .map(|value| f32::from_bits(value.load(Ordering::Relaxed)))
    }

    fn store(&self, values: [f32; 2]) {
        for (slot, value) in self.values.iter().zip(values) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
    }
}

/// Callback state shared with the render worker. Device failure stops the
/// worker and invalidates its clock even if no more output callbacks arrive.
#[derive(Clone)]
struct AudioOutputState {
    played_sample_frames: Arc<AtomicU64>,
    events: Arc<Mutex<VecDeque<AudioPlaybackEvent>>>,
    underrun: Arc<AudioUnderrunState>,
    levels: SharedAudioLevels,
    stop: Arc<AtomicBool>,
}

impl AudioOutputState {
    fn device_failed(&self, error: String) {
        push_event(&self.events, AudioPlaybackEvent::DeviceFailed(error));
        self.stop.store(true, Ordering::Release);
    }
}

struct AudioPlaybackSession {
    stream: cpal::Stream,
    worker: AudioWorker,
    output: AudioOutputState,
    start_seconds: f64,
    sample_rate: u32,
    updates: Arc<Mutex<Option<AudioTimelineSnapshot>>>,
}

struct AudioWorker {
    handle: thread::JoinHandle<Result<(), AudioTimelineError>>,
    stop: Arc<AtomicBool>,
    events: Arc<Mutex<VecDeque<AudioPlaybackEvent>>>,
    underrun: Arc<AudioUnderrunState>,
    underrun_reported: bool,
}

impl AudioWorker {
    fn has_stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire) || self.handle.is_finished()
    }

    fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    fn poll_events(&mut self, out: &mut VecDeque<AudioPlaybackEvent>) {
        poll_underrun(&self.underrun, &mut self.underrun_reported, out);
        if let Ok(mut events) = self.events.lock() {
            out.extend(events.drain(..));
        }
    }

    fn finish(mut self, out: &mut VecDeque<AudioPlaybackEvent>) {
        if self.handle.join().is_err() {
            push_event(&self.events, AudioPlaybackEvent::WorkerPanicked);
        }
        // Joining consumes the handle; drain the remaining worker events afterward.
        poll_underrun(&self.underrun, &mut self.underrun_reported, out);
        if let Ok(mut events) = self.events.lock() {
            out.extend(events.drain(..));
        }
    }
}

pub(crate) struct AudioPlaybackEngine {
    timeline: Option<AudioTimelineSnapshot>,
    pending: bool,
    session: Option<AudioPlaybackSession>,
    retiring: Vec<AudioWorker>,
    media_readers: Arc<MediaReaderRegistry>,
    completed_events: VecDeque<AudioPlaybackEvent>,
}

impl AudioPlaybackEngine {
    pub(crate) fn new(media_readers: Arc<MediaReaderRegistry>) -> Self {
        Self {
            timeline: None,
            pending: false,
            session: None,
            retiring: Vec::new(),
            media_readers,
            completed_events: VecDeque::new(),
        }
    }

    pub(crate) fn play(
        &mut self,
        timeline: AudioTimelineSnapshot,
        start_frame: Frame,
    ) -> Result<PlaybackClock, AudioPlaybackError> {
        self.request_stop();
        self.timeline = Some(timeline.clone());
        self.start_or_defer(timeline, PlaybackPosition::Frame(start_frame))
    }

    fn start_or_defer(
        &mut self,
        timeline: AudioTimelineSnapshot,
        position: PlaybackPosition,
    ) -> Result<PlaybackClock, AudioPlaybackError> {
        if !self.retiring.is_empty() {
            self.pending = true;
            return Ok(PlaybackClock::Wall);
        }
        self.start(timeline, position)
    }

    fn start(
        &mut self,
        timeline: AudioTimelineSnapshot,
        position: PlaybackPosition,
    ) -> Result<PlaybackClock, AudioPlaybackError> {
        if timeline.sources().is_empty() {
            return Ok(PlaybackClock::Wall);
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
            &self.media_readers,
            AudioCalculationPolicy::Fallback,
        )
        .map_err(AudioPlaybackError::Timeline)?;
        if graph.is_empty() {
            return Ok(PlaybackClock::Wall);
        }
        let updates = Arc::new(Mutex::new(None));

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
        let prebuffer_target =
            u64::from(format.sample_rate).saturating_mul(PREBUFFER_MILLIS) / 1000;
        let capacity_frames = (capacity / channels.max(1)) as u64;
        while prebuffered_frames < prebuffer_target
            && prebuffered_frames.saturating_add(MIX_BLOCK_SAMPLE_FRAMES as u64) <= capacity_frames
        {
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

        let output = AudioOutputState {
            played_sample_frames: Arc::new(AtomicU64::new(0)),
            events: Arc::new(Mutex::new(VecDeque::with_capacity(16))),
            underrun: Arc::new(AudioUnderrunState {
                active: AtomicBool::new(false),
                pending: AtomicBool::new(false),
                missing: AtomicUsize::new(0),
            }),
            levels: SharedAudioLevels::default(),
            stop: Arc::new(AtomicBool::new(false)),
        };
        let stream = build_output_stream(&device, &supported, consumer, output.clone())?;
        let worker_stop = output.stop.clone();
        let worker_events = output.events.clone();
        let worker_played = output.played_sample_frames.clone();
        let worker_updates = updates.clone();
        let worker_readers = self.media_readers.clone();
        let worker = thread::Builder::new()
            .name("zerium-audio-render".to_owned())
            .spawn(move || {
                let mut cursor = render_cursor;
                while !worker_stop.load(Ordering::Acquire) {
                    let expected =
                        start_sample_frame.saturating_add(worker_played.load(Ordering::Acquire));
                    if expected > cursor {
                        cursor = expected;
                    }
                    let update = worker_updates
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .take();
                    let mixed = update
                        .map_or(Ok(false), |timeline| {
                            graph.update_timeline(timeline, &worker_readers)
                        })
                        .and_then(|_| graph.render(cursor, MIX_BLOCK_SAMPLE_FRAMES));
                    let mixed = match mixed {
                        Ok(mixed) => mixed,
                        Err(error) => {
                            push_event(
                                &worker_events,
                                AudioPlaybackEvent::DecodeFailed(error.clone()),
                            );
                            return Err(error);
                        }
                    };
                    for sample in mixed {
                        let mut pending = sample;
                        loop {
                            match producer.push(pending) {
                                Ok(()) => break,
                                Err(rtrb::PushError::Full(returned)) => {
                                    pending = returned;
                                    if worker_stop.load(Ordering::Acquire) {
                                        return Ok(());
                                    }
                                    thread::sleep(Duration::from_millis(1));
                                }
                            }
                        }
                    }
                    cursor = cursor.saturating_add(MIX_BLOCK_SAMPLE_FRAMES as u64);
                }
                Ok(())
            })
            .map_err(|error| {
                AudioPlaybackError::Worker(format!("Failed to start audio render thread: {error}"))
            })?;

        let worker = AudioWorker {
            handle: worker,
            stop: output.stop.clone(),
            events: output.events.clone(),
            underrun: output.underrun.clone(),
            underrun_reported: false,
        };
        if let Err(error) = stream.play() {
            worker.request_stop();
            drop(stream);
            self.retire_worker(worker);
            return Err(AudioPlaybackError::Stream(format!(
                "Failed to start audio playback: {error}"
            )));
        }
        self.session = Some(AudioPlaybackSession {
            stream,
            worker,
            output,
            start_seconds: start_sample_frame as f64 / f64::from(format.sample_rate),
            sample_rate: format.sample_rate,
            updates,
        });
        Ok(PlaybackClock::Audio)
    }

    /// Accept edits and resume deferred playback at the current cursor. Decoder
    /// rebuilding happens in the mixer worker; UI code only follows the clock.
    pub(crate) fn update_timeline(
        &mut self,
        timeline: AudioTimelineSnapshot,
        seconds: f64,
    ) -> Result<PlaybackClock, AudioPlaybackError> {
        self.reap_finished_workers();
        if self.session_stopped() {
            self.stop_session();
        }
        let changed = self
            .timeline
            .as_ref()
            .is_none_or(|previous| !previous.same_content(&timeline));
        if changed {
            self.timeline = Some(timeline.clone());
        }
        if self.pending {
            if !self.retiring.is_empty() {
                return Ok(PlaybackClock::Wall);
            }
            self.pending = false;
            return self.start(timeline, PlaybackPosition::Seconds(seconds));
        }
        if !changed {
            return Ok(if self.session.is_some() {
                PlaybackClock::Audio
            } else {
                PlaybackClock::Wall
            });
        }
        let Some(session) = &mut self.session else {
            return self.start_or_defer(timeline, PlaybackPosition::Seconds(seconds));
        };
        *session
            .updates
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(timeline);
        Ok(PlaybackClock::Audio)
    }

    pub(crate) fn request_stop(&mut self) -> bool {
        self.reap_finished_workers();
        let had_pending = std::mem::take(&mut self.pending);
        self.timeline = None;
        self.stop_session() || had_pending
    }

    /// Keep the requested revision after a worker failure, so an unchanged
    /// timeline does not repeatedly restart a failed decoder or output device.
    fn stop_session(&mut self) -> bool {
        let Some(session) = self.session.take() else {
            return false;
        };
        session.worker.request_stop();
        drop(session.stream);
        self.retire_worker(session.worker);
        true
    }

    fn retire_worker(&mut self, mut worker: AudioWorker) {
        worker.poll_events(&mut self.completed_events);
        if worker.handle.is_finished() {
            worker.finish(&mut self.completed_events);
        } else {
            self.retiring.push(worker);
        }
    }

    fn reap_finished_workers(&mut self) {
        let mut index = 0;
        while index < self.retiring.len() {
            let worker = &mut self.retiring[index];
            worker.poll_events(&mut self.completed_events);
            if worker.handle.is_finished() {
                self.retiring
                    .remove(index)
                    .finish(&mut self.completed_events);
            } else {
                index += 1;
            }
        }
    }

    pub(crate) fn take_events(&mut self) -> Vec<AudioPlaybackEvent> {
        if self.session_stopped() {
            self.stop_session();
        }
        self.reap_finished_workers();
        if let Some(session) = self.session.as_mut() {
            session.worker.poll_events(&mut self.completed_events);
        }
        self.completed_events.drain(..).collect()
    }

    fn session_stopped(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.worker.has_stopped())
    }

    pub(crate) fn playhead_seconds(&self) -> Option<f64> {
        let session = self.session.as_ref()?;
        if session.worker.has_stopped() {
            return None;
        }
        let played = session.output.played_sample_frames.load(Ordering::Acquire);
        Some(session.start_seconds + played as f64 / f64::from(session.sample_rate))
    }

    pub(crate) fn levels(&self) -> [f32; 2] {
        self.session
            .as_ref()
            .filter(|session| !session.worker.has_stopped())
            .map(|session| session.output.levels.values())
            .unwrap_or([0.; 2])
    }
}

impl Drop for AudioPlaybackEngine {
    fn drop(&mut self) {
        self.request_stop();
    }
}

fn build_output_stream(
    device: &cpal::Device,
    supported: &cpal::SupportedStreamConfig,
    consumer: rtrb::Consumer<f32>,
    state: AudioOutputState,
) -> Result<cpal::Stream, AudioPlaybackError> {
    macro_rules! build_stream {
        ($sample:ty) => {
            build_output_stream_for_format::<$sample>(device, supported, consumer, state)
        };
    }
    match supported.sample_format() {
        cpal::SampleFormat::I8 => build_stream!(i8),
        cpal::SampleFormat::I16 => build_stream!(i16),
        cpal::SampleFormat::I24 => build_stream!(cpal::I24),
        cpal::SampleFormat::I32 => build_stream!(i32),
        cpal::SampleFormat::I64 => build_stream!(i64),
        cpal::SampleFormat::U8 => build_stream!(u8),
        cpal::SampleFormat::U16 => build_stream!(u16),
        cpal::SampleFormat::U24 => build_stream!(cpal::U24),
        cpal::SampleFormat::U32 => build_stream!(u32),
        cpal::SampleFormat::U64 => build_stream!(u64),
        cpal::SampleFormat::F32 => build_stream!(f32),
        cpal::SampleFormat::F64 => build_stream!(f64),
        format => Err(AudioPlaybackError::Configuration(format!(
            "Unsupported audio output sample format: {format:?}"
        ))),
    }
}

fn build_output_stream_for_format<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    supported: &cpal::SupportedStreamConfig,
    mut consumer: rtrb::Consumer<f32>,
    state: AudioOutputState,
) -> Result<cpal::Stream, AudioPlaybackError> {
    let errors = state.clone();
    let config = supported.config();
    let channels = usize::from(config.channels).max(1);
    device
        .build_output_stream(
            config,
            move |output: &mut [T], _| {
                fill_output(output, channels, &mut consumer, &state, |sample| {
                    T::from_sample(sample.clamp(-1., 1.))
                })
            },
            move |error| {
                errors.device_failed(error.to_string());
            },
            None,
        )
        .map_err(|error| {
            AudioPlaybackError::Stream(format!("Failed to create audio output: {error}"))
        })
}

fn fill_output<T: Copy>(
    output: &mut [T],
    channels: usize,
    consumer: &mut rtrb::Consumer<f32>,
    state: &AudioOutputState,
    convert: impl Fn(f32) -> T,
) {
    let queued_samples = consumer.slots().min(output.len());
    let queued_samples = queued_samples - queued_samples % channels;
    let mut peak = [0_f32; 2];
    for (index, output_sample) in output.iter_mut().enumerate() {
        let sample = if index < queued_samples {
            consumer.pop().unwrap_or(0.)
        } else {
            0.
        };
        let channel = index % channels;
        if channel < peak.len() {
            peak[channel] = peak[channel].max(sample.abs());
        }
        *output_sample = convert(sample);
    }
    if channels == 1 {
        peak[1] = peak[0];
    }
    state.levels.store(peak);
    if queued_samples < output.len() {
        let missing = (output.len() - queued_samples).div_ceil(channels);
        state.underrun.missing.fetch_add(missing, Ordering::Relaxed);
        state.underrun.active.store(true, Ordering::Relaxed);
        state.underrun.pending.store(true, Ordering::Release);
    } else {
        state.underrun.active.store(false, Ordering::Release);
    }
    state
        .played_sample_frames
        .fetch_add((output.len() / channels) as u64, Ordering::Release);
}

fn poll_underrun(
    state: &AudioUnderrunState,
    reported: &mut bool,
    out: &mut VecDeque<AudioPlaybackEvent>,
) {
    let had = state.pending.swap(false, Ordering::AcqRel);
    let missing = state.missing.swap(0, Ordering::AcqRel);
    let active = state.active.load(Ordering::Acquire);
    if had && !*reported {
        out.push_back(AudioPlaybackEvent::Underrun {
            missing_sample_frames: missing,
        });
        *reported = true;
    }
    if *reported && !active {
        out.push_back(AudioPlaybackEvent::Recovered);
        *reported = false;
    }
}

fn push_event(events: &Mutex<VecDeque<AudioPlaybackEvent>>, event: AudioPlaybackEvent) {
    if let Ok(mut events) = events.lock() {
        events.push_back(event);
    }
}
