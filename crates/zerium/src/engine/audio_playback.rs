use std::{
    collections::{HashMap, VecDeque},
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
    AudioClipId, AudioFormat, AudioGainEvaluation, AudioTimelineError, AudioTimelineGraph,
    MediaReaderRegistry,
};
use zerium_core::media::{MediaAsset, MediaPlayback};
use zerium_core::timeline::{Frame, FrameDuration, FrameRate, ItemId, TimelineItem, TimelineTime};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AudioSourcePlan {
    item_id: ItemId,
    input_id: String,
    asset: MediaAsset,
    start: Frame,
    duration: FrameDuration,
    playback: MediaPlayback,
    preserve_pitch: bool,
}

pub(crate) fn audio_plan(items: &[TimelineItem]) -> Vec<AudioSourcePlan> {
    let mut plan = Vec::new();
    for item in items {
        let Some(schema) = item.schema() else {
            continue;
        };
        for input in schema.audio() {
            if let Some((asset, playback, preserve_pitch)) = item.audio_input(input.id()) {
                plan.push(AudioSourcePlan {
                    item_id: item.id,
                    input_id: input.id().to_owned(),
                    asset,
                    start: item.start,
                    duration: item.duration,
                    playback,
                    preserve_pitch,
                });
            }
        }
    }
    plan.sort_unstable_by(|a, b| {
        (a.item_id.get(), &a.input_id).cmp(&(b.item_id.get(), &b.input_id))
    });
    plan
}

const AUDIO_BUFFER_MILLIS: u64 = 200;
const MIX_BLOCK_SAMPLE_FRAMES: usize = 2_048;
const PREBUFFER_MILLIS: u64 = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaybackClock {
    Audio,
    Wall,
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

struct AudioPlaybackSession {
    stream: Option<cpal::Stream>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<Result<(), AudioTimelineError>>>,
    played_sample_frames: Arc<AtomicU64>,
    start_seconds: f64,
    sample_rate: u32,
    levels: SharedAudioLevels,
    gains: HashMap<AudioClipId, Arc<AtomicU32>>,
    events: Arc<Mutex<VecDeque<AudioPlaybackEvent>>>,
    underrun: Arc<AudioUnderrunState>,
    underrun_reported: bool,
}

struct RetiringAudioWorker {
    worker: Option<thread::JoinHandle<Result<(), AudioTimelineError>>>,
    events: Arc<Mutex<VecDeque<AudioPlaybackEvent>>>,
    underrun: Arc<AudioUnderrunState>,
    underrun_reported: bool,
}

struct PendingAudioPlayback {
    items: Vec<TimelineItem>,
    frame_rate: FrameRate,
}

pub(crate) struct AudioPlaybackEngine {
    pending: Option<PendingAudioPlayback>,
    session: Option<AudioPlaybackSession>,
    retiring: Vec<RetiringAudioWorker>,
    media_readers: Arc<MediaReaderRegistry>,
    completed_events: VecDeque<AudioPlaybackEvent>,
}

impl AudioPlaybackEngine {
    pub(crate) fn new(media_readers: Arc<MediaReaderRegistry>) -> Self {
        Self {
            pending: None,
            session: None,
            retiring: Vec::new(),
            media_readers,
            completed_events: VecDeque::new(),
        }
    }

    pub(crate) fn play(
        &mut self,
        items: Vec<TimelineItem>,
        start_frame: Frame,
        frame_rate: FrameRate,
    ) -> Result<PlaybackClock, AudioPlaybackError> {
        self.request_stop();
        if !self.retiring.is_empty() {
            self.pending = Some(PendingAudioPlayback { items, frame_rate });
            return Ok(PlaybackClock::Wall);
        }
        self.start(items, frame_rate.frame_to_seconds(start_frame), frame_rate)
    }

    pub(crate) fn resume_pending(
        &mut self,
        seconds: f64,
    ) -> Option<Result<PlaybackClock, AudioPlaybackError>> {
        let pending = self.take_pending_if_ready()?;
        Some(self.start(pending.items, seconds, pending.frame_rate))
    }

    fn take_pending_if_ready(&mut self) -> Option<PendingAudioPlayback> {
        self.reap_finished_workers();
        if !self.retiring.is_empty() {
            return None;
        }
        self.pending.take()
    }

    fn start(
        &mut self,
        items: Vec<TimelineItem>,
        start_seconds: f64,
        frame_rate: FrameRate,
    ) -> Result<PlaybackClock, AudioPlaybackError> {
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
            sample_rate: supported.sample_rate().0,
            channels: supported.channels(),
        };
        let seed_time = TimelineTime::from_frames(start_seconds * frame_rate.frames_per_second());
        let mut graph = AudioTimelineGraph::new(
            &items,
            frame_rate,
            format,
            &self.media_readers,
            AudioGainEvaluation::Live,
        )
        .map_err(AudioPlaybackError::Timeline)?;
        if graph.is_empty() {
            return Ok(PlaybackClock::Wall);
        }
        let gains = graph.live_gains();
        update_gain_slots(&gains, &items, seed_time);

        let channels = usize::from(format.channels);
        let capacity = u64::from(format.sample_rate)
            .saturating_mul(channels as u64)
            .saturating_mul(AUDIO_BUFFER_MILLIS)
            .checked_div(1000)
            .and_then(|samples| usize::try_from(samples).ok())
            .unwrap_or(channels)
            .max(channels);
        let (mut producer, consumer) = rtrb::RingBuffer::<f32>::new(capacity);
        let start_sample_frame = (start_seconds * f64::from(format.sample_rate)).round() as u64;
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

        let played_sample_frames = Arc::new(AtomicU64::new(0));
        let events = Arc::new(Mutex::new(VecDeque::with_capacity(16)));
        let underrun = Arc::new(AudioUnderrunState {
            active: AtomicBool::new(false),
            pending: AtomicBool::new(false),
            missing: AtomicUsize::new(0),
        });
        let levels = SharedAudioLevels::default();
        let stream = build_output_stream(
            &device,
            &supported,
            consumer,
            played_sample_frames.clone(),
            events.clone(),
            underrun.clone(),
            levels.clone(),
        )?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_events = events.clone();
        let worker_played = played_sample_frames.clone();
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
                    let mixed = match graph.render(cursor, MIX_BLOCK_SAMPLE_FRAMES) {
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

        if let Err(error) = stream.play() {
            stop.store(true, Ordering::Release);
            self.retire_worker(worker, events, underrun, false);
            return Err(AudioPlaybackError::Stream(format!(
                "Failed to start audio playback: {error}"
            )));
        }
        self.session = Some(AudioPlaybackSession {
            stream: Some(stream),
            stop,
            worker: Some(worker),
            played_sample_frames,
            start_seconds: start_sample_frame as f64 / f64::from(format.sample_rate),
            sample_rate: format.sample_rate,
            levels,
            gains,
            events,
            underrun,
            underrun_reported: false,
        });
        Ok(PlaybackClock::Audio)
    }

    pub(crate) fn update_gains(
        &mut self,
        items: &[TimelineItem],
        time_seconds: f64,
        frame_rate: FrameRate,
    ) {
        self.reap_finished_workers();
        if self.worker_finished() {
            self.request_stop();
            return;
        }
        let Some(session) = &self.session else {
            return;
        };
        let time = TimelineTime::from_frames(time_seconds * frame_rate.frames_per_second());
        update_gain_slots(&session.gains, items, time);
    }

    pub(crate) fn request_stop(&mut self) -> bool {
        self.reap_finished_workers();
        let had_pending = self.pending.take().is_some();
        let Some(mut session) = self.session.take() else {
            return had_pending;
        };
        session.stop.store(true, Ordering::Release);
        drop(session.stream.take());
        poll_underrun(
            &session.underrun,
            &mut session.underrun_reported,
            &mut self.completed_events,
        );
        if let Ok(mut events) = session.events.lock() {
            self.completed_events.extend(events.drain(..));
        }
        if let Some(worker) = session.worker.take() {
            self.retire_worker(
                worker,
                session.events,
                session.underrun,
                session.underrun_reported,
            );
        }
        true
    }

    fn retire_worker(
        &mut self,
        worker: thread::JoinHandle<Result<(), AudioTimelineError>>,
        events: Arc<Mutex<VecDeque<AudioPlaybackEvent>>>,
        underrun: Arc<AudioUnderrunState>,
        mut underrun_reported: bool,
    ) {
        poll_underrun(
            &underrun,
            &mut underrun_reported,
            &mut self.completed_events,
        );
        if worker.is_finished() {
            match worker.join() {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {}
                Err(_) => push_event(&events, AudioPlaybackEvent::WorkerPanicked),
            }
            if let Ok(mut pending) = events.lock() {
                self.completed_events.extend(pending.drain(..));
            }
            return;
        }
        self.retiring.push(RetiringAudioWorker {
            worker: Some(worker),
            events,
            underrun,
            underrun_reported,
        });
    }

    fn reap_finished_workers(&mut self) {
        let mut index = 0;
        while index < self.retiring.len() {
            let underrun = self.retiring[index].underrun.clone();
            let mut reported = self.retiring[index].underrun_reported;
            poll_underrun(&underrun, &mut reported, &mut self.completed_events);
            self.retiring[index].underrun_reported = reported;
            if let Ok(mut pending) = self.retiring[index].events.lock() {
                self.completed_events.extend(pending.drain(..));
            }
            let finished = self.retiring[index]
                .worker
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished);
            if !finished {
                index += 1;
                continue;
            }
            let mut retiring = self.retiring.remove(index);
            if let Some(worker) = retiring.worker.take() {
                match worker.join() {
                    Ok(Ok(())) => {}
                    Ok(Err(_)) => {}
                    Err(_) => push_event(&retiring.events, AudioPlaybackEvent::WorkerPanicked),
                }
            }
            if let Ok(mut pending) = retiring.events.lock() {
                self.completed_events.extend(pending.drain(..));
            }
        }
    }

    pub(crate) fn take_events(&mut self) -> Vec<AudioPlaybackEvent> {
        if self.worker_finished() {
            self.request_stop();
        }
        self.reap_finished_workers();
        if let Some(session) = self.session.as_mut() {
            poll_underrun(
                &session.underrun,
                &mut session.underrun_reported,
                &mut self.completed_events,
            );
            if let Ok(mut events) = session.events.lock() {
                self.completed_events.extend(events.drain(..));
            }
        }
        self.completed_events.drain(..).collect()
    }

    fn worker_finished(&self) -> bool {
        self.session
            .as_ref()
            .and_then(|session| session.worker.as_ref())
            .is_some_and(thread::JoinHandle::is_finished)
    }

    pub(crate) fn playhead_seconds(&self) -> Option<f64> {
        let session = self.session.as_ref()?;
        let played = session.played_sample_frames.load(Ordering::Acquire);
        Some(session.start_seconds + played as f64 / f64::from(session.sample_rate))
    }

    pub(crate) fn levels(&self) -> [f32; 2] {
        self.session
            .as_ref()
            .map(|session| session.levels.values())
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
    played_sample_frames: Arc<AtomicU64>,
    events: Arc<Mutex<VecDeque<AudioPlaybackEvent>>>,
    underrun: Arc<AudioUnderrunState>,
    levels: SharedAudioLevels,
) -> Result<cpal::Stream, AudioPlaybackError> {
    macro_rules! build_stream {
        ($sample:ty) => {
            build_output_stream_for_format::<$sample>(
                device,
                supported,
                consumer,
                played_sample_frames,
                events,
                underrun,
                levels,
            )
        };
    }
    match supported.sample_format() {
        cpal::SampleFormat::I8 => build_stream!(i8),
        cpal::SampleFormat::I16 => build_stream!(i16),
        cpal::SampleFormat::I32 => build_stream!(i32),
        cpal::SampleFormat::I64 => build_stream!(i64),
        cpal::SampleFormat::U8 => build_stream!(u8),
        cpal::SampleFormat::U16 => build_stream!(u16),
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
    played_sample_frames: Arc<AtomicU64>,
    events: Arc<Mutex<VecDeque<AudioPlaybackEvent>>>,
    underrun: Arc<AudioUnderrunState>,
    levels: SharedAudioLevels,
) -> Result<cpal::Stream, AudioPlaybackError> {
    let config = supported.config();
    let channels = usize::from(config.channels).max(1);
    device
        .build_output_stream(
            &config,
            move |output: &mut [T], _| {
                fill_output(
                    output,
                    channels,
                    &mut consumer,
                    &played_sample_frames,
                    &underrun,
                    &levels,
                    |sample| T::from_sample(sample.clamp(-1., 1.)),
                )
            },
            move |error| {
                push_event(&events, AudioPlaybackEvent::DeviceFailed(error.to_string()));
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
    played_sample_frames: &AtomicU64,
    underrun: &AudioUnderrunState,
    levels: &SharedAudioLevels,
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
    levels.store(peak);
    if queued_samples < output.len() {
        let missing = (output.len() - queued_samples).div_ceil(channels);
        underrun.missing.fetch_add(missing, Ordering::Relaxed);
        underrun.active.store(true, Ordering::Relaxed);
        underrun.pending.store(true, Ordering::Release);
    } else {
        underrun.active.store(false, Ordering::Release);
    }
    played_sample_frames.fetch_add((output.len() / channels) as u64, Ordering::Release);
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

/// Update each input independently while evaluating animation once per item.
fn update_gain_slots(
    gains: &HashMap<AudioClipId, Arc<AtomicU32>>,
    items: &[TimelineItem],
    time: TimelineTime,
) {
    for item in items {
        let Some(schema) = item.schema() else {
            continue;
        };
        if schema.audio().is_empty() {
            continue;
        }
        let evaluated = item.evaluated_at_time(time);
        for input in schema.audio() {
            let id = AudioClipId {
                item_id: item.id,
                input_id: input.id().to_owned(),
            };
            if let Some(slot) = gains.get(&id) {
                slot.store(
                    evaluated
                        .audio_gain(input.id())
                        .expect("validated audio input")
                        .to_bits(),
                    Ordering::Relaxed,
                );
            }
        }
    }
}
