//! Version 1 → 2: normalize scene arguments into scene variables.
use serde::Deserialize;
use serde_json::Value;

use super::{Migration, invalid_format};
use crate::{
    persistence::ProjectError,
    property::PropertySchema,
    timeline::{SceneBindingTarget, SceneVariable},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Argument {
    schema: PropertySchema,
    bindings: Vec<SceneBindingTarget>,
}

pub(super) fn migrate(migration: &mut Migration<'_>) -> Result<(), ProjectError> {
    let scenes = migration
        .file
        .get_mut("scenes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| ProjectError::invalid_data("Project scenes are missing"))?;
    for scene in scenes {
        let scene = scene
            .as_object_mut()
            .ok_or_else(|| ProjectError::invalid_data("Invalid scene"))?;
        let Some(arguments) = scene.remove("arguments") else {
            continue;
        };
        if scene.contains_key("variables") {
            return Err(ProjectError::invalid_data(
                "Scene cannot contain both arguments and variables",
            ));
        }
        let name = scene
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| ProjectError::invalid_data("Scene name is missing"))?;
        let arguments: Vec<Argument> = serde_json::from_value(arguments).map_err(invalid_format)?;
        let variables = arguments
            .into_iter()
            .map(|argument| {
                // Check the old constraints before converting them to evaluation limits.
                argument.schema.validate("scene", name).map_err(|error| {
                    ProjectError::invalid_data(format!("Scene '{name}': {error}"))
                })?;
                let id = argument.schema.id().to_owned();
                SceneVariable::argument(argument.schema, argument.bindings).ok_or_else(|| {
                    ProjectError::invalid_data(format!(
                        "Scene '{name}' argument '{id}' cannot be migrated"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        scene.insert(
            "variables".into(),
            serde_json::to_value(variables).map_err(invalid_format)?,
        );
    }
    migration.format_version = 2;
    Ok(())
}
