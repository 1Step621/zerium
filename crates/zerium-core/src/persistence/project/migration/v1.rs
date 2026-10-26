//! Version 1 → 2: normalize scene arguments into scene variables.
use serde::Deserialize;
use serde_json::{Map, Value};

use super::invalid_format;
use crate::{
    persistence::{ProjectError, items::ProjectItem, scene::definitions::StoredScene},
    property::PropertySchema,
    timeline::{SceneBindingTarget, SceneVariable},
};

#[derive(Deserialize)]
#[serde(untagged)]
enum Scene {
    Arguments(ArgumentScene),
    Variables(StoredScene),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArgumentScene {
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

pub(super) fn migrate(file: &mut Map<String, Value>) -> Result<(), ProjectError> {
    let scenes = file
        .remove("scenes")
        .ok_or_else(|| ProjectError::invalid_data("Project scenes are missing"))?;
    // Keep unrelated fields, including the old media cache, opaque until their
    // own migration has run. Validate scene contracts before converting them.
    let scenes: Vec<Scene> = serde_json::from_value(scenes).map_err(invalid_format)?;
    let scenes = scenes
        .into_iter()
        .map(|scene| match scene {
            Scene::Variables(scene) => Ok(scene),
            Scene::Arguments(scene) => migrate_scene(scene),
        })
        .collect::<Result<Vec<_>, _>>()?;
    file.insert(
        "scenes".to_owned(),
        serde_json::to_value(scenes).map_err(invalid_format)?,
    );
    file.insert("format_version".to_owned(), Value::from(2));
    Ok(())
}

fn migrate_scene(scene: ArgumentScene) -> Result<StoredScene, ProjectError> {
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
    Ok(StoredScene {
        id: scene.id,
        name: scene.name,
        stretch_point: scene.stretch_point,
        variables,
        items: scene.items,
    })
}
