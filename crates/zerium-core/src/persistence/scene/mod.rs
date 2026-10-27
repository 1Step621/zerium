//! Portable scene files with their nested scene definitions.
pub(super) mod definitions;

use std::{collections::HashSet, path::Path};

use serde::{Deserialize, Serialize};

use self::definitions::{StoredScene, load_scenes};
use super::{DocumentKind, FORMAT_VERSION, ProjectError, project::migration};
use crate::{
    plugin::PluginRegistry,
    timeline::{
        FrameRate, ProjectId, SceneDefinition, SceneId, TimelineEditError, TimelineEditor,
        TimelineItem, TimelineSnapshot, TimelineView,
    },
};

/// Suggested filename suffix; the OS extension is still `.zero`.
pub const SCENE_FILE_SUFFIX: &str = ".scene.zero";

pub struct LoadedScene {
    root: SceneId,
    scenes: Vec<SceneDefinition>,
}

impl LoadedScene {
    pub fn import(self, editor: &mut TimelineEditor) -> Result<SceneId, TimelineEditError> {
        editor.import_scenes(self.root, self.scenes)
    }
}

pub fn encode_scene(
    snapshot: &TimelineSnapshot,
    root: SceneId,
    path: &Path,
) -> Result<String, ProjectError> {
    let mut pending = vec![root];
    let mut visited = HashSet::new();
    let mut scenes = Vec::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let scene = snapshot
            .scene(id)
            .filter(|scene| scene.id.project() == snapshot.project_id())
            .ok_or_else(|| ProjectError::invalid_data("Referenced scene was not found"))?;
        pending.extend(scene.items().filter_map(TimelineItem::scene_id));
        scenes.push(StoredScene::capture(scene, path)?);
    }
    scenes.sort_by_key(|scene| scene.id);
    let file = SceneFile {
        kind: DocumentKind::Scene,
        format_version: FORMAT_VERSION,
        project_high: root.project().high(),
        project_low: root.project().low(),
        frame_rate: [
            snapshot.frame_rate().numerator(),
            snapshot.frame_rate().denominator(),
        ],
        root_scene: root.get(),
        scenes,
    };
    serde_json::to_string_pretty(&file)
        .map_err(|error| ProjectError::encode(format!("Failed to serialize scene: {error}"), error))
}

pub fn decode_scene(
    source: &str,
    path: &Path,
    plugins: &PluginRegistry,
) -> Result<LoadedScene, ProjectError> {
    let file: SceneFile = migration::decode(source, plugins)?;
    if file.kind != DocumentKind::Scene {
        return Err(ProjectError::invalid_data(
            "The selected file is not a scene file",
        ));
    }
    let project = ProjectId::from_parts(file.project_high, file.project_low)
        .ok_or_else(|| ProjectError::invalid_data("Invalid scene project ID"))?;
    let frame_rate = FrameRate::new(file.frame_rate[0], file.frame_rate[1])
        .ok_or_else(|| ProjectError::invalid_data("Invalid scene frame rate"))?;
    let root = SceneId::new(project, file.root_scene);
    let scenes = load_scenes(file.scenes, project, frame_rate, root, path, plugins)?;
    Ok(LoadedScene {
        root,
        scenes: scenes.into_values().collect(),
    })
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SceneFile {
    #[serde(rename = "type")]
    kind: DocumentKind,
    format_version: u32,
    project_high: u64,
    project_low: u64,
    frame_rate: [u32; 2],
    root_scene: u64,
    scenes: Vec<StoredScene>,
}
