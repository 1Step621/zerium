use std::sync::Arc;

use super::{
    EvaluatedItem,
    editor::TimelineEditor,
    evaluation::{
        EvaluatedSceneNode, SceneRenderStage, evaluated_items_at_time,
        evaluated_scene_graph_at_time,
    },
    ids::{ItemId, LayerId, ProjectId},
    item::TimelineItem,
    project::TimelineProject,
    scene::SceneDefinition,
    settings::{BeatGuide, ProjectResolution},
    time::{Frame, FrameRate, TimelineTime},
};

/// Immutable, detached timeline state for background save and export jobs.
///
/// Editing state deliberately is not `Clone`: copying an editor used to
/// silently discard its undo/redo history. Callers now request this explicit
/// read model, which contains only the state a background consumer can use.
#[derive(Clone)]
pub struct TimelineSnapshot {
    pub(super) project: Arc<TimelineProject>,
    pub(super) media_cache: Arc<crate::media::MediaMetadataCache>,
    pub(super) playhead: Frame,
    pub(super) project_revision: u64,
}

/// Read-only timeline contract shared by the live editor and detached
/// snapshots. Rendering and export depend on this view instead of the editor's
/// command/history implementation.
pub trait TimelineView {
    fn media_cache(&self) -> &crate::media::MediaMetadataCache;

    fn resolution(&self) -> ProjectResolution;

    fn frame_rate(&self) -> FrameRate;

    fn playhead(&self) -> Frame;

    fn active_items_at_time(&self, time: TimelineTime) -> Vec<(LayerId, EvaluatedItem)>;

    fn active_scene_graph_at_time(&self, time: TimelineTime) -> Vec<EvaluatedSceneNode> {
        self.scene_graph_at_time(time, &[])
    }

    /// Evaluate the edited composition for a consumer's effect-stage limits.
    /// Omitted effects never resolve properties or execute scene calculations.
    fn scene_graph_at_time(
        &self,
        time: TimelineTime,
        stages: &[SceneRenderStage],
    ) -> Vec<EvaluatedSceneNode>;

    fn active_items_at(&self, frame: Frame) -> Vec<(LayerId, EvaluatedItem)> {
        self.active_items_at_time(TimelineTime::from_frame(frame))
    }

    fn end_frame_exclusive(&self) -> Frame;
}

impl TimelineSnapshot {
    pub fn with_media_cache(mut self, cache: crate::media::MediaMetadataCache) -> Self {
        self.media_cache = Arc::new(cache);
        self
    }

    pub fn media_dependencies_revision(&self) -> u64 {
        self.project.media_dependencies_revision
    }

    pub fn project_revision(&self) -> u64 {
        self.project_revision
    }

    pub fn project_id(&self) -> ProjectId {
        self.project.id
    }

    pub fn resolution(&self) -> ProjectResolution {
        self.project.resolution
    }

    pub fn beat_guide(&self) -> BeatGuide {
        self.project.beat_guide
    }

    pub fn items(&self) -> impl Iterator<Item = &TimelineItem> {
        self.project.document.items()
    }

    pub fn audio_snapshot(&self) -> super::AudioTimelineSnapshot {
        super::AudioTimelineSnapshot::new(
            self.project.clone(),
            self.media_cache.clone(),
            None,
            None,
        )
    }

    pub fn item_layer(&self, id: ItemId) -> Option<LayerId> {
        self.project.document.item_layer(id)
    }

    pub fn scenes(&self) -> impl Iterator<Item = &SceneDefinition> {
        self.project.scenes.values()
    }
}

impl TimelineView for TimelineSnapshot {
    fn media_cache(&self) -> &crate::media::MediaMetadataCache {
        &self.media_cache
    }

    fn resolution(&self) -> ProjectResolution {
        self.project.resolution
    }

    fn frame_rate(&self) -> FrameRate {
        self.project.document.frame_rate()
    }

    fn playhead(&self) -> Frame {
        self.playhead
    }

    fn active_items_at_time(&self, time: TimelineTime) -> Vec<(LayerId, EvaluatedItem)> {
        evaluated_items_at_time(
            self.project.document.active_source_items_at_time(time),
            &self.project.scenes,
            time,
            None,
            None,
        )
    }

    fn scene_graph_at_time(
        &self,
        time: TimelineTime,
        stages: &[SceneRenderStage],
    ) -> Vec<EvaluatedSceneNode> {
        evaluated_scene_graph_at_time(
            self.project.document.active_source_items_at_time(time),
            &self.project.scenes,
            time,
            None,
            None,
            stages,
        )
    }

    fn end_frame_exclusive(&self) -> Frame {
        self.project.document.end_frame_exclusive()
    }
}

impl TimelineView for TimelineEditor {
    fn media_cache(&self) -> &crate::media::MediaMetadataCache {
        TimelineEditor::media_cache(self)
    }

    fn resolution(&self) -> ProjectResolution {
        TimelineEditor::resolution(self)
    }

    fn frame_rate(&self) -> FrameRate {
        TimelineEditor::frame_rate(self)
    }

    fn playhead(&self) -> Frame {
        TimelineEditor::playhead(self)
    }

    fn active_items_at_time(&self, time: TimelineTime) -> Vec<(LayerId, EvaluatedItem)> {
        TimelineEditor::active_items_at_time(self, time)
    }

    fn scene_graph_at_time(
        &self,
        time: TimelineTime,
        stages: &[SceneRenderStage],
    ) -> Vec<EvaluatedSceneNode> {
        evaluated_scene_graph_at_time(
            self.active_document().active_source_items_at_time(time),
            &self.project().scenes,
            time,
            Some(&self.visibility),
            self.active_scene_id().and_then(|id| self.scene(id)),
            stages,
        )
    }

    fn end_frame_exclusive(&self) -> Frame {
        TimelineEditor::end_frame_exclusive(self)
    }
}
