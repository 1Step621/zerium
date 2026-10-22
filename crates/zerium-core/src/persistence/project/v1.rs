//! Version 1 used both the original arguments and the later variables layout.
use serde::Deserialize;

use super::{FORMAT_VERSION, ProjectFile, ProjectScene};
use crate::persistence::{ProjectError, items::ProjectItem};
use crate::property::PropertySchema;
use crate::timeline::{SceneBindingTarget, SceneVariable};

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Scene {
    Arguments(ArgumentScene),
    Variables(ProjectScene),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ArgumentScene {
    id: u64,
    name: String,
    stretch_point: u64,
    arguments: Vec<Argument>,
    items: Vec<ProjectItem>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Argument {
    schema: PropertySchema,
    bindings: Vec<SceneBindingTarget>,
}

pub(super) fn migrate(file: ProjectFile<Scene>) -> Result<ProjectFile, ProjectError> {
    let scenes = file
        .scenes
        .into_iter()
        .map(|scene| match scene {
            Scene::Variables(scene) => Ok(scene),
            Scene::Arguments(scene) => migrate_scene(scene),
        })
        .collect::<Result<_, _>>()?;
    Ok(ProjectFile {
        format_version: FORMAT_VERSION,
        project_high: file.project_high,
        project_low: file.project_low,
        resolution: file.resolution,
        frame_rate: file.frame_rate,
        beat_guide: file.beat_guide,
        playhead: file.playhead,
        items: file.items,
        scenes,
        media_cache: file.media_cache,
    })
}

fn migrate_scene(scene: ArgumentScene) -> Result<ProjectScene, ProjectError> {
    let variables = scene
        .arguments
        .into_iter()
        .map(|argument| {
            // Validate the old contract before its constraints become evaluation
            // limits; migration must not make an invalid old default acceptable.
            argument
                .schema
                .validate("scene", &scene.name)
                .map_err(|error| {
                    ProjectError::invalid_data(format!("Scene '{}': {error}", scene.name))
                })?;
            let id = argument.schema.id().to_owned();
            SceneVariable::argument(argument.schema, argument.bindings).ok_or_else(|| {
                ProjectError::invalid_data(format!(
                    "Scene '{}' argument '{id}' cannot be migrated",
                    scene.name
                ))
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(ProjectScene {
        id: scene.id,
        name: scene.name,
        stretch_point: scene.stretch_point,
        variables,
        items: scene.items,
    })
}
