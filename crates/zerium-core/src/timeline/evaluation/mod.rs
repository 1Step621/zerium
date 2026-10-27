//! Consumer projections of evaluated scenes.

use self::scene::evaluated_items_at_time;
use super::{Frame, FrameRate, LayerId, ProjectResolution, TimelineSnapshot, TimelineTime};

mod audio;
mod media;
mod render;
pub(super) mod scene;
pub use audio::{
    AudioCalculationDiagnostic, AudioClipId, AudioTimelineSnapshot, AudioTimelineSource,
    EvaluatedAudioGain,
};
pub use render::{
    RenderEvaluation, RenderPlacement, RenderPropertyTarget, ResolvedRenderProperties,
};

/// Read-only timeline contract shared by the live editor and detached
/// snapshots. Rendering and export depend on this view instead of the editor's
/// command/history implementation.
pub trait TimelineView {
    fn media_cache(&self) -> &crate::media::MediaMetadataCache;

    fn resolution(&self) -> ProjectResolution;

    fn frame_rate(&self) -> FrameRate;

    fn playhead(&self) -> Frame;

    fn active_items_at_time(&self, time: TimelineTime) -> Vec<(LayerId, EvaluatedItem)>;

    /// Placement lookup and stage properties share the same scene clocks.
    fn render_evaluation(&self) -> RenderEvaluation<'_>;

    fn active_items_at(&self, frame: Frame) -> Vec<(LayerId, EvaluatedItem)> {
        self.active_items_at_time(TimelineTime::from_frame(frame))
    }

    fn end_frame_exclusive(&self) -> Frame;
}

impl TimelineView for TimelineSnapshot {
    fn media_cache(&self) -> &crate::media::MediaMetadataCache {
        &self.media_cache
    }

    fn resolution(&self) -> ProjectResolution {
        self.project.resolution
    }

    fn frame_rate(&self) -> FrameRate {
        self.root_scene().document().frame_rate()
    }

    fn playhead(&self) -> Frame {
        self.playhead
    }

    fn active_items_at_time(&self, time: TimelineTime) -> Vec<(LayerId, EvaluatedItem)> {
        evaluated_items_at_time(self.root_scene(), &self.project.scenes, time)
    }

    fn render_evaluation(&self) -> RenderEvaluation<'_> {
        RenderEvaluation::new(self.root_scene(), &self.project.scenes)
    }

    fn end_frame_exclusive(&self) -> Frame {
        self.root_scene().document().end_frame_exclusive()
    }
}

pub use scene::{
    EvaluatedClip, EvaluatedSceneNode,
    item::{EvaluatedEffect, EvaluatedItem, RenderResultSettings},
};
