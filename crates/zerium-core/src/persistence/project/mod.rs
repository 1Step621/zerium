//! Project file representation, capture, and checked reconstruction.
mod migration;
mod scenes;

use super::{ProjectError, items::load_items};
use scenes::{ProjectScene, validate_scenes};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{
    plugin::PluginRegistry,
    timeline::{
        BeatGuide, Frame, FrameRate, ProjectId, ProjectResolution, SceneDefinition, SceneId,
        TimelineDocument, TimelineEditor, TimelineSnapshot, TimelineView,
    },
};

pub const PROJECT_EXTENSION: &str = "zero";
const FORMAT_VERSION: u32 = 6;

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
    scenes: Vec<ProjectScene>,
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
                ProjectScene::capture(scene, project_path)
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
        mut self,
        project_path: &Path,
        plugins: &PluginRegistry,
    ) -> Result<LoadedProject, ProjectError> {
        let project_id = ProjectId::from_parts(self.project_high, self.project_low)
            .ok_or_else(|| ProjectError::invalid_data("Invalid project ID"))?;
        let frame_rate = FrameRate::new(self.frame_rate[0], self.frame_rate[1])
            .ok_or_else(|| ProjectError::invalid_data("Invalid frame rate"))?;
        let resolution = ProjectResolution::new(self.resolution[0], self.resolution[1])
            .ok_or_else(|| ProjectError::invalid_data("Invalid resolution"))?;
        let mut scene_ids = HashSet::new();
        let mut scene_schemas = HashMap::new();
        for scene in &mut self.scenes {
            if scene.id == u64::MAX || !scene_ids.insert(scene.id) {
                return Err(ProjectError::invalid_data(format!(
                    "Scene ID {} is invalid or duplicated",
                    scene.id
                )));
            }
            scene_schemas.insert(
                SceneId::new(project_id, scene.id),
                scene.prepare_variables(project_path)?,
            );
        }

        let mut effect_ids = HashSet::new();
        let mut scenes = HashMap::new();
        for scene in self.scenes {
            let scene_id = SceneId::new(project_id, scene.id);
            let scene_items = load_items(
                scene.items,
                project_path,
                &scene_schemas,
                &mut effect_ids,
                plugins,
                frame_rate,
            )?;
            let variables = scene.variables;
            let mut definition = SceneDefinition::from_project(
                scene_id,
                scene.name,
                variables,
                TimelineDocument::from_items(frame_rate, scene_items),
            );
            if scene.stretch_point >= definition.duration().get() {
                return Err(ProjectError::invalid_data(
                    "Scene stretch point is outside its duration",
                ));
            }
            definition.set_stretch_point(Frame::new(scene.stretch_point));
            scenes.insert(scene_id, definition);
        }
        let main_scene = SceneId::new(project_id, self.main_scene);
        if !scenes.contains_key(&main_scene) {
            return Err(ProjectError::invalid_data("Main scene was not found"));
        }
        validate_scenes(main_scene, &scenes)?;
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
