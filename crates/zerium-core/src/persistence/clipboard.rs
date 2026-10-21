//! Timeline clipboard encoding using the same checked item representation as project files.
use super::{
    ProjectError,
    items::{ProjectItem, load_items},
    paths::item_files,
};
use crate::property::PropertyValue;
use crate::timeline::{
    EffectInstanceId, ItemId, LayerId, ProjectId, SceneBindingOwner, SceneBindingTarget, SceneId,
    TimelineEditor, TimelineItem,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

const TIMELINE_CLIPBOARD_FORMAT: &str = "zerium/timeline-items";
const TIMELINE_CLIPBOARD_FORMAT_VERSION: u32 = 1;

#[derive(Debug)]
pub struct DecodedTimelineClipboard {
    pub media_cache: crate::media::MediaMetadataCache,
    pub source_scene: Option<SceneId>,
    pub items: Vec<(LayerId, TimelineItem)>,
    pub scene_bindings: Vec<(String, SceneBindingTarget)>,
    pub hidden_effects: Vec<EffectInstanceId>,
}

pub fn encode_timeline_clipboard(
    items: &[(LayerId, TimelineItem)],
    editor: &TimelineEditor,
) -> Result<String, ProjectError> {
    let source_scene = editor.active_scene_id();
    let scene = source_scene.and_then(|id| editor.scene(id));
    let ids = items
        .iter()
        .map(|(_, item)| item.id)
        .collect::<HashSet<_>>();
    let scene_bindings = scene
        .into_iter()
        .flat_map(|scene| scene.variables())
        .flat_map(|argument| {
            argument
                .bindings
                .iter()
                .filter(|binding| ids.contains(&binding.item_id()))
                .map(|binding| (argument.id().to_owned(), binding.clone()))
        })
        .collect::<Vec<_>>();
    // Preserve authored fallbacks for copies that retain their connections.
    // Detached copies use the connected properties' values at copy time.
    let mut resolved = items.to_vec();
    editor.resolve_active_scene_variables(resolved.iter_mut().map(|(_, item)| item));
    let mut properties = HashSet::new();
    let resolved_properties = scene_bindings
        .iter()
        .filter_map(|(_, binding)| {
            let key = (
                binding.item_id(),
                binding.owner(),
                binding.property_id().to_owned(),
            );
            if !properties.insert(key.clone()) {
                return None;
            }
            let (_, item) = resolved
                .iter()
                .find(|(_, item)| item.id == binding.item_id())?;
            let value = item
                .property_values(binding.owner().effect_id())?
                .property(binding.property_id())?
                .clone();
            Some(ClipboardProperty {
                item_id: key.0,
                owner: key.1,
                property_id: key.2,
                value,
            })
        })
        .collect();
    let file = TimelineClipboardFile {
        hidden_effects: items
            .iter()
            .flat_map(|(_, item)| &item.effects)
            .filter(|effect| editor.is_effect_hidden(effect.id))
            .map(|effect| effect.id)
            .collect(),
        format: TIMELINE_CLIPBOARD_FORMAT.to_owned(),
        format_version: TIMELINE_CLIPBOARD_FORMAT_VERSION,
        source_scene: source_scene
            .map(|scene| [scene.project().high(), scene.project().low(), scene.get()]),
        items: items
            .iter()
            .map(|(layer, item)| ProjectItem::capture(item, *layer, Path::new("")))
            .collect(),
        scene_bindings,
        resolved_properties,
        media_cache: editor.media_cache().retained_paths(
            items
                .iter()
                .chain(&resolved)
                .flat_map(|(_, item)| item_files(item)),
        ),
    };
    serde_json::to_string(&file).map_err(|error| {
        ProjectError::encode(
            format!("Failed to serialize clipboard data: {error}"),
            error,
        )
    })
}

pub fn decode_timeline_clipboard(
    source: &str,
    editor: &TimelineEditor,
) -> Result<DecodedTimelineClipboard, ProjectError> {
    let mut file = serde_json::from_str::<TimelineClipboardFile>(source).map_err(|error| {
        ProjectError::invalid_format(format!("Invalid clipboard data format: {error}"), error)
    })?;
    if file.format != TIMELINE_CLIPBOARD_FORMAT
        || file.format_version != TIMELINE_CLIPBOARD_FORMAT_VERSION
    {
        return Err(ProjectError::unsupported_format(
            "Unsupported clipboard format",
        ));
    }
    let source_scene = file
        .source_scene
        .map(|[high, low, scene]| {
            let project = ProjectId::from_parts(high, low)
                .ok_or_else(|| ProjectError::invalid_data("Invalid project ID"))?;
            if scene == 0 || scene == u64::MAX {
                return Err(ProjectError::invalid_data("Invalid scene ID"));
            }
            Ok(SceneId::new(project, scene))
        })
        .transpose()?;
    let scene_schemas = editor
        .scenes()
        .map(|scene| {
            (
                scene.id,
                scene
                    .arguments()
                    .filter_map(|argument| argument.argument_schema().cloned())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    let preserve_bindings = source_scene.is_some()
        && source_scene == editor.active_scene_id()
        && source_scene
            .and_then(|id| editor.scene(id))
            .is_some_and(|scene| {
                file.scene_bindings
                    .iter()
                    .all(|(id, _)| scene.variable(id).is_some())
            });
    if !preserve_bindings {
        for property in &file.resolved_properties {
            if !file.items.iter_mut().any(|item| {
                item.apply_clipboard_property(
                    property.item_id,
                    property.owner,
                    &property.property_id,
                    &property.value,
                )
            }) {
                return Err(ProjectError::invalid_data(
                    "Copied property target was not found",
                ));
            }
        }
    }
    let mut effect_ids = HashSet::new();
    let items = load_items(
        file.items,
        Path::new(""),
        &scene_schemas,
        &mut effect_ids,
        editor.plugin_registry(),
    )?;
    let item_ids = items
        .iter()
        .map(|(_, item)| item.id)
        .collect::<HashSet<_>>();
    for (_, binding) in &file.scene_bindings {
        if !item_ids.contains(&binding.item_id()) {
            return Err(ProjectError::invalid_data(
                "Copied scene argument binding target was not found",
            ));
        }
    }
    if file
        .hidden_effects
        .iter()
        .any(|id| !effect_ids.contains(&id.get()))
    {
        return Err(ProjectError::invalid_data("Invalid clipboard visibility"));
    }
    Ok(DecodedTimelineClipboard {
        media_cache: file.media_cache,
        source_scene: preserve_bindings.then_some(source_scene).flatten(),
        items,
        scene_bindings: if preserve_bindings {
            file.scene_bindings
        } else {
            Vec::new()
        },
        hidden_effects: file.hidden_effects,
    })
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TimelineClipboardFile {
    hidden_effects: Vec<EffectInstanceId>,
    media_cache: crate::media::MediaMetadataCache,
    format: String,
    format_version: u32,
    source_scene: Option<[u64; 3]>,
    items: Vec<ProjectItem>,
    scene_bindings: Vec<(String, SceneBindingTarget)>,
    resolved_properties: Vec<ClipboardProperty>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClipboardProperty {
    item_id: ItemId,
    owner: SceneBindingOwner,
    property_id: String,
    value: PropertyValue,
}
