use rust_i18n::t;
use std::time::Instant;

use gpui::{App, Context, Entity};

use crate::{
    engine::audio_playback::{
        AudioPlaybackEngine, AudioPlaybackError, AudioPlaybackEvent, AudioSourcePlan,
        PlaybackClock, audio_plan,
    },
    engine::video_playback::VideoPlaybackMode,
};
use zerium_core::timeline::{Frame, FrameRate, TimelineEditor};

use super::{TimelineEditorEntityExt, session::UiNotifications};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrubSource {
    Timeline,
    AnimationCurve,
    Preview,
}

#[derive(Clone, Copy, Debug)]
struct PlaybackSession {
    start_seconds: f64,
    started_at: Instant,
    uses_audio_clock: bool,
}

#[derive(Clone, Copy, Debug)]
enum TransportMode {
    Stopped,
    Playing(PlaybackSession),
    Scrubbing(ScrubSource),
}

pub(crate) struct TransportController {
    editor: Entity<TimelineEditor>,
    audio: Entity<AudioPlaybackEngine>,
    notifications: Entity<UiNotifications>,
    mode: TransportMode,
    audio_plan: Vec<AudioSourcePlan>,
    audio_frame_rate: FrameRate,
}

impl TransportController {
    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        audio: Entity<AudioPlaybackEngine>,
        notifications: Entity<UiNotifications>,
    ) -> Self {
        Self {
            editor,
            audio,
            notifications,
            mode: TransportMode::Stopped,
            audio_plan: Vec::new(),
            audio_frame_rate: FrameRate::FPS_30,
        }
    }

    pub(crate) fn is_playing(&self) -> bool {
        matches!(self.mode, TransportMode::Playing(_))
    }

    pub(crate) fn playback_mode(&self) -> VideoPlaybackMode {
        match self.mode {
            TransportMode::Stopped => VideoPlaybackMode::Idle,
            TransportMode::Scrubbing(_) => VideoPlaybackMode::Scrubbing,
            TransportMode::Playing(_) => VideoPlaybackMode::Playing,
        }
    }

    pub(crate) fn audio_levels(&self, cx: &App) -> [f32; 2] {
        self.audio.read(cx).levels()
    }

    pub(crate) fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        if self.is_playing() {
            self.stop(cx);
        } else {
            self.play(cx);
        }
    }

    fn play(&mut self, cx: &mut Context<Self>) {
        self.stop_audio(cx);
        let (start_frame, frame_rate, items) = {
            let editor = self.editor.read(cx);
            let playhead = editor.playhead();
            let start_frame = if playhead >= editor.end_frame_exclusive() {
                Frame::new(0)
            } else {
                playhead
            };
            (start_frame, editor.frame_rate(), editor.visible_items())
        };
        let clock: Result<PlaybackClock, AudioPlaybackError> = self.audio.update(cx, |audio, _| {
            audio.play(items.clone(), start_frame, frame_rate)
        });
        self.audio_plan = audio_plan(&items);
        self.audio_frame_rate = frame_rate;
        let clock = match clock {
            Ok(clock) => clock,
            Err(error) => {
                self.notifications.update(cx, |notifications, cx| {
                    notifications.push(
                        t!("transport.start_playback_failed", error = error).to_string(),
                        cx,
                    );
                });
                PlaybackClock::Wall
            }
        };
        self.mode = TransportMode::Playing(PlaybackSession {
            start_seconds: frame_rate.frame_to_seconds(start_frame),
            started_at: Instant::now(),
            uses_audio_clock: clock == PlaybackClock::Audio,
        });
        let start_seconds = frame_rate.frame_to_seconds(start_frame);
        self.editor.update(cx, |editor, cx| {
            if editor.set_playback_position(start_seconds, start_frame) {
                cx.notify();
            }
        });
        cx.notify();
    }

    pub(crate) fn stop(&mut self, cx: &mut Context<Self>) -> bool {
        if matches!(self.mode, TransportMode::Stopped) {
            return false;
        }
        self.stop_audio(cx);
        self.mode = TransportMode::Stopped;
        self.editor.update(cx, |editor, cx| {
            if editor.clear_playback_time() {
                cx.notify();
            }
        });
        cx.notify();
        true
    }

    fn stop_audio(&self, cx: &mut Context<Self>) {
        self.audio.update(cx, |audio, _| {
            audio.request_stop();
        });
    }

    pub(crate) fn begin_scrub(&mut self, source: ScrubSource, cx: &mut Context<Self>) {
        self.stop_audio(cx);
        self.mode = TransportMode::Scrubbing(source);
        cx.notify();
    }

    pub(crate) fn end_scrub(&mut self, source: ScrubSource, cx: &mut Context<Self>) {
        if !matches!(self.mode, TransportMode::Scrubbing(current) if current == source) {
            return;
        }
        self.mode = TransportMode::Stopped;
        self.editor.update(cx, |editor, cx| {
            if editor.clear_playback_time() {
                cx.notify();
            }
        });
        cx.notify();
    }

    pub(crate) fn seek(&mut self, frame: Frame, cx: &mut Context<Self>) -> bool {
        if matches!(self.mode, TransportMode::Playing(_)) {
            self.stop(cx);
            let changed = self.set_playhead(frame, cx);
            self.play(cx);
            return changed;
        }
        self.editor
            .update_if_changed(cx, |editor| editor.seek(frame))
    }

    pub(crate) fn set_playhead(&mut self, frame: Frame, cx: &mut Context<Self>) -> bool {
        self.editor
            .update_if_changed(cx, |editor| editor.set_playhead(frame))
    }

    pub(crate) fn step(&mut self, delta: i64, cx: &mut Context<Self>) {
        self.stop(cx);
        self.editor
            .update_if_changed(cx, |editor| editor.step_playhead(delta));
    }

    pub(crate) fn advance(&mut self, cx: &mut Context<Self>) {
        self.report_audio_events(cx);
        let TransportMode::Playing(mut playback) = self.mode else {
            return;
        };
        let (frame_rate, end) = {
            let editor = self.editor.read(cx);
            (editor.frame_rate(), editor.end_frame_exclusive())
        };
        let audio_seconds = self.audio.read(cx).playhead_seconds();
        if playback.uses_audio_clock && audio_seconds.is_none() {
            // Continue from the last displayed position if the audio worker exits.
            let editor = self.editor.read(cx);
            playback.start_seconds = editor
                .playback_time_seconds()
                .unwrap_or_else(|| frame_rate.frame_to_seconds(editor.playhead()));
            playback.started_at = Instant::now();
            playback.uses_audio_clock = false;
        }
        let mut seconds = if playback.uses_audio_clock {
            audio_seconds.unwrap()
        } else {
            playback.start_seconds + playback.started_at.elapsed().as_secs_f64()
        };
        let mut frame = frame_rate.seconds_to_frame(seconds);
        if frame >= end {
            self.stop(cx);
            self.set_playhead(end, cx);
            return;
        }
        let items = self.editor.read(cx).visible_items();
        let plan = audio_plan(&items);
        if plan != self.audio_plan || frame_rate != self.audio_frame_rate {
            let clock = self
                .audio
                .update(cx, |audio, _| audio.play(items, frame, frame_rate));
            playback.start_seconds = seconds;
            playback.started_at = Instant::now();
            playback.uses_audio_clock = match clock {
                Ok(clock) => clock == PlaybackClock::Audio,
                Err(error) => {
                    self.notifications.update(cx, |notifications, cx| {
                        notifications.push(
                            t!("transport.start_playback_failed", error = error).to_string(),
                            cx,
                        )
                    });
                    false
                }
            };
            self.audio_plan = plan;
            self.audio_frame_rate = frame_rate;
        }
        if !playback.uses_audio_clock && frame_rate.seconds_to_frame(seconds) < end {
            match self
                .audio
                .update(cx, |audio, _| audio.resume_pending(seconds))
            {
                Some(Ok(PlaybackClock::Audio)) => {
                    playback.uses_audio_clock = true;
                    seconds = self.audio.read(cx).playhead_seconds().unwrap_or(seconds);
                }
                Some(Err(error)) => {
                    self.notifications.update(cx, |notifications, cx| {
                        notifications.push(
                            t!("transport.start_playback_failed", error = error).to_string(),
                            cx,
                        );
                    });
                }
                _ => {}
            }
        }
        frame = frame_rate.seconds_to_frame(seconds);
        if frame >= end {
            self.stop(cx);
            self.set_playhead(end, cx);
            return;
        }
        self.mode = TransportMode::Playing(playback);
        let changed = self
            .editor
            .update_if_changed(cx, |editor| editor.set_playback_position(seconds, frame));
        if changed {
            let active_items = self
                .editor
                .read(cx)
                .active_items_at(frame)
                .into_iter()
                .map(|(_, item)| item)
                .collect::<Vec<_>>();
            self.audio.update(cx, |audio, _| {
                audio.update_gains(&active_items, seconds, frame_rate)
            });
        }
    }

    fn report_audio_events(&mut self, cx: &mut Context<Self>) {
        let events = self.audio.update(cx, |audio, _| audio.take_events());
        for event in events {
            let message = match event {
                AudioPlaybackEvent::Underrun {
                    missing_sample_frames,
                } => {
                    format!("Audio buffer underrun ({missing_sample_frames} sample frames missing)")
                }
                AudioPlaybackEvent::Recovered => "Audio playback recovered".to_owned(),
                AudioPlaybackEvent::DecodeFailed(error) => {
                    t!("transport.audio_decode_failed", error = error).to_string()
                }
                AudioPlaybackEvent::DeviceFailed(error) => {
                    t!("transport.audio_device_failed", error = error).to_string()
                }
                AudioPlaybackEvent::WorkerPanicked => {
                    t!("transport.audio_worker_panicked").to_string()
                }
            };
            self.notifications
                .update(cx, |notifications, cx| notifications.push(message, cx));
        }
    }

    pub(crate) fn reset_for_project_change(&mut self, cx: &mut Context<Self>) {
        self.stop_audio(cx);
        self.mode = TransportMode::Stopped;
        cx.notify();
    }
}
