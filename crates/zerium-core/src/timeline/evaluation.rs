use std::collections::{HashMap, HashSet};

use super::{
    ids::{ItemId, LayerId, SceneId},
    item::TimelineItem,
    properties::{SceneBindings, resolve_render_item},
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

pub(super) fn unique_runtime_id(seed: u64, item_id: ItemId, used: &mut HashSet<ItemId>) -> ItemId {
    let mut salt = 0_u64;
    loop {
        let candidate = mixed_runtime_id(seed.wrapping_add(salt), item_id);
        if used.insert(candidate) {
            return candidate;
        }
        salt = salt.wrapping_add(0x9E37_79B1_85EB_CA87);
    }
}

pub(super) fn scene_runtime_seed(parent_seed: u64, item_id: ItemId, scene_id: SceneId) -> u64 {
    parent_seed
        .wrapping_mul(0x9E37_79B1_85EB_CA87)
        .wrapping_add(item_id.get())
        .wrapping_add(scene_id.get().rotate_left(23))
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

/// A temporal consumer requests the image before a particular effect. Limits
/// refer to source paths and count effects after preview visibility is applied.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SceneRenderStage {
    pub path: Vec<ItemId>,
    pub effect_count: usize,
}

/// Hierarchical timeline evaluation consumed by the renderer graph adapter.
/// `path` contains stable source IDs from the outermost scene instance to this
/// node; placement-specific runtime IDs remain available on `item`, including
/// scene instances that own effect inputs.
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
    pub calculation_errors: Vec<String>,
}

impl EvaluatedSceneNode {
    pub fn calculation_error(&self) -> Option<&str> {
        self.calculation_errors
            .first()
            .map(String::as_str)
            .or_else(|| self.children.iter().find_map(Self::calculation_error))
    }
}

pub(crate) fn evaluated_scene_graph_at_time(
    items: Vec<(LayerId, TimelineItem)>,
    scenes: &HashMap<SceneId, SceneDefinition>,
    time: TimelineTime,
    visibility: Option<&PreviewVisibility>,
    scene: Option<&SceneDefinition>,
    stages: &[SceneRenderStage],
) -> Vec<EvaluatedSceneNode> {
    struct EvaluationContext<'a> {
        scenes: &'a HashMap<SceneId, SceneDefinition>,
        visibility: Option<&'a PreviewVisibility>,
        stages: &'a [SceneRenderStage],
        used_ids: HashSet<ItemId>,
    }

    #[derive(Default)]
    struct Placement<'a> {
        time_offset: u64,
        runtime_seed: u64,
        layer: Option<LayerId>,
        clip_end: Option<u64>,
        path: &'a [ItemId],
        bindings: Option<&'a SceneBindings<'a>>,
    }

    fn expand(
        context: &mut EvaluationContext<'_>,
        items: Vec<(LayerId, TimelineItem)>,
        time: TimelineTime,
        placement: Placement<'_>,
    ) -> Vec<EvaluatedSceneNode> {
        let mut output = Vec::new();
        for (layer, mut source) in items {
            if placement.path.is_empty()
                && context
                    .visibility
                    .is_some_and(|visibility| !visibility.is_item_visible(layer, source.id))
            {
                continue;
            }
            let source_id = source.id;
            let mut path = placement.path.to_vec();
            path.push(source_id);
            if let Some(visibility) = context.visibility {
                visibility.retain_visible_effects(&mut source);
            }
            for stage in context.stages.iter().filter(|stage| stage.path == path) {
                source.effects.truncate(stage.effect_count);
            }
            let resolved = resolve_render_item(context.scenes, placement.bindings, &source, time);
            let mut item = resolved.value;
            let calculation_errors = resolved.diagnostics;
            let output_layer = placement.layer.unwrap_or(layer);
            let global_start = placement.time_offset.saturating_add(item.start.get());
            let source_end = global_start.saturating_add(item.duration.get());
            let global_end = placement
                .clip_end
                .map_or(source_end, |end| end.min(source_end));
            let Some(clip) = EvaluatedClip::from_bounds(global_start, global_end) else {
                continue;
            };
            item.id = unique_runtime_id(placement.runtime_seed, source_id, &mut context.used_ids);
            let Some(scene_id) = item.scene_id() else {
                item.start = clip.start;
                item.duration = clip.duration;
                output.push(EvaluatedSceneNode {
                    local_layer: layer,
                    layer: output_layer,
                    clip,
                    path,
                    item,
                    children: Vec::new(),
                    calculation_errors,
                });
                continue;
            };
            let Some(scene) = context.scenes.get(&scene_id) else {
                continue;
            };
            let local = TimelineTime::from_frames(time.frames() - item.start.get() as f64);
            if local.frames() < 0. || local.frames() >= item.duration.get() as f64 {
                continue;
            }
            let child_items = scene.expanded_items(&item, Some(local));
            let bindings = SceneBindings::for_instance(
                context.scenes,
                scene,
                &source,
                Some(time),
                placement.bindings,
            );
            let children = expand(
                context,
                child_items,
                local,
                Placement {
                    time_offset: global_start,
                    runtime_seed: scene_runtime_seed(placement.runtime_seed, source_id, scene_id),
                    layer: Some(output_layer),
                    clip_end: Some(global_end),
                    path: &path,
                    bindings: Some(&bindings),
                },
            );
            item.start = clip.start;
            item.duration = clip.duration;
            output.push(EvaluatedSceneNode {
                local_layer: layer,
                layer: output_layer,
                clip,
                path,
                item,
                children,
                calculation_errors,
            });
        }
        output
    }

    let mut context = EvaluationContext {
        scenes,
        visibility,
        stages,
        used_ids: HashSet::new(),
    };
    let bindings = scene.map(|scene| SceneBindings::new(scenes, scene, None));
    expand(
        &mut context,
        items,
        time,
        Placement {
            bindings: bindings.as_ref(),
            ..Default::default()
        },
    )
}

pub(crate) fn evaluated_items_at_time(
    items: Vec<(LayerId, TimelineItem)>,
    scenes: &HashMap<SceneId, SceneDefinition>,
    time: TimelineTime,
    visibility: Option<&PreviewVisibility>,
    scene: Option<&SceneDefinition>,
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

    let graph = evaluated_scene_graph_at_time(items, scenes, time, visibility, scene, &[]);
    let mut output = Vec::new();
    flatten(graph, &mut output);
    output
}
