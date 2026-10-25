use std::{collections::HashMap, sync::Arc};

use super::{
    document::TimelineDocument,
    ids::{ProjectId, SceneId},
    scene::SceneDefinition,
    settings::{BeatGuide, ProjectResolution, ProjectSettingsError},
    time::FrameRate,
};

/// Persistent state for one timeline project. Mutations are coordinated by
/// `TimelineEditor`, which also exposes immutable snapshots to background work.
#[derive(Clone)]
pub(super) struct TimelineProject {
    pub(super) id: ProjectId,
    pub(super) media_dependencies_revision: u64,
    pub(super) document: Arc<TimelineDocument>,
    pub(super) scenes: Arc<HashMap<SceneId, SceneDefinition>>,
    pub(super) resolution: ProjectResolution,
    pub(super) beat_guide: BeatGuide,
}

impl TimelineProject {
    /// Every definition owns its local clock, including its expansion anchor.
    /// Prepare the entire project before publishing a frame-rate change.
    pub(super) fn retimed(&self, frame_rate: FrameRate) -> Result<Self, ProjectSettingsError> {
        let mut project = self.clone();
        if self.document.frame_rate() != frame_rate {
            project.document = Arc::new(self.document.retimed(frame_rate)?);
            project.scenes = Arc::new(
                self.scenes
                    .iter()
                    .map(|(id, scene)| Ok((*id, scene.retimed(frame_rate)?)))
                    .collect::<Result<_, ProjectSettingsError>>()?,
            );
        }
        Ok(project)
    }

    pub(super) fn document_mut(&mut self) -> &mut TimelineDocument {
        Arc::make_mut(&mut self.document)
    }

    pub(super) fn scenes_mut(&mut self) -> &mut HashMap<SceneId, SceneDefinition> {
        Arc::make_mut(&mut self.scenes)
    }
}
