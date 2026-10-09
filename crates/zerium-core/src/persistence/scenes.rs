//! Persisted scenes, argument preparation, and whole-project scene validation.
use super::{
    ProjectError,
    items::{ProjectItem, capture_items},
    paths::{make_relative, resolve_path},
};
use crate::{
    property::PropertySchema,
    timeline::{
        LayerId, SceneArgument, SceneBindingTarget, SceneDefinition, SceneId, TimelineItem,
        resolve_scene_binding,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectScene {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) arguments: Vec<ProjectSceneArgument>,
    pub(super) items: Vec<ProjectItem>,
}

impl ProjectScene {
    pub(super) fn capture(
        scene: &SceneDefinition,
        project_path: &Path,
    ) -> Result<Self, ProjectError> {
        Ok(Self {
            id: scene.id.get(),
            name: scene.name.clone(),
            arguments: scene
                .arguments
                .iter()
                .map(|argument| ProjectSceneArgument::capture(argument, project_path))
                .collect(),
            items: capture_items(scene.items(), |id| scene.item_layer(id), project_path)?,
        })
    }

    /// Resolve file defaults and prepare argument contracts before loading any instances.
    pub(super) fn prepare_arguments(
        &mut self,
        project_path: &Path,
    ) -> Result<Vec<PropertySchema>, ProjectError> {
        if self.name.trim().is_empty() {
            return Err(ProjectError::invalid_data("Scene name cannot be empty"));
        }
        let mut ids = HashSet::new();
        let mut schemas = Vec::with_capacity(self.arguments.len());
        for argument in &mut self.arguments {
            let mut default = argument.schema.default_value();
            default.map_file_paths(&mut |path| resolve_path(path, project_path));
            argument.schema.set_default(default);
            argument
                .schema
                .validate("scene", &self.name)
                .map_err(|error| {
                    ProjectError::invalid_data(format!(
                        "Scene '{}' argument '{}' is invalid: {error}",
                        self.name, argument.schema.id
                    ))
                })?;
            argument.schema = argument
                .schema
                .clone()
                .for_scene_argument()
                .ok_or_else(|| {
                    ProjectError::invalid_data(format!(
                        "Scene '{}' argument '{}' must use a supported argument type",
                        self.name, argument.schema.id
                    ))
                })?;
            if !ids.insert(argument.schema.id.as_str()) {
                return Err(ProjectError::invalid_data(format!(
                    "Scene '{}' argument '{}' is duplicated",
                    self.name, argument.schema.id
                )));
            }
            schemas.push(argument.schema.clone());
        }
        Ok(schemas)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectSceneArgument {
    schema: PropertySchema,
    bindings: Vec<SceneBindingTarget>,
}

impl ProjectSceneArgument {
    fn capture(argument: &SceneArgument, project_path: &Path) -> Self {
        let mut schema = argument.schema.clone();
        let mut default = schema.default_value();
        default.map_file_paths(&mut |path| make_relative(path, project_path));
        schema.set_default(default);
        Self {
            schema,
            bindings: argument.bindings.clone(),
        }
    }

    pub(super) fn into_domain(self) -> SceneArgument {
        SceneArgument::new(self.schema, self.bindings)
    }
}

pub(super) fn validate_scenes(
    root_items: &[(LayerId, TimelineItem)],
    scenes: &HashMap<SceneId, SceneDefinition>,
) -> Result<(), ProjectError> {
    let mut names = HashSet::new();
    for scene in scenes.values() {
        if !names.insert(scene.name.as_str()) {
            return Err(ProjectError::invalid_data(format!(
                "Scene name '{}' is duplicated",
                scene.name
            )));
        }
        let mut bound_targets = HashSet::new();
        for argument in &scene.arguments {
            for binding in &argument.bindings {
                if !bound_targets.insert(binding) {
                    return Err(ProjectError::invalid_data(format!(
                        "Binding target is used more than once in scene '{}'",
                        scene.name
                    )));
                }
                let property_id = binding.property_id();
                let resolved = resolve_scene_binding(scenes, scene, binding).ok_or_else(|| {
                    ProjectError::invalid_data(format!(
                        "Scene '{}' references missing argument binding target '{}'",
                        scene.name, property_id
                    ))
                })?;
                if !resolved.schema.is_scene_bindable(None) {
                    return Err(ProjectError::invalid_data(format!(
                        "Scene '{}' binding target '{}' cannot be exposed as a scene argument",
                        scene.name, property_id
                    )));
                }
                if !resolved.schema.same_type(&argument.schema) {
                    return Err(ProjectError::invalid_data(format!(
                        "Scene '{}' argument '{}' does not match the binding target type",
                        scene.name,
                        argument.schema.id()
                    )));
                }
                if resolved.animated {
                    return Err(ProjectError::invalid_data(format!(
                        "Scene arguments cannot be bound to animated property '{}'",
                        property_id
                    )));
                }
            }
        }
    }

    for item in root_items
        .iter()
        .map(|(_, item)| item)
        .chain(scenes.values().flat_map(|scene| scene.items()))
    {
        if let Some(scene_id) = item.scene_id()
            && !scenes.contains_key(&scene_id)
        {
            return Err(ProjectError::invalid_data(format!(
                "Referenced scene {} was not found",
                scene_id.get()
            )));
        }
    }

    fn visit(
        scene_id: SceneId,
        scenes: &HashMap<SceneId, SceneDefinition>,
        visiting: &mut HashSet<SceneId>,
        visited: &mut HashSet<SceneId>,
    ) -> Result<(), ProjectError> {
        if visited.contains(&scene_id) {
            return Ok(());
        }
        if !visiting.insert(scene_id) {
            return Err(ProjectError::invalid_data(
                "Scene references contain a cycle",
            ));
        }
        let scene = scenes
            .get(&scene_id)
            .ok_or_else(|| ProjectError::invalid_data("Referenced scene was not found"))?;
        for nested in scene.items().filter_map(TimelineItem::scene_id) {
            visit(nested, scenes, visiting, visited)?;
        }
        visiting.remove(&scene_id);
        visited.insert(scene_id);
        Ok(())
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for scene_id in scenes.keys().copied() {
        visit(scene_id, scenes, &mut visiting, &mut visited)?;
    }
    Ok(())
}
