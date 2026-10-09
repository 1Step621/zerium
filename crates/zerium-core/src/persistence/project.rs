//! Project file representation, capture, and checked reconstruction.
use super::{
    ProjectError,
    items::{ProjectItem, capture_items, load_items},
    paths::{item_files, make_relative, resolve_path},
    scenes::{ProjectScene, ProjectSceneArgument, validate_scenes},
};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::plugin::PluginRegistry;
use crate::timeline::{
    BeatGuide, Frame, FrameRate, ProjectId, ProjectResolution, SceneDefinition, SceneId,
    TimelineDocument, TimelineEditor, TimelineSnapshot, TimelineView,
};

pub const PROJECT_EXTENSION: &str = "zero";
const FORMAT_VERSION: u32 = 1;

pub struct LoadedProject {
    project_id: ProjectId,
    document: TimelineDocument,
    scenes: HashMap<SceneId, SceneDefinition>,
    resolution: ProjectResolution,
    beat_guide: BeatGuide,
    playhead: Frame,
    media_cache: crate::media::MediaMetadataCache,
}

impl LoadedProject {
    pub fn apply(self, editor: &mut TimelineEditor) {
        editor.replace_project(
            self.project_id,
            self.document,
            self.scenes,
            self.resolution,
            self.beat_guide,
            self.playhead,
        );
        editor.replace_media_cache(self.media_cache);
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
    let file = serde_json::from_str::<ProjectFile>(source).map_err(|error| {
        ProjectError::invalid_format(format!("Invalid project file format: {error}"), error)
    })?;
    file.into_loaded(project_path, plugins)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectFile {
    format_version: u32,
    project_high: u64,
    project_low: u64,
    resolution: [u32; 2],
    frame_rate: [u32; 2],
    #[serde(default)]
    beat_guide: BeatGuide,
    playhead: u64,
    items: Vec<ProjectItem>,
    scenes: Vec<ProjectScene>,
    #[serde(default)]
    media_cache: crate::media::MediaMetadataCache,
}

impl ProjectFile {
    fn capture(snapshot: &TimelineSnapshot, project_path: &Path) -> Result<Self, ProjectError> {
        let items = capture_items(snapshot.items(), |id| snapshot.item_layer(id), project_path)?;
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
            items,
            scenes,
            media_cache: snapshot
                .media_cache()
                .retained_paths(
                    snapshot
                        .items()
                        .chain(snapshot.scenes().flat_map(|scene| scene.items()))
                        .flat_map(item_files)
                        .chain(snapshot.scenes().flat_map(|scene| {
                            scene
                                .arguments
                                .iter()
                                .filter_map(|arg| arg.schema.default_value().file())
                        })),
                )
                .mapped_paths(|path| make_relative(path, project_path)),
        })
    }

    fn into_loaded(
        mut self,
        project_path: &Path,
        plugins: &PluginRegistry,
    ) -> Result<LoadedProject, ProjectError> {
        if self.format_version != FORMAT_VERSION {
            return Err(ProjectError::unsupported_format(format!(
                "Unsupported project format (version {})",
                self.format_version
            )));
        }
        let project_id = ProjectId::from_parts(self.project_high, self.project_low)
            .ok_or_else(|| ProjectError::invalid_data("Invalid project ID"))?;
        let frame_rate = FrameRate::new(self.frame_rate[0], self.frame_rate[1])
            .ok_or_else(|| ProjectError::invalid_data("Invalid frame rate"))?;
        let resolution = ProjectResolution::new(self.resolution[0], self.resolution[1])
            .ok_or_else(|| ProjectError::invalid_data("Invalid resolution"))?;
        let mut scene_ids = HashSet::new();
        let mut scene_schemas = HashMap::new();
        for scene in &mut self.scenes {
            if scene.id == 0 || scene.id == u64::MAX || !scene_ids.insert(scene.id) {
                return Err(ProjectError::invalid_data(format!(
                    "Scene ID {} is invalid or duplicated",
                    scene.id
                )));
            }
            scene_schemas.insert(
                SceneId::new(project_id, scene.id),
                scene.prepare_arguments(project_path)?,
            );
        }

        let mut effect_ids = HashSet::new();
        let items = load_items(
            self.items,
            project_path,
            &scene_schemas,
            &mut effect_ids,
            plugins,
        )?;
        let mut scenes = HashMap::new();
        for scene in self.scenes {
            let scene_id = SceneId::new(project_id, scene.id);
            let scene_items = load_items(
                scene.items,
                project_path,
                &scene_schemas,
                &mut effect_ids,
                plugins,
            )?;
            let arguments = scene
                .arguments
                .into_iter()
                .map(ProjectSceneArgument::into_domain)
                .collect();
            scenes.insert(
                scene_id,
                SceneDefinition::from_project(
                    scene_id,
                    scene.name,
                    arguments,
                    TimelineDocument::from_items(frame_rate, scene_items),
                ),
            );
        }
        validate_scenes(&items, &scenes)?;
        Ok(LoadedProject {
            project_id,
            document: TimelineDocument::from_items(frame_rate, items),
            scenes,
            resolution,
            beat_guide: self.beat_guide,
            playhead: Frame::new(self.playhead),
            media_cache: self
                .media_cache
                .mapped_paths(|path| resolve_path(path, project_path)),
        })
    }
}
