use std::time::Instant;

use gpui::{Context, Entity};

use crate::{
    domain::timeline::{Frame, TimelineEditor},
    engine::audio_playback::{
        AudioPlaybackEngine, AudioPlaybackError, AudioPlaybackEvent, PlaybackClock,
    },
};

use super::session::UiNotifications;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrubSource {
    Timeline,
    AnimationCurve,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreviewPlaybackMode {
    Idle,
    Scrubbing,
    Playing,
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

    pub(crate) fn playback_mode(&self) -> PreviewPlaybackMode {
        match self.mode {
            TransportMode::Stopped => PreviewPlaybackMode::Idle,
            TransportMode::Scrubbing(_) => PreviewPlaybackMode::Scrubbing,
            TransportMode::Playing(_) => PreviewPlaybackMode::Playing,
        }
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
        let (start_frame, frame_rate, items, active_items) = {
            let editor = self.editor.read(cx);
            let playhead = editor.playhead();
            let start_frame = if playhead >= editor.end_frame_exclusive() {
                Frame::new(0)
            } else {
                playhead
            };
            (
                start_frame,
                editor.frame_rate(),
                editor.visible_items(),
                editor
                    .active_items_at(start_frame)
                    .into_iter()
                    .map(|(_, item)| item)
                    .collect::<Vec<_>>(),
            )
        };
        let clock: Result<PlaybackClock, AudioPlaybackError> = self.audio.update(cx, |audio, _| {
            let clock = audio.play(items, start_frame, frame_rate)?;
            audio.update_gains(&active_items);
            Ok(clock)
        });
        let clock = match clock {
            Ok(clock) => clock,
            Err(error) => {
                self.notifications.update(cx, |notifications, cx| {
                    notifications.push(format!("音声再生を開始できません: {error}"), cx);
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
        let changed = self.editor.update(cx, |editor, _| editor.seek(frame));
        if changed {
            cx.notify();
        }
        changed
    }

    pub(crate) fn set_playhead(&mut self, frame: Frame, cx: &mut Context<Self>) -> bool {
        let changed = self
            .editor
            .update(cx, |editor, _| editor.set_playhead(frame));
        if changed {
            cx.notify();
        }
        changed
    }

    pub(crate) fn step(&mut self, delta: i64, cx: &mut Context<Self>) {
        self.stop(cx);
        let changed = self
            .editor
            .update(cx, |editor, _| editor.step_playhead(delta));
        if changed {
            cx.notify();
        }
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
                        notifications.push(format!("音声再生を開始できません: {error}"), cx);
                    });
                }
                _ => {}
            }
        }
        self.mode = TransportMode::Playing(playback);
        let frame = frame_rate.seconds_to_frame(seconds);
        if frame >= end {
            self.stop(cx);
            self.set_playhead(end, cx);
            return;
        }
        let changed = self
            .editor
            .update(cx, |editor, _| editor.set_playback_position(seconds, frame));
        if changed {
            let active_items = self
                .editor
                .read(cx)
                .active_items_at(frame)
                .into_iter()
                .map(|(_, item)| item)
                .collect::<Vec<_>>();
            self.audio
                .update(cx, |audio, _| audio.update_gains(&active_items));
            cx.notify();
        }
    }

    fn report_audio_events(&mut self, cx: &mut Context<Self>) {
        let events = self.audio.update(cx, |audio, _| audio.take_events());
        for event in events {
            let message = match event {
                AudioPlaybackEvent::Underrun {
                    missing_sample_frames,
                } => {
                    format!("音声バッファが不足しました（{missing_sample_frames}サンプルフレーム）")
                }
                AudioPlaybackEvent::Recovered => "音声再生が復旧しました".to_owned(),
                AudioPlaybackEvent::DecodeFailed(error) => {
                    format!("音声のデコードに失敗しました: {error}")
                }
                AudioPlaybackEvent::DeviceFailed(error) => {
                    format!("音声デバイスでエラーが発生しました: {error}")
                }
                AudioPlaybackEvent::WorkerPanicked => {
                    "音声処理スレッドが予期せず終了しました".to_owned()
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
