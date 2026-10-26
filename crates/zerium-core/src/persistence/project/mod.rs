//! Project file representation, capture, and checked reconstruction.
mod migration;

use super::{
    ProjectError,
    scene::definitions::{StoredScene, load_scenes},
};
use std::{collections::HashMap, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    plugin::PluginRegistry,
    timeline::{
        BeatGuide, Frame, FrameRate, ProjectId, ProjectResolution, SceneDefinition, SceneId,
        TimelineEditor, TimelineSnapshot, TimelineView,
    },
};

pub const PROJECT_EXTENSION: &str = "zero";
const FORMAT_VERSION: u32 = 6;

#[derive(Clone)]
pub struct LoadedProject {
    project_id: ProjectId,
    main_scene: SceneId,
    scenes: HashMap<SceneId, SceneDefinition>,
    resolution: ProjectResolution,
    beat_guide: BeatGuide,
    playhead: Frame,
}

impl LoadedProject {
    pub fn apply(self, editor: &mut TimelineEditor) {
        editor.replace_project(
            self.project_id,
            self.main_scene,
            self.scenes,
            self.resolution,
            self.beat_guide,
            self.playhead,
        );
    }
}

pub fn encode(snapshot: &TimelineSnapshot, project_path: &Path) -> Result<String, ProjectError> {
    let file = ProjectFile::capture(snapshot, project_path)?;
    serde_json::to_string_pretty(&file).map_err(|error| {
        ProjectError::encode(format!("Failed to serialize project: {error}"), error)
    })
}

pub fn decode(
    source: &str,
    project_path: &Path,
    plugins: &PluginRegistry,
) -> Result<LoadedProject, ProjectError> {
    migration::decode(source, plugins)?.into_loaded(project_path, plugins)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectFile {
    format_version: u32,
    project_high: u64,
    project_low: u64,
    resolution: [u32; 2],
    frame_rate: [u32; 2],
    beat_guide: BeatGuide,
    playhead: u64,
    main_scene: u64,
    scenes: Vec<StoredScene>,
}

impl ProjectFile {
    fn capture(snapshot: &TimelineSnapshot, project_path: &Path) -> Result<Self, ProjectError> {
        let mut scenes = snapshot
            .scenes()
            .map(|scene| {
                if scene.id.project() != snapshot.project_id() {
                    return Err(ProjectError::invalid_data(
                        "Cannot save a scene from another project",
                    ));
                }
                StoredScene::capture(scene, project_path)
            })
            .collect::<Result<Vec<_>, _>>()?;
        scenes.sort_by_key(|scene| scene.id);
        Ok(Self {
            format_version: FORMAT_VERSION,
            project_high: snapshot.project_id().high(),
            project_low: snapshot.project_id().low(),
            resolution: [
                snapshot.resolution().width(),
                snapshot.resolution().height(),
            ],
            frame_rate: [
                snapshot.frame_rate().numerator(),
                snapshot.frame_rate().denominator(),
            ],
            beat_guide: snapshot.beat_guide(),
            playhead: snapshot.playhead().get(),
            main_scene: snapshot.main_scene_id().get(),
            scenes,
        })
    }

    fn into_loaded(
        self,
        project_path: &Path,
        plugins: &PluginRegistry,
    ) -> Result<LoadedProject, ProjectError> {
        let project_id = ProjectId::from_parts(self.project_high, self.project_low)
            .ok_or_else(|| ProjectError::invalid_data("Invalid project ID"))?;
        let frame_rate = FrameRate::new(self.frame_rate[0], self.frame_rate[1])
            .ok_or_else(|| ProjectError::invalid_data("Invalid frame rate"))?;
        let resolution = ProjectResolution::new(self.resolution[0], self.resolution[1])
            .ok_or_else(|| ProjectError::invalid_data("Invalid resolution"))?;
        let main_scene = SceneId::new(project_id, self.main_scene);
        let scenes = load_scenes(
            self.scenes,
            project_id,
            frame_rate,
            main_scene,
            project_path,
            plugins,
        )?;
        Ok(LoadedProject {
            project_id,
            main_scene,
            scenes,
            resolution,
            beat_guide: self.beat_guide,
            playhead: Frame::new(self.playhead),
        })
    }
}
