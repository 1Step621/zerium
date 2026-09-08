use std::collections::{HashMap, HashSet};

use crate::domain::parameter::ParameterValue;

use super::{
    document::TimelineDocument,
    ids::{ItemId, LayerId, SceneId},
    item::TimelineItem,
    scene::{SceneDefinition, apply_scene_binding_to_item, resolve_scene_binding},
    time::{Frame, FrameDuration, TimelineTime},
    visibility::PreviewVisibility,
};

fn mixed_runtime_id(seed: u64, item_id: ItemId) -> ItemId {
    if seed == 0 {
        return item_id;
    }
    let mixed = seed.wrapping_mul(0x9E37_79B1_85EB_CA87).rotate_left(17)
        ^ item_id.get().wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    ItemId(mixed.max(1))
}

fn unique_runtime_id(seed: u64, item_id: ItemId, used: &mut HashSet<ItemId>) -> ItemId {
    let mut salt = 0_u64;
    loop {
        let candidate = mixed_runtime_id(seed.wrapping_add(salt), item_id);
        if used.insert(candidate) {
            return candidate;
        }
        salt = salt.wrapping_add(0x9E37_79B1_85EB_CA87);
    }
}

pub(super) fn resolve_derived_argument_values(
    scene: &SceneDefinition,
    numeric_values: &mut HashMap<String, f32>,
    argument_values: &mut HashMap<String, ParameterValue>,
) {
    let mut pending = scene
        .arguments
        .iter()
        .filter(|argument| argument.is_derived())
        .collect::<Vec<_>>();
    while !pending.is_empty() {
        let previous_len = pending.len();
        pending.retain(|argument| {
            let Some(value) = argument.evaluate_expression(numeric_values) else {
                return true;
            };
            numeric_values.insert(argument.schema.id().to_owned(), value);
            argument_values.insert(argument.schema.id().to_owned(), ParameterValue::F32(value));
            false
        });
        if pending.len() == previous_len {
            break;
        }
    }
}

fn apply_scene_arguments(
    scene: &SceneDefinition,
    scenes: &HashMap<SceneId, SceneDefinition>,
    instance: &TimelineItem,
    time: TimelineTime,
    items: &mut [(LayerId, TimelineItem)],
) {
    let item_indexes = items
        .iter()
        .enumerate()
        .map(|(index, (_, item))| (item.id, index))
        .collect::<HashMap<_, _>>();
    let aspect_ratios = items
        .iter()
        .filter_map(|(_, item)| Some((item.id, item.current_aspect_ratio(item.schema()?)?)))
        .collect::<HashMap<_, _>>();
    let progress = instance.animation_progress_at_time(time);
    let schemas = scene
        .arguments
        .iter()
        .filter(|argument| !argument.is_derived())
        .map(|argument| argument.schema.parameter().clone())
        .collect::<Vec<_>>();
    let values = instance
        .animations
        .evaluated_values(&instance.parameters, &schemas, progress);
    let mut argument_values = HashMap::new();
    let mut numeric_values = HashMap::new();
    for argument in scene
        .arguments
        .iter()
        .filter(|argument| !argument.is_derived())
    {
        let Some(value) = values.get(argument.schema.id()).cloned() else {
            continue;
        };
        if let ParameterValue::F32(value) = value {
            numeric_values.insert(argument.schema.id().to_owned(), value);
            argument_values.insert(argument.schema.id().to_owned(), ParameterValue::F32(value));
        } else {
            argument_values.insert(argument.schema.id().to_owned(), value);
        }
    }
    resolve_derived_argument_values(scene, &mut numeric_values, &mut argument_values);
    for argument in &scene.arguments {
        for binding in &argument.bindings {
            let Some(value) = argument_values
                .get(argument.schema.id())
                .and_then(|value| argument.schema.constrained_value(value))
            else {
                continue;
            };
            let Some(item) = item_indexes
                .get(&binding.item_id())
                .and_then(|index| items.get_mut(*index))
                .map(|(_, item)| item)
            else {
                continue;
            };
            let Some(resolved) = resolve_scene_binding(scenes, scene, binding) else {
                continue;
            };
            apply_scene_binding_to_item(item, binding, &resolved.schema, value)
                .expect("validated scene binding must remain applicable during evaluation");
        }
    }
    for (_, item) in items {
        if item.preserves_aspect_ratio()
            && let Some(aspect_ratio) = aspect_ratios.get(&item.id)
        {
            item.constrain_size_to_aspect_ratio(*aspect_ratio);
        }
    }
}

/// Global clip retained on every evaluated render node. Scene clips are
/// intersected with all parent scene-instance clips.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EvaluatedClip {
    pub start: Frame,
    pub duration: FrameDuration,
}

impl EvaluatedClip {
    fn from_bounds(start: u64, end: u64) -> Option<Self> {
        (end > start).then(|| Self {
            start: Frame::new(start),
            duration: FrameDuration::new_saturating(end - start),
        })
    }

    pub(crate) fn end_exclusive(self) -> Frame {
        Frame::new(self.start.get().saturating_add(self.duration.get()))
    }
}

/// Hierarchical timeline evaluation consumed by the renderer graph adapter.
/// `path` contains stable source IDs from the outermost scene instance to this
/// node; runtime-remapped leaf IDs remain available on `item`.
#[derive(Clone, Debug)]
pub(crate) struct EvaluatedSceneNode {
    pub layer: LayerId,
    pub clip: EvaluatedClip,
    pub path: Vec<ItemId>,
    pub kind: EvaluatedSceneNodeKind,
}

#[derive(Clone, Debug)]
pub(crate) enum EvaluatedSceneNodeKind {
    Item(TimelineItem),
    Scene {
        scene_id: SceneId,
        instance: TimelineItem,
        children: Vec<EvaluatedSceneNode>,
    },
}

impl EvaluatedSceneNode {
    pub(crate) fn children(&self) -> &[Self] {
        match &self.kind {
            EvaluatedSceneNodeKind::Item(_) => &[],
            EvaluatedSceneNodeKind::Scene { children, .. } => children,
        }
    }

    pub(crate) fn item(&self) -> &TimelineItem {
        match &self.kind {
            EvaluatedSceneNodeKind::Item(item) => item,
            EvaluatedSceneNodeKind::Scene { instance, .. } => instance,
        }
    }
}

fn evaluated_document_graph_with_visibility(
    document: &TimelineDocument,
    scenes: &HashMap<SceneId, SceneDefinition>,
    time: TimelineTime,
    visibility: Option<&PreviewVisibility>,
) -> Vec<EvaluatedSceneNode> {
    struct EvaluationContext<'a> {
        scenes: &'a HashMap<SceneId, SceneDefinition>,
        visibility: Option<&'a PreviewVisibility>,
        used_ids: &'a mut HashSet<ItemId>,
    }

    #[allow(clippy::too_many_arguments)]
    fn expand(
        context: &mut EvaluationContext<'_>,
        items: Vec<(LayerId, TimelineItem)>,
        time: TimelineTime,
        time_offset: u64,
        runtime_seed: u64,
        parent_layer: Option<LayerId>,
        parent_clip_end: Option<u64>,
        parent_path: &[ItemId],
        apply_visibility: bool,
    ) -> Vec<EvaluatedSceneNode> {
        let mut output = Vec::new();
        for (layer, source) in items {
            if apply_visibility
                && context
                    .visibility
                    .is_some_and(|visibility| !visibility.is_item_visible(layer, source.id))
            {
                continue;
            }
            let source_id = source.id;
            let mut item = source.evaluated_at_time(time);
            let output_layer = parent_layer.unwrap_or(layer);
            let global_start = time_offset.saturating_add(item.start.get());
            let source_end = global_start.saturating_add(item.duration.get());
            let global_end = parent_clip_end.map_or(source_end, |end| end.min(source_end));
            let Some(clip) = EvaluatedClip::from_bounds(global_start, global_end) else {
                continue;
            };
            let mut path = parent_path.to_vec();
            path.push(source_id);
            let Some(scene_id) = item.scene_id() else {
                if let Some(visibility) = context.visibility {
                    visibility.retain_visible_effects(&mut item);
                }
                item.id = unique_runtime_id(runtime_seed, item.id, context.used_ids);
                item.start = clip.start;
                item.duration = clip.duration;
                output.push(EvaluatedSceneNode {
                    layer: output_layer,
                    clip,
                    path,
                    kind: EvaluatedSceneNodeKind::Item(item),
                });
                continue;
            };
            let Some(scene) = context.scenes.get(&scene_id) else {
                continue;
            };
            let local = TimelineTime::from_frames(time.frames() - item.start.get() as f64);
            let mut child_items = scene.document().active_source_items_at_time(local);
            apply_scene_arguments(scene, context.scenes, &item, time, &mut child_items);
            let child_seed = runtime_seed
                .wrapping_mul(0x9E37_79B1_85EB_CA87)
                .wrapping_add(item.id.get())
                .wrapping_add(scene_id.get().rotate_left(23));
            let children = expand(
                context,
                child_items,
                local,
                global_start,
                child_seed,
                Some(output_layer),
                Some(global_end),
                &path,
                false,
            );
            item.start = clip.start;
            item.duration = clip.duration;
            if let Some(visibility) = context.visibility {
                visibility.retain_visible_effects(&mut item);
            }
            output.push(EvaluatedSceneNode {
                layer: output_layer,
                clip,
                path,
                kind: EvaluatedSceneNodeKind::Scene {
                    scene_id,
                    instance: item,
                    children,
                },
            });
        }
        output
    }

    let mut used_ids = HashSet::new();
    let mut context = EvaluationContext {
        scenes,
        visibility,
        used_ids: &mut used_ids,
    };
    expand(
        &mut context,
        document.active_source_items_at_time(time),
        time,
        0,
        0,
        None,
        None,
        &[],
        true,
    )
}

pub(crate) fn evaluated_visible_document_graph_at_time(
    document: &TimelineDocument,
    scenes: &HashMap<SceneId, SceneDefinition>,
    visibility: &PreviewVisibility,
    time: TimelineTime,
) -> Vec<EvaluatedSceneNode> {
    evaluated_document_graph_with_visibility(document, scenes, time, Some(visibility))
}

pub(crate) fn evaluated_document_graph_at_time(
    document: &TimelineDocument,
    scenes: &HashMap<SceneId, SceneDefinition>,
    time: TimelineTime,
) -> Vec<EvaluatedSceneNode> {
    evaluated_document_graph_with_visibility(document, scenes, time, None)
}

fn evaluated_document_items_with_visibility(
    document: &TimelineDocument,
    scenes: &HashMap<SceneId, SceneDefinition>,
    time: TimelineTime,
    visibility: Option<&PreviewVisibility>,
) -> Vec<(LayerId, TimelineItem)> {
    fn flatten(nodes: Vec<EvaluatedSceneNode>, output: &mut Vec<(LayerId, TimelineItem)>) {
        for node in nodes {
            match node.kind {
                EvaluatedSceneNodeKind::Item(item) => output.push((node.layer, item)),
                EvaluatedSceneNodeKind::Scene { children, .. } => flatten(children, output),
            }
        }
    }

    let graph = evaluated_document_graph_with_visibility(document, scenes, time, visibility);
    let mut output = Vec::new();
    flatten(graph, &mut output);
    output
}

pub(crate) fn evaluated_visible_document_items_at_time(
    document: &TimelineDocument,
    scenes: &HashMap<SceneId, SceneDefinition>,
    visibility: &PreviewVisibility,
    time: TimelineTime,
) -> Vec<(LayerId, TimelineItem)> {
    evaluated_document_items_with_visibility(document, scenes, time, Some(visibility))
}

pub(crate) fn evaluated_document_items_at_time(
    document: &TimelineDocument,
    scenes: &HashMap<SceneId, SceneDefinition>,
    time: TimelineTime,
) -> Vec<(LayerId, TimelineItem)> {
    evaluated_document_items_with_visibility(document, scenes, time, None)
}

fn visible_document_items_with_visibility(
    document: &TimelineDocument,
    scenes: &HashMap<SceneId, SceneDefinition>,
    visibility: Option<&PreviewVisibility>,
) -> Vec<TimelineItem> {
    struct VisibilityContext<'a> {
        scenes: &'a HashMap<SceneId, SceneDefinition>,
        visibility: Option<&'a PreviewVisibility>,
        output: &'a mut Vec<TimelineItem>,
        used_ids: &'a mut HashSet<ItemId>,
    }

    fn expand(
        context: &mut VisibilityContext<'_>,
        items: Vec<(LayerId, TimelineItem)>,
        time_offset: u64,
        runtime_seed: u64,
        clip_end: Option<u64>,
    ) {
        for (layer, source) in items {
            if context
                .visibility
                .is_some_and(|visibility| !visibility.is_item_visible(layer, source.id))
            {
                continue;
            }
            let global_start = time_offset.saturating_add(source.start.get());
            let source_end = global_start.saturating_add(source.duration.get());
            let visible_end = clip_end.map_or(source_end, |end| end.min(source_end));
            if visible_end <= global_start {
                continue;
            }
            let Some(scene_id) = source.scene_id() else {
                let mut item = source;
                item.id = unique_runtime_id(runtime_seed, item.id, context.used_ids);
                item.start = Frame::new(global_start);
                item.duration = FrameDuration::new_saturating(visible_end - global_start);
                context.output.push(item);
                continue;
            };
            let Some(scene) = context.scenes.get(&scene_id) else {
                continue;
            };
            let mut children = scene.document().source_items();
            apply_scene_arguments(
                scene,
                context.scenes,
                &source,
                TimelineTime::from_frame(source.start),
                &mut children,
            );
            let child_seed = runtime_seed
                .wrapping_mul(0x9E37_79B1_85EB_CA87)
                .wrapping_add(source.id.get())
                .wrapping_add(scene_id.get().rotate_left(23));
            expand(
                context,
                children,
                global_start,
                child_seed,
                Some(visible_end),
            );
        }
    }

    let mut output = Vec::new();
    let mut used_ids = HashSet::new();
    let mut context = VisibilityContext {
        scenes,
        visibility,
        output: &mut output,
        used_ids: &mut used_ids,
    };
    expand(&mut context, document.source_items(), 0, 0, None);
    output
}

pub(crate) fn visibility_filtered_document_items(
    document: &TimelineDocument,
    scenes: &HashMap<SceneId, SceneDefinition>,
    visibility: &PreviewVisibility,
) -> Vec<TimelineItem> {
    visible_document_items_with_visibility(document, scenes, Some(visibility))
}

pub(crate) fn document_items(
    document: &TimelineDocument,
    scenes: &HashMap<SceneId, SceneDefinition>,
) -> Vec<TimelineItem> {
    visible_document_items_with_visibility(document, scenes, None)
}
