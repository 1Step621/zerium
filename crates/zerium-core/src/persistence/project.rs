//! Project file representation, capture, and checked reconstruction.
use super::ProjectError;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::animation::{ScalarAnimations, ScalarTrack};
use crate::plugin::PluginRegistry;
use crate::property::materialized_property_values;
use crate::property::{PropertyPath, PropertySchema, PropertyValue, PropertyValues};
use crate::timeline::{
    EffectInstance, EffectInstanceId, Frame, FrameDuration, FrameRate, ItemId, LayerId, ProjectId,
    ProjectResolution, SceneArgument, SceneBindingTarget, SceneDefinition, SceneId,
    TimelineDocument, TimelineEditor, TimelineItem, TimelineItemKind, TimelineSnapshot,
    TimelineView, resolve_scene_binding,
};

pub const PROJECT_EXTENSION: &str = "zero";
const FORMAT_VERSION: u32 = 1;

pub struct LoadedProject {
    project_id: ProjectId,
    document: TimelineDocument,
    scenes: HashMap<SceneId, SceneDefinition>,
    resolution: ProjectResolution,
    playhead: Frame,
}

impl LoadedProject {
    pub fn apply(self, editor: &mut TimelineEditor) {
        editor.replace_project(
            self.project_id,
            self.document,
            self.scenes,
            self.resolution,
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
    playhead: u64,
    items: Vec<ProjectItem>,
    scenes: Vec<ProjectScene>,
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
            playhead: snapshot.playhead().get(),
            items,
            scenes,
        })
    }

    fn into_loaded(
        self,
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
        for scene in &self.scenes {
            if scene.id == 0 || scene.id == u64::MAX || !scene_ids.insert(scene.id) {
                return Err(ProjectError::invalid_data(format!(
                    "Scene ID {} is invalid or duplicated",
                    scene.id
                )));
            }
            if scene.name.trim().is_empty() {
                return Err(ProjectError::invalid_data("Scene name cannot be empty"));
            }
            let mut argument_ids = HashSet::new();
            for argument in &scene.arguments {
                argument
                    .schema
                    .validate("scene", &scene.name)
                    .map_err(|error| {
                        ProjectError::invalid_data(format!(
                            "Scene '{}' argument '{}' is invalid: {error}",
                            scene.name, argument.schema.id
                        ))
                    })?;
                if argument.schema.clone().for_scene_argument().is_none() {
                    return Err(ProjectError::invalid_data(format!(
                        "Scene '{}' argument '{}' must use a supported scalar type",
                        scene.name, argument.schema.id
                    )));
                }
                if !argument_ids.insert(argument.schema.id.as_str()) {
                    return Err(ProjectError::invalid_data(format!(
                        "Scene '{}' argument '{}' is duplicated",
                        scene.name, argument.schema.id
                    )));
                }
            }
            scene_schemas.insert(
                SceneId::new(project_id, scene.id),
                scene
                    .arguments
                    .iter()
                    .map(|argument| argument.schema.clone())
                    .collect::<Vec<_>>(),
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
        validate_no_overlaps(&items)?;
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
            validate_no_overlaps(&scene_items)?;
            let arguments = scene
                .arguments
                .into_iter()
                .map(ProjectSceneArgument::into_domain)
                .collect::<Result<Vec<_>, _>>()?;
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
            playhead: Frame::new(self.playhead),
        })
    }
}

fn capture_items<'a>(
    items: impl Iterator<Item = &'a TimelineItem>,
    layer_for: impl Fn(ItemId) -> Option<LayerId>,
    project_path: &Path,
) -> Result<Vec<ProjectItem>, ProjectError> {
    let mut captured = items
        .map(|item| {
            let layer = layer_for(item.id).ok_or_else(|| {
                ProjectError::invalid_data(format!(
                    "No layer information found for item {}",
                    item.id.get()
                ))
            })?;
            Ok(ProjectItem::capture(item, layer, project_path))
        })
        .collect::<Result<Vec<_>, ProjectError>>()?;
    captured.sort_by_key(|item| (item.layer, item.start, item.id));
    Ok(captured)
}

pub(super) fn load_items(
    items: Vec<ProjectItem>,
    project_path: &Path,
    scene_schemas: &HashMap<SceneId, Vec<PropertySchema>>,
    effect_ids: &mut HashSet<u64>,
    plugins: &PluginRegistry,
) -> Result<Vec<(LayerId, TimelineItem)>, ProjectError> {
    let mut item_ids = HashSet::new();
    items
        .into_iter()
        .map(|item| {
            if item.id == 0 || item.id == u64::MAX || !item_ids.insert(item.id) {
                return Err(ProjectError::invalid_data(format!(
                    "Item ID {} is invalid or duplicated",
                    item.id
                )));
            }
            item.into_timeline(project_path, effect_ids, scene_schemas, plugins)
        })
        .collect()
}

fn capture_property_overrides(
    values: &PropertyValues,
    schema: Option<&[PropertySchema]>,
    project_path: &Path,
) -> BTreeMap<String, PropertyValue> {
    values
        .iter()
        .filter(|(id, value)| {
            schema
                .and_then(|schema| schema.iter().find(|property| property.id() == *id))
                .is_none_or(|property| property.default_value() != *value)
        })
        .map(|(id, value)| {
            let mut value = value.clone();
            if let PropertyValue::File(Some(path)) = &mut value {
                *path = make_relative(path, project_path);
            }
            (id.to_owned(), value)
        })
        .collect()
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectScene {
    id: u64,
    name: String,
    arguments: Vec<ProjectSceneArgument>,
    items: Vec<ProjectItem>,
}

impl ProjectScene {
    fn capture(scene: &SceneDefinition, project_path: &Path) -> Result<Self, ProjectError> {
        Ok(Self {
            id: scene.id.get(),
            name: scene.name.clone(),
            arguments: scene
                .arguments
                .iter()
                .map(ProjectSceneArgument::capture)
                .collect(),
            items: capture_items(scene.items(), |id| scene.item_layer(id), project_path)?,
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectSceneArgument {
    schema: PropertySchema,
    bindings: Vec<SceneBindingTarget>,
}

impl ProjectSceneArgument {
    fn capture(argument: &SceneArgument) -> Self {
        Self {
            schema: argument.schema.clone(),
            bindings: argument.bindings.clone(),
        }
    }

    fn into_domain(self) -> Result<SceneArgument, ProjectError> {
        let schema = self.schema.for_scene_argument().ok_or_else(|| {
            ProjectError::invalid_data("Scene argument must use a supported scalar type")
        })?;
        Ok(SceneArgument::new(schema, self.bindings))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectItem {
    id: u64,
    layer: u64,
    start: u64,
    duration: u64,
    kind: ProjectItemKind,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    properties: BTreeMap<String, PropertyValue>,
    #[serde(default, skip_serializing_if = "crate::media::MediaInputs::is_empty")]
    media_inputs: crate::media::MediaInputs,
    animations: Vec<ProjectScalarAnimation>,
    aspect_ratio: Option<crate::timeline::AspectRatio>,
    effects: Vec<ProjectEffect>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ProjectItemKind {
    Plugin {
        plugin_id: String,
        item_id: String,
    },
    Scene {
        project_high: u64,
        project_low: u64,
        scene_id: u64,
    },
}

impl ProjectItem {
    pub(super) fn capture(item: &TimelineItem, layer: LayerId, project_path: &Path) -> Self {
        Self {
            id: item.id.get(),
            layer: layer.get(),
            start: item.start.get(),
            duration: item.duration.get(),
            kind: match item.scene_id() {
                Some(scene_id) => ProjectItemKind::Scene {
                    project_high: scene_id.project().high(),
                    project_low: scene_id.project().low(),
                    scene_id: scene_id.get(),
                },
                None => ProjectItemKind::Plugin {
                    plugin_id: item.plugin_id().unwrap_or_default().to_owned(),
                    item_id: item.item_id().unwrap_or_default().to_owned(),
                },
            },
            properties: capture_property_overrides(
                &item.properties,
                item.schema().map(|schema| schema.properties()),
                project_path,
            ),
            media_inputs: item.media_inputs.clone(),
            animations: capture_animations(&item.animations),
            aspect_ratio: item.aspect_ratio,
            effects: item
                .effects
                .iter()
                .map(|effect| ProjectEffect::capture(effect, project_path))
                .collect(),
        }
    }

    fn into_timeline(
        self,
        project_path: &Path,
        effect_ids: &mut HashSet<u64>,
        scene_schemas: &HashMap<SceneId, Vec<PropertySchema>>,
        plugins: &PluginRegistry,
    ) -> Result<(LayerId, TimelineItem), ProjectError> {
        let duration = FrameDuration::new(self.duration).ok_or_else(|| {
            ProjectError::invalid_data("Item duration must be at least one frame")
        })?;
        self.start
            .checked_add(self.duration)
            .ok_or_else(|| ProjectError::invalid_data("Item timestamp is too large"))?;
        let plugin_schema = match &self.kind {
            ProjectItemKind::Scene { .. } => None,
            ProjectItemKind::Plugin { plugin_id, item_id } => {
                Some(plugins.item(plugin_id, item_id).ok_or_else(|| {
                    ProjectError::invalid_data(format!(
                        "Plugin for item '{}:{}' was not found",
                        plugin_id, item_id
                    ))
                })?)
            }
        };
        let property_schema = match &self.kind {
            ProjectItemKind::Scene {
                project_high,
                project_low,
                scene_id,
            } => {
                let project =
                    ProjectId::from_parts(*project_high, *project_low).ok_or_else(|| {
                        ProjectError::invalid_data("Referenced project ID is invalid")
                    })?;
                let scene = SceneId::new(project, *scene_id);
                scene_schemas.get(&scene).ok_or_else(|| {
                    ProjectError::invalid_data(format!("Referenced scene {scene_id} was not found"))
                })?
            }
            ProjectItemKind::Plugin { .. } => plugin_schema
                .as_deref()
                .expect("plugin item has a schema")
                .properties(),
        };
        let scene_instance = matches!(&self.kind, ProjectItemKind::Scene { .. });
        let initial = if scene_instance {
            PropertyValues::default()
        } else {
            PropertyValues::from_properties(property_schema)
        };
        let properties = load_properties(
            property_schema,
            self.properties,
            if scene_instance {
                "scene instance"
            } else {
                "item"
            },
            initial,
            project_path,
        )?;
        let animation_base = materialized_property_values(&properties, property_schema);
        let animations = load_animations(property_schema, &animation_base, self.animations)?;

        if self.aspect_ratio.is_some()
            && plugin_schema
                .as_ref()
                .and_then(|schema| schema.aspect_lock_property())
                .is_none()
        {
            return Err(ProjectError::invalid_data(
                "Aspect ratio is stored for an item without an aspect lock property",
            ));
        }

        let mut effects = Vec::with_capacity(self.effects.len());
        for effect in self.effects {
            if effect.id == 0 || effect.id == u64::MAX || !effect_ids.insert(effect.id) {
                return Err(ProjectError::invalid_data(format!(
                    "Effect ID {} is invalid or duplicated",
                    effect.id
                )));
            }
            effects.push(effect.into_effect(project_path, plugins)?);
        }
        if !effects.is_empty()
            && plugin_schema
                .as_ref()
                .is_some_and(|schema| schema.shader().is_none())
        {
            return Err(ProjectError::invalid_data(
                "Effects cannot be set on items without video",
            ));
        }

        let item = TimelineItem {
            id: ItemId(self.id),
            start: Frame::new(self.start),
            duration,
            kind: match self.kind {
                ProjectItemKind::Scene {
                    project_high,
                    project_low,
                    scene_id,
                } => TimelineItemKind::Scene {
                    scene_id: SceneId::new(
                        ProjectId::from_parts(project_high, project_low)
                            .expect("scene project identity was validated"),
                        scene_id,
                    ),
                },
                ProjectItemKind::Plugin { plugin_id, item_id } => TimelineItemKind::Plugin {
                    plugin_id,
                    item_id,
                    schema: plugin_schema.expect("plugin items were required to have a schema"),
                },
            },
            properties,
            media_inputs: self.media_inputs,
            animations,
            aspect_ratio: self.aspect_ratio,
            effects,
        };
        item.validate_playback()
            .map_err(|error| ProjectError::invalid_data(error.to_string()))?;
        Ok((LayerId::new(self.layer), item))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectEffect {
    id: u64,
    plugin_id: String,
    effect_id: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    properties: BTreeMap<String, PropertyValue>,
    #[serde(default, skip_serializing_if = "crate::media::MediaInputs::is_empty")]
    media_inputs: crate::media::MediaInputs,
    animations: Vec<ProjectScalarAnimation>,
    aspect_ratio: Option<crate::timeline::AspectRatio>,
}

impl ProjectEffect {
    fn capture(effect: &EffectInstance, project_path: &Path) -> Self {
        Self {
            id: effect.id.get(),
            plugin_id: effect.plugin_id.clone(),
            effect_id: effect.effect_id.clone(),
            properties: capture_property_overrides(
                &effect.properties,
                Some(effect.schema().properties()),
                project_path,
            ),
            media_inputs: effect.media_inputs.clone(),
            animations: capture_animations(&effect.animations),
            aspect_ratio: effect.aspect_ratio,
        }
    }

    fn into_effect(
        self,
        project_path: &Path,
        plugins: &PluginRegistry,
    ) -> Result<EffectInstance, ProjectError> {
        let schema = plugins
            .effect(&self.plugin_id, &self.effect_id)
            .ok_or_else(|| {
                ProjectError::invalid_data(format!(
                    "Plugin for effect '{}:{}' was not found",
                    self.plugin_id, self.effect_id
                ))
            })?;
        if self.aspect_ratio.is_some() && schema.aspect_lock_property().is_none() {
            return Err(ProjectError::invalid_data(
                "Aspect ratio is stored for an effect without an aspect lock property",
            ));
        }
        let properties = load_properties(
            schema.properties(),
            self.properties,
            "effect",
            PropertyValues::from_properties(schema.properties()),
            project_path,
        )?;
        let animations = load_animations(schema.properties(), &properties, self.animations)?;
        Ok(EffectInstance {
            id: EffectInstanceId::new(self.id),
            plugin_id: self.plugin_id,
            effect_id: self.effect_id,
            properties,
            media_inputs: self.media_inputs,
            animations,
            aspect_ratio: self.aspect_ratio,
            schema,
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectScalarAnimation {
    path: PropertyPath,
    track: ScalarTrack,
}

fn capture_animations(animations: &ScalarAnimations) -> Vec<ProjectScalarAnimation> {
    animations
        .tracks()
        .map(|(address, track)| ProjectScalarAnimation {
            path: address.clone(),
            track: track.clone(),
        })
        .collect()
}

fn load_animations(
    schema: &[PropertySchema],
    properties: &PropertyValues,
    animations: Vec<ProjectScalarAnimation>,
) -> Result<ScalarAnimations, ProjectError> {
    let mut loaded = ScalarAnimations::default();
    for animation in animations {
        let property = schema
            .iter()
            .find(|property| property.id == animation.path.property_id())
            .ok_or_else(|| {
                ProjectError::invalid_data(format!(
                    "Animation target '{}' was not found",
                    animation.path.property_id()
                ))
            })?;
        validate_project_track(
            &mut loaded,
            properties,
            property,
            animation.path,
            animation.track,
        )?;
    }
    Ok(loaded)
}

fn validate_project_track(
    loaded: &mut ScalarAnimations,
    properties: &PropertyValues,
    property: &PropertySchema,
    path: PropertyPath,
    track: ScalarTrack,
) -> Result<(), ProjectError> {
    let property_id = path.property_id();
    let valid = properties
        .property(property_id)
        .and_then(|value| property.resolve_scalar(value, path.element_id(), path.scalar_index()))
        .is_some_and(|resolved| {
            property.is_animatable(path.scalar_index())
                && track.is_valid_for(resolved.ty)
                && track
                    .stops()
                    .iter()
                    .all(|stop| resolved.configuration.constraints.allows(stop.value()))
        });
    if !valid || loaded.track(&path).is_some() {
        return Err(ProjectError::invalid_data(format!(
            "Animation target for '{property_id}' is invalid"
        )));
    }
    loaded.insert(path, track);
    Ok(())
}

fn load_properties(
    schema: &[PropertySchema],
    values: BTreeMap<String, PropertyValue>,
    owner: &str,
    mut loaded: PropertyValues,
    project_path: &Path,
) -> Result<PropertyValues, ProjectError> {
    for (id, mut value) in values {
        if let PropertyValue::File(Some(path)) = &mut value {
            *path = resolve_path(path, project_path);
        }
        let property = schema
            .iter()
            .find(|property| property.id == id)
            .ok_or_else(|| {
                ProjectError::invalid_data(format!("{owner} has unknown property '{id}'"))
            })?;
        if &value == property.default_value() {
            continue;
        }
        loaded.set(property, value).map_err(|error| {
            ProjectError::invalid_data(format!("{owner} property '{id}' is invalid: {error}"))
        })?;
    }
    Ok(loaded)
}

pub(super) fn validate_no_overlaps(items: &[(LayerId, TimelineItem)]) -> Result<(), ProjectError> {
    let mut ranges = items
        .iter()
        .map(|(layer, item)| (layer.get(), item.start.get(), item.end_exclusive().get()))
        .collect::<Vec<_>>();
    ranges.sort_unstable();
    for pair in ranges.windows(2) {
        if pair[0].0 == pair[1].0 && pair[0].2 > pair[1].1 {
            return Err(ProjectError::invalid_data(format!(
                "Items overlap on layer {}",
                pair[0].0
            )));
        }
    }
    Ok(())
}

fn validate_scenes(
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
                if resolved.schema.ty() != argument.schema.ty() {
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

    let validate_instances = |items: Vec<&TimelineItem>| -> Result<(), ProjectError> {
        for item in items {
            if let Some(scene_id) = item.scene_id()
                && !scenes.contains_key(&scene_id)
            {
                return Err(ProjectError::invalid_data(format!(
                    "Referenced scene {} was not found",
                    scene_id.get()
                )));
            }
        }
        Ok(())
    };
    validate_instances(root_items.iter().map(|(_, item)| item).collect())?;
    for scene in scenes.values() {
        validate_instances(scene.items().collect())?;
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

fn make_relative(path: &Path, project_path: &Path) -> PathBuf {
    let Some(directory) = project_path.parent() else {
        return path.to_path_buf();
    };
    path.strip_prefix(directory)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

fn resolve_path(path: &Path, project_path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    project_path
        .parent()
        .map(|directory| directory.join(path))
        .unwrap_or_else(|| path.to_path_buf())
}
