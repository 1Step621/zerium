use rust_i18n::t;
use std::time::Instant;

use gpui::{App, Context, Entity};

use crate::{
    engine::output::audio::{
        AudioPlaybackEngine, AudioPlaybackError, AudioPlaybackEvent, PlaybackClock,
    },
    ui::{TimelineEditorEntityExt, workspace::notifications::UiNotifications},
};
use zerium_core::timeline::{Frame, TimelineEditor};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrubSource {
    Timeline,
    AnimationCurve,
    Preview,
}

#[derive(Clone, Copy, Debug)]
struct PlaybackSession {
    start_frame: Frame,
    start_seconds: f64,
    started_at: Instant,
    clock: PlaybackClock,
}

impl PlaybackSession {
    fn seconds(&mut self, audio: Option<f64>, displayed: f64, now: Instant) -> f64 {
        match self.clock {
            PlaybackClock::Audio => audio.unwrap_or_else(|| {
                // A failed output resumes from the last displayed position.
                self.set_clock(PlaybackClock::Wall, displayed, now);
                displayed
            }),
            PlaybackClock::Preparing => self.start_seconds,
            PlaybackClock::Wall => {
                self.start_seconds + now.duration_since(self.started_at).as_secs_f64()
            }
        }
    }

    fn set_clock(&mut self, clock: PlaybackClock, seconds: f64, now: Instant) {
        if clock != self.clock && clock != PlaybackClock::Audio {
            self.start_seconds = seconds;
            self.started_at = now;
        }
        self.clock = clock;
    }
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
        }
    }

    pub(crate) fn is_playing(&self) -> bool {
        matches!(self.mode, TransportMode::Playing(_))
    }

    pub(crate) fn is_scrubbing(&self, source: ScrubSource) -> bool {
        matches!(self.mode, TransportMode::Scrubbing(current) if current == source)
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

    pub(crate) fn toggle_playback_in_place(&mut self, cx: &mut Context<Self>) {
        if self.is_playing() {
            self.stop_in_place(cx);
        } else {
            self.play(cx);
        }
    }

    fn play(&mut self, cx: &mut Context<Self>) {
        self.stop_audio(cx);
        let (start_frame, timeline) = {
            let editor = self.editor.read(cx);
            let playhead = editor.playhead();
            let start = if playhead >= editor.end_frame_exclusive() {
                Frame::new(0)
            } else {
                playhead
            };
            (start, editor.audio_snapshot())
        };
        let frame_rate = timeline.frame_rate();
        let clock: Result<PlaybackClock, AudioPlaybackError> = self
            .audio
            .update(cx, |audio, _| audio.play(timeline.clone(), start_frame));
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
            start_frame,
            start_seconds: frame_rate.frame_to_seconds(start_frame),
            started_at: Instant::now(),
            clock,
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
        let start_frame = match self.mode {
            TransportMode::Playing(playback) => Some(playback.start_frame),
            _ => None,
        };
        let stopped = self.stop_in_place(cx);
        if let Some(frame) = start_frame {
            self.set_playhead(frame, cx);
        }
        stopped
    }

    pub(crate) fn stop_in_place(&mut self, cx: &mut Context<Self>) -> bool {
        if let TransportMode::Stopped = self.mode {
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
        if !self.is_scrubbing(source) {
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

    pub(crate) fn set_playhead(&mut self, frame: Frame, cx: &mut Context<Self>) -> bool {
        let stopped = self.is_playing() && self.stop_in_place(cx);
        self.editor
            .update_if_changed(cx, |editor| editor.set_playhead(frame))
            || stopped
    }

    pub(crate) fn step(&mut self, delta: i64, cx: &mut Context<Self>) {
        self.stop_in_place(cx);
        self.editor
            .update_if_changed(cx, |editor| editor.step_playhead(delta));
    }

    pub(crate) fn advance(&mut self, cx: &mut Context<Self>) {
        self.report_audio_events(cx);
        let TransportMode::Playing(mut playback) = self.mode else {
            return;
        };
        let (frame_rate, end, displayed_seconds) = {
            let editor = self.editor.read(cx);
            (
                editor.frame_rate(),
                editor.end_frame_exclusive(),
                editor.playhead_seconds(),
            )
        };
        let audio_seconds = self.audio.read(cx).playhead_seconds();
        let mut seconds = playback.seconds(audio_seconds, displayed_seconds, Instant::now());
        let mut frame = frame_rate.seconds_to_frame(seconds);
        if frame >= end {
            self.stop(cx);
            return;
        }
        let timeline = self.editor.read(cx).audio_snapshot();
        match self
            .audio
            .update(cx, |audio, _| audio.update_timeline(timeline, seconds))
        {
            Ok(clock) => {
                if clock == PlaybackClock::Audio {
                    seconds = self.audio.read(cx).playhead_seconds().unwrap_or(seconds);
                }
                playback.set_clock(clock, seconds, Instant::now());
            }
            Err(error) => {
                playback.set_clock(PlaybackClock::Wall, seconds, Instant::now());
                self.notifications.update(cx, |notifications, cx| {
                    notifications.push(
                        t!("transport.start_playback_failed", error = error).to_string(),
                        cx,
                    );
                });
            }
        }
        frame = frame_rate.seconds_to_frame(seconds);
        if frame >= end {
            self.stop(cx);
            return;
        }
        self.mode = TransportMode::Playing(playback);
        self.editor
            .update_if_changed(cx, |editor| editor.set_playback_position(seconds, frame));
    }

    fn report_audio_events(&mut self, cx: &mut Context<Self>) {
        let events = self.audio.update(cx, |audio, _| audio.take_events());
        for event in events {
            let message = match event {
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
