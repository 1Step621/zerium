//! Scene definitions shared by project and scene files.
use crate::{
    persistence::{
        ProjectError,
        items::{ProjectItem, capture_items, load_items},
        make_relative, resolve_path,
    },
    plugin::PluginRegistry,
    property::PropertySchema,
    timeline::{
        Frame, FrameRate, LayerId, ProjectId, SceneDefinition, SceneId, SceneVariable,
        TimelineDocument, TimelineItem, resolve_scene_binding,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::persistence) struct StoredScene {
    pub(in crate::persistence) id: u64,
    pub(in crate::persistence) name: String,
    pub(in crate::persistence) stretch_point: u64,
    pub(in crate::persistence) variables: Vec<SceneVariable>,
    pub(in crate::persistence) items: Vec<ProjectItem>,
    pub(in crate::persistence) hidden_layers: Vec<u64>,
}

impl StoredScene {
    pub(in crate::persistence) fn capture(
        scene: &SceneDefinition,
        project_path: &Path,
    ) -> Result<Self, ProjectError> {
        Ok(Self {
            id: scene.id.get(),
            name: scene.name.clone(),
            stretch_point: scene.stretch_point().get(),
            variables: scene
                .variables()
                .iter()
                .cloned()
                .map(|mut variable| {
                    if let Some(schema) = variable.argument_schema_mut() {
                        let mut value = schema.default_value();
                        value.map_file_paths(&mut |path| make_relative(path, project_path));
                        schema.set_default(value);
                    }
                    variable
                })
                .collect(),
            items: capture_items(scene.items(), |id| scene.item_layer(id), project_path)?,
            hidden_layers: {
                let mut layers = scene
                    .hidden_layer_ids()
                    .map(LayerId::get)
                    .collect::<Vec<_>>();
                layers.sort_unstable();
                layers
            },
        })
    }

    /// Resolve file defaults and prepare argument contracts before loading any instances.
    fn prepare_variables(
        &mut self,
        project_path: &Path,
    ) -> Result<Vec<PropertySchema>, ProjectError> {
        if self.name.trim().is_empty() {
            return Err(ProjectError::invalid_data("Scene name cannot be empty"));
        }
        let mut ids = HashSet::new();
        let mut schemas = Vec::with_capacity(self.variables.len());
        for variable in &mut self.variables {
            if !ids.insert(variable.id().to_owned()) {
                return Err(ProjectError::invalid_data("Duplicate scene variable ID"));
            }
            if let Some(schema) = variable.argument_schema_mut() {
                let mut default = schema.default_value();
                default.map_file_paths(&mut |path| resolve_path(path, project_path));
                schema.set_default(default);
                schema.validate("scene", &self.name).map_err(|error| {
                    ProjectError::invalid_data(format!("Scene '{}': {error}", self.name))
                })?;
                schema.configuration_mut(None).editable = true;
                schema.configuration_mut(None).scene_bindable = true;
                schemas.push(schema.clone());
            }
        }
        Ok(schemas)
    }
}

fn validate_scenes(
    root_scene: SceneId,
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
        scene.validate_calculations().map_err(|error| {
            ProjectError::invalid_data(format!("Scene '{}': {error}", scene.name))
        })?;
        let mut bound_targets = HashSet::new();
        for argument in scene.variables() {
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
                if !resolved
                    .schema
                    .scalar_type(None, None)
                    .is_some_and(|ty| ty.same_type(argument.ty()))
                {
                    return Err(ProjectError::invalid_data(format!(
                        "Scene '{}' argument '{}' does not match the binding target type",
                        scene.name,
                        argument.id()
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

    for item in scenes.values().flat_map(|scene| scene.items()) {
        if item.scene_id() == Some(root_scene) {
            return Err(ProjectError::invalid_data(
                "Root scene cannot be placed as an instance",
            ));
        }
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

pub(in crate::persistence) fn load_scenes(
    mut definitions: Vec<StoredScene>,
    project_id: ProjectId,
    frame_rate: FrameRate,
    root_scene: SceneId,
    project_path: &Path,
    plugins: &PluginRegistry,
) -> Result<HashMap<SceneId, SceneDefinition>, ProjectError> {
    let mut scene_ids = HashSet::new();
    let mut scene_schemas = HashMap::new();
    for scene in &mut definitions {
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
    for scene in definitions {
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
            TimelineDocument::from_items(
                frame_rate,
                scene_items,
                scene.hidden_layers.into_iter().map(LayerId::new),
            ),
        );
        if scene.stretch_point >= definition.duration().get() {
            return Err(ProjectError::invalid_data(
                "Scene stretch point is outside its duration",
            ));
        }
        definition.set_stretch_point(Frame::new(scene.stretch_point));
        scenes.insert(scene_id, definition);
    }
    if !scenes.contains_key(&root_scene) {
        return Err(ProjectError::invalid_data("Root scene was not found"));
    }
    validate_scenes(root_scene, &scenes)?;
    Ok(scenes)
}
