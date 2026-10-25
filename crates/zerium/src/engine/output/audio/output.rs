//! Real-time device output and the state shared with its mixer worker.
use super::{AudioPlaybackError, AudioPlaybackEvent};
use cpal::traits::DeviceTrait as _;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
};

/// One shared allocation owns the device clock, metering, cancellation and events.
/// Device failure stops the mixer even when no more output callbacks arrive.
#[derive(Default)]
pub(super) struct AudioOutputState {
    played_sample_frames: AtomicU64,
    events: Mutex<VecDeque<AudioPlaybackEvent>>,
    levels: [AtomicU32; 2],
    stopped: AtomicBool,
}

impl AudioOutputState {
    pub(super) fn played_sample_frames(&self) -> u64 {
        self.played_sample_frames.load(Ordering::Acquire)
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    pub(super) fn request_stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }

    pub(super) fn levels(&self) -> [f32; 2] {
        self.levels
            .each_ref()
            .map(|value| f32::from_bits(value.load(Ordering::Relaxed)))
    }

    fn store_levels(&self, levels: [f32; 2]) {
        for (slot, value) in self.levels.iter().zip(levels) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
    }

    pub(super) fn push_event(&self, event: AudioPlaybackEvent) {
        self.events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push_back(event);
    }

    pub(super) fn poll_events(&self, out: &mut VecDeque<AudioPlaybackEvent>) {
        out.extend(
            self.events
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .drain(..),
        );
    }

    fn device_failed(&self, error: String) {
        self.push_event(AudioPlaybackEvent::DeviceFailed(error));
        self.request_stop();
    }
}

pub(super) fn build_output_stream(
    device: &cpal::Device,
    supported: &cpal::SupportedStreamConfig,
    consumer: rtrb::Consumer<f32>,
    state: Arc<AudioOutputState>,
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
    state: Arc<AudioOutputState>,
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
    state.store_levels(peak);
    state
        .played_sample_frames
        .fetch_add((output.len() / channels) as u64, Ordering::Release);
}
