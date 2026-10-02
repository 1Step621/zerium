use std::collections::HashMap;

use super::{
    document::TimelineDocument,
    ids::{ProjectId, SceneId},
    scene::SceneDefinition,
    settings::ProjectResolution,
};

/// Persistent state for one timeline project. Mutations are coordinated by
/// `TimelineEditor`, which also exposes immutable snapshots to background work.
#[derive(Clone)]
pub(super) struct TimelineProject {
    pub(super) id: ProjectId,
    pub(super) document: TimelineDocument,
    pub(super) scenes: HashMap<SceneId, SceneDefinition>,
    pub(super) resolution: ProjectResolution,
}
