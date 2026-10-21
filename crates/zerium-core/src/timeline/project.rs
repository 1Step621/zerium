use std::{collections::HashMap, sync::Arc};

use super::{
    document::TimelineDocument,
    ids::{ProjectId, SceneId},
    scene::SceneDefinition,
    settings::{BeatGuide, ProjectResolution},
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
    pub(super) fn document_mut(&mut self) -> &mut TimelineDocument {
        Arc::make_mut(&mut self.document)
    }

    pub(super) fn scenes_mut(&mut self) -> &mut HashMap<SceneId, SceneDefinition> {
        Arc::make_mut(&mut self.scenes)
    }
}
