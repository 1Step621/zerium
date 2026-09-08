use super::{
    document::TimelineDocument,
    editor::TimelineEditor,
    evaluation::{
        EvaluatedSceneNode, document_items, evaluated_document_graph_at_time,
        evaluated_document_items_at_time, evaluated_visible_document_graph_at_time,
    },
    ids::{ItemId, LayerId, ProjectId, SceneId},
    item::TimelineItem,
    scene::SceneDefinition,
    settings::ProjectResolution,
    time::{Frame, FrameRate, TimelineTime},
};

/// Immutable, detached timeline state for background save and export jobs.
///
/// Editing state deliberately is not `Clone`: copying an editor used to
/// silently discard its undo/redo history. Callers now request this explicit
/// read model, which contains only the state a background consumer can use.
#[derive(Clone)]
pub(crate) struct TimelineSnapshot {
    document: TimelineDocument,
    scenes: std::collections::HashMap<SceneId, SceneDefinition>,
    project_id: ProjectId,
    resolution: ProjectResolution,
    playhead: Frame,
    project_revision: u64,
}

/// Read-only timeline contract shared by the live editor and detached
/// snapshots. Rendering and export depend on this view instead of the editor's
/// command/history implementation.
pub(crate) trait TimelineView {
    fn resolution(&self) -> ProjectResolution;
    fn frame_rate(&self) -> FrameRate;
    fn playhead(&self) -> Frame;
    fn active_items_at_time(&self, time: TimelineTime) -> Vec<(LayerId, TimelineItem)>;
    fn active_scene_graph_at_time(&self, time: TimelineTime) -> Vec<EvaluatedSceneNode>;
    fn active_items_at(&self, frame: Frame) -> Vec<(LayerId, TimelineItem)> {
        self.active_items_at_time(TimelineTime::from_frame(frame))
    }
    fn visible_items(&self) -> Vec<TimelineItem>;
    fn end_frame_exclusive(&self) -> Frame;
}

impl TimelineSnapshot {
    pub(super) fn new(
        document: TimelineDocument,
        scenes: std::collections::HashMap<SceneId, SceneDefinition>,
        project_id: ProjectId,
        resolution: ProjectResolution,
        playhead: Frame,
        project_revision: u64,
    ) -> Self {
        Self {
            document,
            scenes,
            project_id,
            resolution,
            playhead,
            project_revision,
        }
    }

    pub(crate) fn project_revision(&self) -> u64 {
        self.project_revision
    }

    pub(crate) fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub(crate) fn resolution(&self) -> ProjectResolution {
        self.resolution
    }

    pub(crate) fn items(&self) -> impl Iterator<Item = &TimelineItem> {
        self.document.items()
    }

    pub(crate) fn item_layer(&self, id: ItemId) -> Option<LayerId> {
        self.document.item_layer(id)
    }

    pub(crate) fn scenes(&self) -> impl Iterator<Item = &SceneDefinition> {
        self.scenes.values()
    }
}

impl TimelineView for TimelineSnapshot {
    fn resolution(&self) -> ProjectResolution {
        self.resolution
    }

    fn frame_rate(&self) -> FrameRate {
        self.document.frame_rate()
    }

    fn playhead(&self) -> Frame {
        self.playhead
    }

    fn active_items_at_time(&self, time: TimelineTime) -> Vec<(LayerId, TimelineItem)> {
        evaluated_document_items_at_time(&self.document, &self.scenes, time)
    }

    fn active_scene_graph_at_time(&self, time: TimelineTime) -> Vec<EvaluatedSceneNode> {
        evaluated_document_graph_at_time(&self.document, &self.scenes, time)
    }

    fn visible_items(&self) -> Vec<TimelineItem> {
        document_items(&self.document, &self.scenes)
    }

    fn end_frame_exclusive(&self) -> Frame {
        self.document
            .items()
            .map(TimelineItem::end_exclusive)
            .max()
            .unwrap_or(Frame::new(0))
    }
}

impl TimelineView for TimelineEditor {
    fn resolution(&self) -> ProjectResolution {
        TimelineEditor::resolution(self)
    }

    fn frame_rate(&self) -> FrameRate {
        TimelineEditor::frame_rate(self)
    }

    fn playhead(&self) -> Frame {
        TimelineEditor::playhead(self)
    }

    fn active_items_at_time(&self, time: TimelineTime) -> Vec<(LayerId, TimelineItem)> {
        TimelineEditor::active_items_at_time(self, time)
    }

    fn active_scene_graph_at_time(&self, time: TimelineTime) -> Vec<EvaluatedSceneNode> {
        evaluated_visible_document_graph_at_time(
            self.active_document(),
            &self.project().scenes,
            &self.visibility,
            time,
        )
    }

    fn visible_items(&self) -> Vec<TimelineItem> {
        TimelineEditor::visible_items(self)
    }

    fn end_frame_exclusive(&self) -> Frame {
        TimelineEditor::end_frame_exclusive(self)
    }
}
