use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};

use crate::{
    domain::timeline::{Frame, FrameRate, ItemId, TimelineItem},
    engine::media::{
        AudioFormat, AudioGainEvaluation, AudioTimelineError, AudioTimelineGraph,
        MediaReaderRegistry, sample_boundary,
    },
};

const AUDIO_BUFFER_SECONDS: usize = 2;
const MIX_BLOCK_SAMPLE_FRAMES: usize = 2_048;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AudioPlaybackError {
    DeviceUnavailable,
    Configuration(String),
    Stream(String),
    Worker(String),
    Timeline(AudioTimelineError),
}

impl fmt::Display for AudioPlaybackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeviceUnavailable => formatter.write_str("音声出力デバイスが見つかりません"),
            Self::Configuration(message) | Self::Stream(message) | Self::Worker(message) => {
                formatter.write_str(message)
            }
            Self::Timeline(error) => error.fmt(formatter),
        }
    }
}

impl Error for AudioPlaybackError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Timeline(error) => Some(error),
            Self::DeviceUnavailable
            | Self::Configuration(_)
            | Self::Stream(_)
            | Self::Worker(_) => None,
        }
    }
}

struct AudioUnderrunState {
    active: AtomicBool,
    pending: AtomicBool,
    missing: AtomicUsize,
}

struct AudioPlaybackSession {
    stream: Option<cpal::Stream>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<Result<(), AudioTimelineError>>>,
    played_sample_frames: Arc<AtomicU64>,
    start_frame: Frame,
    sample_rate: u32,
    gains: HashMap<ItemId, Arc<AtomicU32>>,
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

pub(crate) struct AudioPlaybackEngine {
    session: Option<AudioPlaybackSession>,
    retiring: Vec<RetiringAudioWorker>,
    media_readers: Arc<MediaReaderRegistry>,
    completed_events: VecDeque<AudioPlaybackEvent>,
}

impl AudioPlaybackEngine {
    pub(crate) fn new(media_readers: Arc<MediaReaderRegistry>) -> Self {
        Self {
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
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(AudioPlaybackError::DeviceUnavailable)?;
        let supported = device.default_output_config().map_err(|error| {
            AudioPlaybackError::Configuration(format!("音声出力設定を取得できません: {error}"))
        })?;
        let format = AudioFormat {
            sample_rate: supported.sample_rate().0,
            channels: supported.channels(),
        };
        let mut graph = AudioTimelineGraph::new(
            items,
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

        let channels = usize::from(format.channels);
        let capacity = usize::try_from(format.sample_rate)
            .unwrap_or(48_000)
            .saturating_mul(channels)
            .saturating_mul(AUDIO_BUFFER_SECONDS)
            .max(channels);
        let (mut producer, consumer) = rtrb::RingBuffer::<f32>::new(capacity);
        let start_sample_frame = sample_boundary(start_frame.get(), frame_rate, format.sample_rate);
        let initial = graph
            .render(start_sample_frame, MIX_BLOCK_SAMPLE_FRAMES)
            .map_err(AudioPlaybackError::Timeline)?;
        for sample in initial {
            producer.push(sample).map_err(|_| {
                AudioPlaybackError::Worker("初期音声バッファが不足しています".to_owned())
            })?;
        }

        let played_sample_frames = Arc::new(AtomicU64::new(0));
        let events = Arc::new(Mutex::new(VecDeque::with_capacity(16)));
        let underrun = Arc::new(AudioUnderrunState {
            active: AtomicBool::new(false),
            pending: AtomicBool::new(false),
            missing: AtomicUsize::new(0),
        });
        let stream = build_output_stream(
            &device,
            &supported,
            consumer,
            played_sample_frames.clone(),
            events.clone(),
            underrun.clone(),
        )?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_events = events.clone();
        let worker = thread::Builder::new()
            .name("zerium-audio-render".to_owned())
            .spawn(move || {
                let mut cursor = start_sample_frame.saturating_add(MIX_BLOCK_SAMPLE_FRAMES as u64);
                while !worker_stop.load(Ordering::Acquire) {
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
                AudioPlaybackError::Worker(format!("音声レンダースレッドを開始できません: {error}"))
            })?;

        if let Err(error) = stream.play() {
            stop.store(true, Ordering::Release);
            self.retire_worker(worker, events, underrun, false);
            return Err(AudioPlaybackError::Stream(format!(
                "音声再生を開始できません: {error}"
            )));
        }
        self.session = Some(AudioPlaybackSession {
            stream: Some(stream),
            stop,
            worker: Some(worker),
            played_sample_frames,
            start_frame,
            sample_rate: format.sample_rate,
            gains,
            events,
            underrun,
            underrun_reported: false,
        });
        Ok(PlaybackClock::Audio)
    }

    pub(crate) fn update_gains(&mut self, items: &[TimelineItem]) {
        self.reap_finished_workers();
        if self.worker_finished() {
            self.request_stop();
            return;
        }
        let Some(session) = &self.session else {
            return;
        };
        for item in items {
            if let Some(gain) = session.gains.get(&item.id) {
                gain.store(item.audio_gain().to_bits(), Ordering::Relaxed);
            }
        }
    }

    pub(crate) fn request_stop(&mut self) -> bool {
        self.reap_finished_workers();
        let Some(mut session) = self.session.take() else {
            return false;
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

    pub(crate) fn playhead_seconds(&self, frame_rate: FrameRate) -> Option<f64> {
        let session = self.session.as_ref()?;
        let played = session.played_sample_frames.load(Ordering::Acquire);
        Some(
            frame_rate.frame_to_seconds(session.start_frame)
                + played as f64 / f64::from(session.sample_rate),
        )
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
) -> Result<cpal::Stream, AudioPlaybackError> {
    let config = supported.config();
    let channels = usize::from(config.channels).max(1);
    let error_events = events.clone();
    let error_callback = move |error: cpal::StreamError| {
        push_event(
            &error_events,
            AudioPlaybackEvent::DeviceFailed(error.to_string()),
        );
    };
    match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            let mut consumer = consumer;
            let underrun = underrun.clone();
            device
                .build_output_stream(
                    &config,
                    move |output: &mut [f32], _| {
                        fill_output(
                            output,
                            channels,
                            &mut consumer,
                            &played_sample_frames,
                            &underrun,
                            |sample| sample,
                        )
                    },
                    error_callback,
                    None,
                )
                .map_err(|error| {
                    AudioPlaybackError::Stream(format!("音声出力を作成できません: {error}"))
                })
        }
        cpal::SampleFormat::I16 => {
            let mut consumer = consumer;
            let underrun = underrun.clone();
            device
                .build_output_stream(
                    &config,
                    move |output: &mut [i16], _| {
                        fill_output(
                            output,
                            channels,
                            &mut consumer,
                            &played_sample_frames,
                            &underrun,
                            |sample| (sample * f32::from(i16::MAX)) as i16,
                        )
                    },
                    error_callback,
                    None,
                )
                .map_err(|error| {
                    AudioPlaybackError::Stream(format!("音声出力を作成できません: {error}"))
                })
        }
        cpal::SampleFormat::U16 => {
            let mut consumer = consumer;
            let underrun = underrun.clone();
            device
                .build_output_stream(
                    &config,
                    move |output: &mut [u16], _| {
                        fill_output(
                            output,
                            channels,
                            &mut consumer,
                            &played_sample_frames,
                            &underrun,
                            |sample| ((sample * 0.5 + 0.5) * f32::from(u16::MAX)) as u16,
                        )
                    },
                    error_callback,
                    None,
                )
                .map_err(|error| {
                    AudioPlaybackError::Stream(format!("音声出力を作成できません: {error}"))
                })
        }
        format => Err(AudioPlaybackError::Configuration(format!(
            "未対応の音声出力サンプル形式です: {format:?}"
        ))),
    }
}

fn fill_output<T: Copy>(
    output: &mut [T],
    channels: usize,
    consumer: &mut rtrb::Consumer<f32>,
    played_sample_frames: &AtomicU64,
    underrun: &AudioUnderrunState,
    convert: impl Fn(f32) -> T,
) {
    let queued_samples = consumer.slots().min(output.len());
    let queued_samples = queued_samples - queued_samples % channels;
    for (index, output_sample) in output.iter_mut().enumerate() {
        let sample = if index < queued_samples {
            consumer.pop().unwrap_or(0.)
        } else {
            0.
        };
        *output_sample = convert(sample);
    }
    if queued_samples < output.len() {
        let missing = (output.len() - queued_samples).div_ceil(channels);
        underrun.missing.fetch_add(missing, Ordering::Relaxed);
        underrun.pending.store(true, Ordering::Release);
        underrun.active.store(true, Ordering::Relaxed);
    } else {
        underrun.active.store(false, Ordering::Relaxed);
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
