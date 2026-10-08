use std::collections::{HashMap, HashSet};

use super::{
    ids::{ItemId, LayerId, SceneId},
    item::TimelineItem,
    properties::resolve_item,
    scene::SceneDefinition,
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

fn scene_runtime_seed(parent_seed: u64, item_id: ItemId, scene_id: SceneId) -> u64 {
    parent_seed
        .wrapping_mul(0x9E37_79B1_85EB_CA87)
        .wrapping_add(item_id.get())
        .wrapping_add(scene_id.get().rotate_left(23))
}

/// Visit every source item after applying scene instance arguments, without
/// visibility or time clipping. Scene instances themselves are included so
/// their effects remain available to consumers of project resources.
pub(crate) fn visit_source_items(
    items: Vec<(LayerId, TimelineItem)>,
    scenes: &HashMap<SceneId, SceneDefinition>,
    visit: &mut impl FnMut(&TimelineItem),
) {
    for (_, item) in items {
        visit(&item);
        if let Some(scene) = item.scene_id().and_then(|id| scenes.get(&id)) {
            let mut children = scene.document().source_items();
            let instance = resolve_item(
                scenes,
                None,
                &item,
                Some(TimelineTime::from_frame(item.start)),
            );
            scene.apply_arguments(
                scenes,
                Some(&instance),
                children.iter_mut().map(|(_, item)| item),
            );
            visit_source_items(children, scenes, visit);
        }
    }
}

/// Global clip retained on every evaluated render node. Scene clips are
/// intersected with all parent scene-instance clips.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvaluatedClip {
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

    pub fn end_exclusive(self) -> Frame {
        Frame::new(self.start.get().saturating_add(self.duration.get()))
    }
}

/// Hierarchical timeline evaluation consumed by the renderer graph adapter.
/// `path` contains stable source IDs from the outermost scene instance to this
/// node; runtime-remapped leaf IDs remain available on `item`.
#[derive(Clone, Debug)]
pub struct EvaluatedSceneNode {
    /// Layer in the composition that directly contains this node.
    pub local_layer: LayerId,
    /// Root timeline layer used by visibility and flattened media evaluation.
    pub layer: LayerId,
    pub clip: EvaluatedClip,
    pub path: Vec<ItemId>,
    pub item: TimelineItem,
    pub children: Vec<EvaluatedSceneNode>,
}

pub(crate) fn evaluated_scene_graph_at_time(
    items: Vec<(LayerId, TimelineItem)>,
    scenes: &HashMap<SceneId, SceneDefinition>,
    time: TimelineTime,
    visibility: Option<&PreviewVisibility>,
) -> Vec<EvaluatedSceneNode> {
    struct EvaluationContext<'a> {
        scenes: &'a HashMap<SceneId, SceneDefinition>,
        visibility: Option<&'a PreviewVisibility>,
        used_ids: HashSet<ItemId>,
    }

    #[derive(Default)]
    struct Placement<'a> {
        time_offset: u64,
        runtime_seed: u64,
        layer: Option<LayerId>,
        clip_end: Option<u64>,
        path: &'a [ItemId],
    }

    fn expand(
        context: &mut EvaluationContext<'_>,
        items: Vec<(LayerId, TimelineItem)>,
        time: TimelineTime,
        placement: Placement<'_>,
    ) -> Vec<EvaluatedSceneNode> {
        let mut output = Vec::new();
        for (layer, source) in items {
            if placement.path.is_empty()
                && context
                    .visibility
                    .is_some_and(|visibility| !visibility.is_item_visible(layer, source.id))
            {
                continue;
            }
            let source_id = source.id;
            let mut item = resolve_item(context.scenes, None, &source, Some(time));
            let output_layer = placement.layer.unwrap_or(layer);
            let global_start = placement.time_offset.saturating_add(item.start.get());
            let source_end = global_start.saturating_add(item.duration.get());
            let global_end = placement
                .clip_end
                .map_or(source_end, |end| end.min(source_end));
            let Some(clip) = EvaluatedClip::from_bounds(global_start, global_end) else {
                continue;
            };
            let mut path = placement.path.to_vec();
            path.push(source_id);
            let Some(scene_id) = item.scene_id() else {
                if let Some(visibility) = context.visibility {
                    visibility.retain_visible_effects(&mut item);
                }
                item.id = unique_runtime_id(placement.runtime_seed, item.id, &mut context.used_ids);
                item.start = clip.start;
                item.duration = clip.duration;
                output.push(EvaluatedSceneNode {
                    local_layer: layer,
                    layer: output_layer,
                    clip,
                    path,
                    item,
                    children: Vec::new(),
                });
                continue;
            };
            let Some(scene) = context.scenes.get(&scene_id) else {
                continue;
            };
            let local = TimelineTime::from_frames(time.frames() - item.start.get() as f64);
            // The instance keeps its own span when its source scene shrinks.
            // Outside the source, even effects on the instance are transparent.
            if local.frames() >= scene.duration().get() as f64 {
                continue;
            }
            let mut child_items = scene.document().active_source_items_at_time(local);
            scene.apply_arguments(
                context.scenes,
                Some(&item),
                child_items.iter_mut().map(|(_, item)| item),
            );
            let children = expand(
                context,
                child_items,
                local,
                Placement {
                    time_offset: global_start,
                    runtime_seed: scene_runtime_seed(placement.runtime_seed, item.id, scene_id),
                    layer: Some(output_layer),
                    clip_end: Some(global_end),
                    path: &path,
                },
            );
            item.start = clip.start;
            item.duration = clip.duration;
            if let Some(visibility) = context.visibility {
                visibility.retain_visible_effects(&mut item);
            }
            output.push(EvaluatedSceneNode {
                local_layer: layer,
                layer: output_layer,
                clip,
                path,
                item,
                children,
            });
        }
        output
    }

    let mut context = EvaluationContext {
        scenes,
        visibility,
        used_ids: HashSet::new(),
    };
    expand(&mut context, items, time, Placement::default())
}

pub(crate) fn evaluated_items_at_time(
    items: Vec<(LayerId, TimelineItem)>,
    scenes: &HashMap<SceneId, SceneDefinition>,
    time: TimelineTime,
    visibility: Option<&PreviewVisibility>,
) -> Vec<(LayerId, TimelineItem)> {
    fn flatten(nodes: Vec<EvaluatedSceneNode>, output: &mut Vec<(LayerId, TimelineItem)>) {
        for node in nodes {
            if node.item.scene_id().is_some() {
                flatten(node.children, output);
            } else {
                output.push((node.layer, node.item));
            }
        }
    }

    let graph = evaluated_scene_graph_at_time(items, scenes, time, visibility);
    let mut output = Vec::new();
    flatten(graph, &mut output);
    output
}

pub(crate) fn visible_items(
    items: Vec<(LayerId, TimelineItem)>,
    scenes: &HashMap<SceneId, SceneDefinition>,
    visibility: Option<&PreviewVisibility>,
) -> Vec<TimelineItem> {
    struct VisibilityContext<'a> {
        scenes: &'a HashMap<SceneId, SceneDefinition>,
        visibility: Option<&'a PreviewVisibility>,
        output: Vec<TimelineItem>,
        used_ids: HashSet<ItemId>,
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
                item.id = unique_runtime_id(runtime_seed, item.id, &mut context.used_ids);
                item.start = Frame::new(global_start);
                item.duration = FrameDuration::new_saturating(visible_end - global_start);
                context.output.push(item);
                continue;
            };
            let Some(scene) = context.scenes.get(&scene_id) else {
                continue;
            };
            let mut children = scene.document().source_items();
            let instance = resolve_item(
                context.scenes,
                None,
                &source,
                Some(TimelineTime::from_frame(source.start)),
            );
            scene.apply_arguments(
                context.scenes,
                Some(&instance),
                children.iter_mut().map(|(_, item)| item),
            );
            let child_seed = scene_runtime_seed(runtime_seed, source.id, scene_id);
            expand(
                context,
                children,
                global_start,
                child_seed,
                Some(visible_end),
            );
        }
    }

    let mut context = VisibilityContext {
        scenes,
        visibility,
        output: Vec::new(),
        used_ids: HashSet::new(),
    };
    expand(&mut context, items, 0, 0, None);
    context.output
}
