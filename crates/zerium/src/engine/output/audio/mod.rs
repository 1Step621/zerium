mod output;
mod preparation;
mod session;

use self::{
    preparation::{AudioPreparation, PlaybackPosition, PreparedAudio},
    session::{AudioPlaybackSession, AudioWorker},
};
use std::{collections::VecDeque, sync::Arc};

use thiserror::Error;

use crate::engine::{input::MediaReaderRegistry, processing::audio::AudioTimelineError};
use zerium_core::timeline::{AudioTimelineSnapshot, Frame};

const MIX_BLOCK_SAMPLE_FRAMES: usize = 2_048;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaybackClock {
    Audio,
    Wall,
    Preparing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AudioPlaybackEvent {
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

enum AudioPlaybackState {
    Idle,
    Preparing(AudioPreparation),
    Playing(AudioPlaybackSession),
}

impl AudioPlaybackState {
    fn clock(&self) -> PlaybackClock {
        match self {
            Self::Idle => PlaybackClock::Wall,
            Self::Preparing(_) => PlaybackClock::Preparing,
            Self::Playing(_) => PlaybackClock::Audio,
        }
    }
}

pub(crate) struct AudioPlaybackEngine {
    requested_timeline: Option<AudioTimelineSnapshot>,
    state: AudioPlaybackState,
    retiring: Vec<AudioWorker>,
    media_readers: Arc<MediaReaderRegistry>,
    completed_events: VecDeque<AudioPlaybackEvent>,
}

impl AudioPlaybackEngine {
    pub(crate) fn new(media_readers: Arc<MediaReaderRegistry>) -> Self {
        Self {
            requested_timeline: None,
            state: AudioPlaybackState::Idle,
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
        self.requested_timeline = Some(timeline.clone());
        self.start(timeline, PlaybackPosition::Frame(start_frame))
    }

    fn start(
        &mut self,
        timeline: AudioTimelineSnapshot,
        position: PlaybackPosition,
    ) -> Result<PlaybackClock, AudioPlaybackError> {
        self.state = AudioPlaybackState::Preparing(AudioPreparation::spawn(
            timeline,
            position,
            self.media_readers.clone(),
        )?);
        Ok(PlaybackClock::Preparing)
    }

    fn start_prepared(
        &mut self,
        prepared: PreparedAudio,
        timeline: AudioTimelineSnapshot,
    ) -> Result<PlaybackClock, AudioPlaybackError> {
        let session = AudioPlaybackSession::new(prepared, timeline, self.media_readers.clone())?;
        if let Err(error) = session.play() {
            self.retire_session(session);
            return Err(error);
        }
        self.state = AudioPlaybackState::Playing(session);
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
            .requested_timeline
            .as_ref()
            .is_none_or(|previous| !previous.same_content(&timeline));
        if changed {
            self.requested_timeline = Some(timeline.clone());
        }
        if let AudioPlaybackState::Preparing(preparation) = &self.state {
            if !preparation.is_finished() {
                return Ok(PlaybackClock::Preparing);
            }
            let AudioPlaybackState::Preparing(preparation) =
                std::mem::replace(&mut self.state, AudioPlaybackState::Idle)
            else {
                unreachable!("checked preparation")
            };
            let superseded = !preparation.matches_timeline(&timeline);
            return match preparation.finish() {
                Ok(Some(prepared)) => self.start_prepared(prepared, timeline),
                Ok(None) | Err(_) if superseded => {
                    self.start(timeline, PlaybackPosition::Seconds(seconds))
                }
                Ok(None) => Ok(PlaybackClock::Wall),
                Err(error) => Err(error),
            };
        }
        if !changed {
            return Ok(self.state.clock());
        }
        let AudioPlaybackState::Playing(session) = &mut self.state else {
            return self.start(timeline, PlaybackPosition::Seconds(seconds));
        };
        session.update_timeline(timeline);
        Ok(PlaybackClock::Audio)
    }

    pub(crate) fn request_stop(&mut self) -> bool {
        self.reap_finished_workers();
        self.requested_timeline = None;
        match std::mem::replace(&mut self.state, AudioPlaybackState::Idle) {
            AudioPlaybackState::Idle => false,
            AudioPlaybackState::Preparing(preparation) => {
                preparation.cancel();
                true
            }
            AudioPlaybackState::Playing(session) => {
                self.retire_session(session);
                true
            }
        }
    }

    /// Keep the requested revision after a worker failure, so an unchanged
    /// timeline does not repeatedly restart a failed decoder or output device.
    fn stop_session(&mut self) -> bool {
        if !matches!(self.state, AudioPlaybackState::Playing(_)) {
            return false;
        }
        let AudioPlaybackState::Playing(session) =
            std::mem::replace(&mut self.state, AudioPlaybackState::Idle)
        else {
            unreachable!("checked session")
        };
        self.retire_session(session);
        true
    }

    fn retire_session(&mut self, session: AudioPlaybackSession) {
        self.retire_worker(session.retire());
    }

    fn retire_worker(&mut self, worker: AudioWorker) {
        worker.poll_events(&mut self.completed_events);
        if worker.is_finished() {
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
            if worker.is_finished() {
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
        if let AudioPlaybackState::Playing(session) = &mut self.state {
            session.poll_events(&mut self.completed_events);
        }
        self.completed_events.drain(..).collect()
    }

    fn session_stopped(&self) -> bool {
        matches!(&self.state, AudioPlaybackState::Playing(session) if session.has_stopped())
    }

    pub(crate) fn playhead_seconds(&self) -> Option<f64> {
        let AudioPlaybackState::Playing(session) = &self.state else {
            return None;
        };
        session.playhead_seconds()
    }

    pub(crate) fn levels(&self) -> [f32; 2] {
        match &self.state {
            AudioPlaybackState::Playing(session) => session.levels(),
            _ => [0.; 2],
        }
    }
}

impl Drop for AudioPlaybackEngine {
    fn drop(&mut self) {
        self.request_stop();
    }
}
