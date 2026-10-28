//! A device stream and its mixer form one playback session. Retired workers
//! keep only their completion state after the stream has been dropped.
use super::{
    AudioPlaybackError, AudioPlaybackEvent, MIX_BLOCK_SAMPLE_FRAMES,
    output::{AudioOutputState, build_output_stream},
    preparation::PreparedAudio,
};
use crate::engine::processing::audio::{AudioTimelineError, PlaybackRate};
use cpal::traits::StreamTrait as _;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use zerium_core::timeline::AudioTimelineSnapshot;

pub(super) struct AudioPlaybackSession {
    stream: cpal::Stream,
    worker: AudioWorker,
    start_sample_frame: u64,
    sample_rate: u32,
    rate: PlaybackRate,
    updates: Arc<Mutex<Option<AudioTimelineSnapshot>>>,
}

pub(super) struct AudioWorker {
    handle: thread::JoinHandle<Result<(), AudioTimelineError>>,
    output: Arc<AudioOutputState>,
}

impl AudioPlaybackSession {
    pub(super) fn new(
        prepared: PreparedAudio,
        timeline: AudioTimelineSnapshot,
    ) -> Result<Self, AudioPlaybackError> {
        let PreparedAudio {
            rate,
            device,
            supported,
            mut graph,
            mut producer,
            consumer,
            start_sample_frame,
            render_cursor,
        } = prepared;
        // Edits received during preparation are applied by the mixer worker.
        let updates = Arc::new(Mutex::new(Some(timeline)));
        let output = Arc::new(AudioOutputState::default());
        let stream = build_output_stream(&device, &supported, consumer, output.clone())?;
        let worker_output = output.clone();
        let worker_updates = updates.clone();
        let worker = thread::Builder::new()
            .name("zerium-audio-render".to_owned())
            .spawn(move || {
                let mut cursor = render_cursor;
                while !worker_output.is_stopped() {
                    let expected =
                        start_sample_frame.saturating_add(worker_output.played_sample_frames());
                    if expected > cursor {
                        cursor = expected;
                    }
                    let update = worker_updates
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .take();
                    let mixed = update
                        .map_or(Ok(false), |timeline| graph.update_timeline(timeline))
                        .and_then(|_| graph.render(cursor, MIX_BLOCK_SAMPLE_FRAMES));
                    let mixed = mixed?;
                    for sample in mixed {
                        let mut pending = sample;
                        loop {
                            match producer.push(pending) {
                                Ok(()) => break,
                                Err(rtrb::PushError::Full(returned)) => {
                                    pending = returned;
                                    if worker_output.is_stopped() {
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

        Ok(Self {
            stream,
            worker: AudioWorker {
                handle: worker,
                output,
            },
            start_sample_frame,
            sample_rate: supported.sample_rate(),
            rate,
            updates,
        })
    }

    pub(super) fn play(&self) -> Result<(), AudioPlaybackError> {
        self.stream.play().map_err(|error| {
            AudioPlaybackError::Stream(format!("Failed to start audio playback: {error}"))
        })
    }

    pub(super) fn has_stopped(&self) -> bool {
        self.worker.has_stopped()
    }

    pub(super) fn update_timeline(&self, timeline: AudioTimelineSnapshot) {
        *self
            .updates
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(timeline);
    }

    pub(super) fn poll_events(&self, events: &mut VecDeque<AudioPlaybackEvent>) {
        self.worker.poll_events(events);
    }

    pub(super) fn retire(self) -> AudioWorker {
        self.worker.output.request_stop();
        drop(self.stream);
        self.worker
    }

    pub(super) fn playhead_seconds(&self) -> Option<f64> {
        if self.has_stopped() {
            return None;
        }
        Some(
            (self.start_sample_frame as f64 + self.worker.output.played_sample_frames() as f64)
                * f64::from(self.rate.multiplier())
                / f64::from(self.sample_rate),
        )
    }

    pub(super) fn levels(&self) -> [f32; 2] {
        if self.has_stopped() {
            [0.; 2]
        } else {
            self.worker.output.levels()
        }
    }
}

impl AudioWorker {
    fn has_stopped(&self) -> bool {
        self.output.is_stopped() || self.is_finished()
    }

    pub(super) fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    pub(super) fn poll_events(&self, out: &mut VecDeque<AudioPlaybackEvent>) {
        self.output.poll_events(out);
    }

    pub(super) fn finish(self, out: &mut VecDeque<AudioPlaybackEvent>) {
        match self.handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => self
                .output
                .push_event(AudioPlaybackEvent::DecodeFailed(error)),
            Err(_) => self.output.push_event(AudioPlaybackEvent::WorkerPanicked),
        }
        self.output.poll_events(out);
    }
}
