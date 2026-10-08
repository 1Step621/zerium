//! Timeline clipboard encoding using the same checked item representation as project files.
use super::{
    ProjectError,
    items::{ProjectItem, load_items},
    paths::item_files,
};
use crate::timeline::{
    LayerId, ProjectId, SceneBindingTarget, SceneId, TimelineEditor, TimelineItem,
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
        .flat_map(|scene| &scene.arguments)
        .flat_map(|argument| {
            argument
                .bindings
                .iter()
                .filter(|binding| ids.contains(&binding.item_id()))
                .map(|binding| (argument.schema.id().to_owned(), binding.clone()))
        })
        .collect::<Vec<_>>();
    // A clipboard copy carries usable values even when pasted outside the
    // source scene. Unbound scene instance inputs retain their inheritance.
    let mut items = items.to_vec();
    editor.resolve_active_scene_arguments(items.iter_mut().map(|(_, item)| item));
    let file = TimelineClipboardFile {
        format: TIMELINE_CLIPBOARD_FORMAT.to_owned(),
        format_version: TIMELINE_CLIPBOARD_FORMAT_VERSION,
        source_scene: source_scene
            .map(|scene| [scene.project().high(), scene.project().low(), scene.get()]),
        items: items
            .iter()
            .map(|(layer, item)| ProjectItem::capture(item, *layer, Path::new("")))
            .collect(),
        scene_bindings,
        media_cache: editor
            .media_cache()
            .retained_paths(items.iter().flat_map(|(_, item)| item_files(item))),
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
    let file = serde_json::from_str::<TimelineClipboardFile>(source).map_err(|error| {
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
                    .map(|argument| argument.schema.clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
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
    Ok(DecodedTimelineClipboard {
        media_cache: file.media_cache,
        source_scene,
        items,
        scene_bindings: file.scene_bindings,
    })
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TimelineClipboardFile {
    #[serde(default)]
    media_cache: crate::media::MediaMetadataCache,
    format: String,
    format_version: u32,
    source_scene: Option<[u64; 3]>,
    items: Vec<ProjectItem>,
    scene_bindings: Vec<(String, SceneBindingTarget)>,
}
