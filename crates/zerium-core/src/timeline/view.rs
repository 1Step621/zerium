use std::sync::Arc;

use super::{
    editor::TimelineEditor,
    evaluation::{
        EvaluatedSceneNode, evaluated_items_at_time, evaluated_scene_graph_at_time, visible_items,
        visit_source_items,
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
    fn active_items_at_time(&self, time: TimelineTime) -> Vec<(LayerId, TimelineItem)>;
    fn active_scene_graph_at_time(&self, time: TimelineTime) -> Vec<EvaluatedSceneNode>;
    fn active_items_at(&self, frame: Frame) -> Vec<(LayerId, TimelineItem)> {
        self.active_items_at_time(TimelineTime::from_frame(frame))
    }
    fn visible_items(&self) -> Vec<TimelineItem>;
    fn end_frame_exclusive(&self) -> Frame;
}

impl TimelineSnapshot {
    pub fn with_media_cache(mut self, cache: crate::media::MediaMetadataCache) -> Self {
        self.media_cache = Arc::new(cache);
        self
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

    /// Visit all argument-resolved source items, including unused scenes and
    /// scene-instance effects. Unlike rendering, this traversal does not clip
    /// items to a time range or assign runtime identities.
    pub fn visit_resolved_items(&self, mut visit: impl FnMut(&TimelineItem)) {
        visit_source_items(
            self.project.document.source_items(),
            &self.project.scenes,
            &mut visit,
        );
        for scene in self.project.scenes.values() {
            let mut items = scene.document().source_items();
            scene.apply_arguments(
                &self.project.scenes,
                None,
                items.iter_mut().map(|(_, item)| item),
            );
            visit_source_items(items, &self.project.scenes, &mut visit);
        }
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

    fn active_items_at_time(&self, time: TimelineTime) -> Vec<(LayerId, TimelineItem)> {
        evaluated_items_at_time(
            self.project.document.active_source_items_at_time(time),
            &self.project.scenes,
            time,
            None,
        )
    }

    fn active_scene_graph_at_time(&self, time: TimelineTime) -> Vec<EvaluatedSceneNode> {
        evaluated_scene_graph_at_time(
            self.project.document.active_source_items_at_time(time),
            &self.project.scenes,
            time,
            None,
        )
    }

    fn visible_items(&self) -> Vec<TimelineItem> {
        visible_items(
            self.project.document.source_items(),
            &self.project.scenes,
            None,
        )
    }

    fn end_frame_exclusive(&self) -> Frame {
        self.project
            .document
            .items()
            .map(TimelineItem::end_exclusive)
            .max()
            .unwrap_or(Frame::new(0))
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

    fn active_items_at_time(&self, time: TimelineTime) -> Vec<(LayerId, TimelineItem)> {
        TimelineEditor::active_items_at_time(self, time)
    }

    fn active_scene_graph_at_time(&self, time: TimelineTime) -> Vec<EvaluatedSceneNode> {
        evaluated_scene_graph_at_time(
            self.active_source_items(Some(time)),
            &self.project().scenes,
            time,
            Some(&self.visibility),
        )
    }

    fn visible_items(&self) -> Vec<TimelineItem> {
        TimelineEditor::visible_items(self)
    }

    fn end_frame_exclusive(&self) -> Frame {
        TimelineEditor::end_frame_exclusive(self)
    }
}
