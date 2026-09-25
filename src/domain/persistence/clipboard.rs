//! Timeline clipboard encoding using the same checked item representation as project files.
use super::ProjectError;
use super::project::{ProjectItem, load_items, validate_no_overlaps};
use crate::domain::timeline::{
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
pub(crate) struct DecodedTimelineClipboard {
    pub source_scene: Option<SceneId>,
    pub items: Vec<(LayerId, TimelineItem)>,
    pub scene_bindings: Vec<(String, SceneBindingTarget)>,
}

pub(crate) fn encode_timeline_clipboard(
    items: &[(LayerId, TimelineItem)],
    source_scene: Option<SceneId>,
    scene_bindings: &[(String, SceneBindingTarget)],
) -> Result<String, ProjectError> {
    let file = TimelineClipboardFile {
        format: TIMELINE_CLIPBOARD_FORMAT.to_owned(),
        format_version: TIMELINE_CLIPBOARD_FORMAT_VERSION,
        source_scene: source_scene
            .map(|scene| [scene.project().high(), scene.project().low(), scene.get()]),
        items: items
            .iter()
            .map(|(layer, item)| ProjectItem::capture(item, *layer, Path::new("")))
            .collect(),
        scene_bindings: scene_bindings.to_vec(),
    };
    serde_json::to_string(&file).map_err(|error| {
        ProjectError::encode(format!("コピー内容を変換できません: {error}"), error)
    })
}

pub(crate) fn decode_timeline_clipboard(
    source: &str,
    editor: &TimelineEditor,
) -> Result<DecodedTimelineClipboard, ProjectError> {
    let file = serde_json::from_str::<TimelineClipboardFile>(source).map_err(|error| {
        ProjectError::invalid_format(format!("コピー内容の形式が不正です: {error}"), error)
    })?;
    if file.format != TIMELINE_CLIPBOARD_FORMAT
        || file.format_version != TIMELINE_CLIPBOARD_FORMAT_VERSION
    {
        return Err(ProjectError::unsupported_format("未対応のコピー形式です"));
    }
    let source_scene = file
        .source_scene
        .map(|[high, low, scene]| {
            let project = ProjectId::from_parts(high, low)
                .ok_or_else(|| ProjectError::invalid_data("プロジェクトIDが不正です"))?;
            if scene == 0 || scene == u64::MAX {
                return Err(ProjectError::invalid_data("シーンIDが不正です"));
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
    validate_no_overlaps(&items)?;
    let item_ids = items
        .iter()
        .map(|(_, item)| item.id)
        .collect::<HashSet<_>>();
    let mut scene_bindings = Vec::with_capacity(file.scene_bindings.len());
    for (argument_id, binding) in file.scene_bindings {
        if !item_ids.contains(&binding.item_id()) {
            return Err(ProjectError::invalid_data(
                "コピーされたシーン引数接続の対象がありません",
            ));
        }
        scene_bindings.push((argument_id, binding));
    }
    Ok(DecodedTimelineClipboard {
        source_scene,
        items,
        scene_bindings,
    })
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TimelineClipboardFile {
    format: String,
    format_version: u32,
    source_scene: Option<[u64; 3]>,
    items: Vec<ProjectItem>,
    scene_bindings: Vec<(String, SceneBindingTarget)>,
}
