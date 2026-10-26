//! Shared persisted items, effects, properties, and animation tracks.
use super::{ProjectError, make_relative, resolve_path};
use crate::{
    animation::{ScalarAnimations, ScalarTrack},
    plugin::PluginRegistry,
    property::{PropertyOverrides, PropertyPath, PropertySchema, PropertyValue, PropertyValues},
    timeline::{
        BlendMode, EffectInstance, EffectInstanceId, Frame, FrameDuration, FrameRate, ItemId,
        LayerId, ProjectId, SceneId, TimelineItem, TimelineItemKind,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};

pub(super) fn capture_items<'a>(
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
    frame_rate: FrameRate,
) -> Result<Vec<(LayerId, TimelineItem)>, ProjectError> {
    let mut item_ids = HashSet::new();
    let items = items
        .into_iter()
        .map(|item| {
            if item.id == 0 || item.id == u64::MAX || !item_ids.insert(item.id) {
                return Err(ProjectError::invalid_data(format!(
                    "Item ID {} is invalid or duplicated",
                    item.id
                )));
            }
            item.into_timeline(project_path, effect_ids, scene_schemas, plugins, frame_rate)
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_no_overlaps(&items, frame_rate)?;
    Ok(items)
}

fn capture_property_overrides(
    values: &PropertyOverrides,
    project_path: &Path,
) -> BTreeMap<String, PropertyValue> {
    values
        .iter()
        .map(|(id, value)| {
            let mut value = value.clone();
            value.map_file_paths(&mut |path| make_relative(path, project_path));
            (id.to_owned(), value)
        })
        .collect()
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectItem {
    id: u64,
    layer: u64,
    start: Frame,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration: Option<FrameDuration>,
    blend_mode: BlendMode,
    kind: ProjectItemKind,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    properties: BTreeMap<String, PropertyValue>,
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
    pub(super) fn apply_clipboard_property(
        &mut self,
        item_id: crate::timeline::ItemId,
        owner: crate::timeline::SceneBindingOwner,
        property_id: &str,
        value: &PropertyValue,
    ) -> bool {
        if self.id != item_id.get() {
            return false;
        }
        let properties = match owner {
            crate::timeline::SceneBindingOwner::Item => &mut self.properties,
            crate::timeline::SceneBindingOwner::Effect(id) => {
                let Some(effect) = self.effects.iter_mut().find(|effect| effect.id == id.get())
                else {
                    return false;
                };
                &mut effect.properties
            }
        };
        properties.insert(property_id.to_owned(), value.clone());
        true
    }

    pub(super) fn capture(item: &TimelineItem, layer: LayerId, project_path: &Path) -> Self {
        Self {
            id: item.id.get(),
            layer: layer.get(),
            start: item.start(),
            duration: item.fixed_duration,
            blend_mode: item.blend_mode,
            kind: match &item.kind {
                TimelineItemKind::Scene { scene_id } => ProjectItemKind::Scene {
                    project_high: scene_id.project().high(),
                    project_low: scene_id.project().low(),
                    scene_id: scene_id.get(),
                },
                TimelineItemKind::Plugin {
                    plugin_id, item_id, ..
                } => ProjectItemKind::Plugin {
                    plugin_id: plugin_id.clone(),
                    item_id: item_id.clone(),
                },
            },
            properties: capture_property_overrides(&item.properties, project_path),
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
        frame_rate: FrameRate,
    ) -> Result<(LayerId, TimelineItem), ProjectError> {
        let kind = match self.kind {
            ProjectItemKind::Plugin { plugin_id, item_id } => {
                let schema = plugins.item(&plugin_id, &item_id).ok_or_else(|| {
                    ProjectError::invalid_data(format!(
                        "Plugin for item '{}:{}' was not found",
                        plugin_id, item_id
                    ))
                })?;
                TimelineItemKind::Plugin {
                    plugin_id,
                    item_id,
                    schema,
                }
            }
            ProjectItemKind::Scene {
                project_high,
                project_low,
                scene_id,
            } => {
                let project =
                    ProjectId::from_parts(project_high, project_low).ok_or_else(|| {
                        ProjectError::invalid_data("Referenced project ID is invalid")
                    })?;
                TimelineItemKind::Scene {
                    scene_id: SceneId::new(project, scene_id),
                }
            }
        };
        let (property_schema, owner) = match &kind {
            TimelineItemKind::Plugin { schema, .. } => (schema.properties(), "item"),
            TimelineItemKind::Scene { scene_id } => (
                scene_schemas
                    .get(scene_id)
                    .ok_or_else(|| {
                        ProjectError::invalid_data(format!(
                            "Referenced scene {} was not found",
                            scene_id.get()
                        ))
                    })?
                    .as_slice(),
                "scene instance",
            ),
        };
        let properties = load_properties(property_schema, self.properties, owner, project_path)?;
        let duration = match &kind {
            TimelineItemKind::Plugin { schema, .. } if schema.playback().is_some() => schema
                .playback()
                .expect("playback")
                .mapping(&properties.materialize(property_schema))
                .map_err(|error| ProjectError::invalid_data(error.to_string()))?
                .timeline_duration(frame_rate),
            _ => self
                .duration
                .ok_or_else(|| ProjectError::invalid_data("Fixed item duration is missing"))?,
        };
        let animation_base = properties.materialize(property_schema);
        let animations =
            load_animations(property_schema, &animation_base, self.animations, duration)?;

        let aspect_lock_property = if let TimelineItemKind::Plugin { schema, .. } = &kind {
            schema.aspect_lock_property()
        } else {
            None
        };
        if self.aspect_ratio.is_some() && aspect_lock_property.is_none() {
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
            effects.push(effect.into_effect(project_path, plugins, duration)?);
        }
        if let TimelineItemKind::Plugin { schema, .. } = &kind
            && !effects.is_empty()
            && schema.render().is_none()
        {
            return Err(ProjectError::invalid_data(
                "Effects cannot be set on items without video",
            ));
        }

        let item = TimelineItem {
            id: ItemId(self.id),
            start: self.start,
            fixed_duration: self.duration,
            blend_mode: self.blend_mode,
            kind,
            properties,
            animations,
            aspect_ratio: self.aspect_ratio,
            effects,
        };
        item.validate_playback(frame_rate)
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
    animations: Vec<ProjectScalarAnimation>,
    aspect_ratio: Option<crate::timeline::AspectRatio>,
}

impl ProjectEffect {
    fn capture(effect: &EffectInstance, project_path: &Path) -> Self {
        Self {
            id: effect.id.get(),
            plugin_id: effect.plugin_id.clone(),
            effect_id: effect.effect_id.clone(),
            properties: capture_property_overrides(&effect.properties, project_path),
            animations: capture_animations(&effect.animations),
            aspect_ratio: effect.aspect_ratio,
        }
    }

    fn into_effect(
        self,
        project_path: &Path,
        plugins: &PluginRegistry,
        duration: FrameDuration,
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
        let properties =
            load_properties(schema.properties(), self.properties, "effect", project_path)?;
        let animations = load_animations(
            schema.properties(),
            &properties.materialize(schema.properties()),
            self.animations,
            duration,
        )?;
        Ok(EffectInstance {
            id: EffectInstanceId::new(self.id),
            plugin_id: self.plugin_id,
            effect_id: self.effect_id,
            properties,
            animations,
            aspect_ratio: self.aspect_ratio,
            schema,
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectScalarAnimation {
    path: PropertyPath,
    track: ScalarTrack,
}

pub(super) fn capture_animations(animations: &ScalarAnimations) -> Vec<ProjectScalarAnimation> {
    animations
        .tracks()
        .map(|(address, track)| ProjectScalarAnimation {
            path: address.clone(),
            track: track.clone(),
        })
        .collect()
}

pub(super) fn load_animations(
    schema: &[PropertySchema],
    properties: &PropertyValues,
    animations: Vec<ProjectScalarAnimation>,
    duration: FrameDuration,
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
        let path = animation.path;
        let track = animation.track;
        let property_id = path.property_id();
        let valid = properties
            .property(property_id)
            .and_then(|value| {
                property.resolve_scalar(value, path.element_id(), path.scalar_index())
            })
            .is_some_and(|resolved| {
                property.is_animatable(path.scalar_index())
                    && track.span() == track.repeat().pattern_span(duration)
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
    }
    Ok(loaded)
}

fn load_properties(
    schema: &[PropertySchema],
    values: BTreeMap<String, PropertyValue>,
    owner: &str,
    project_path: &Path,
) -> Result<PropertyOverrides, ProjectError> {
    let mut loaded = PropertyOverrides::default();
    for (id, mut value) in values {
        value.map_file_paths(&mut |path| resolve_path(path, project_path));
        let property = schema
            .iter()
            .find(|property| property.id == id)
            .ok_or_else(|| {
                ProjectError::invalid_data(format!("{owner} has unknown property '{id}'"))
            })?;
        loaded.set(property, value).map_err(|error| {
            ProjectError::invalid_data(format!("{owner} property '{id}' is invalid: {error}"))
        })?;
    }
    Ok(loaded)
}

fn validate_no_overlaps(
    items: &[(LayerId, TimelineItem)],
    frame_rate: FrameRate,
) -> Result<(), ProjectError> {
    let mut ranges = items
        .iter()
        .map(|(layer, item)| {
            (
                layer.get(),
                item.start().get(),
                item.end_exclusive(frame_rate).get(),
            )
        })
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
