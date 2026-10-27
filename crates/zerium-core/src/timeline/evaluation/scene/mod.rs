//! Scene clocks, bindings, traversal, and evaluated scene occurrences.
use self::properties::{SceneBindings, resolve_render_item};
use crate::timeline::model::{
    ItemId, LayerId, SceneId,
    scene::SceneDefinition,
    time::{Frame, FrameDuration, TimelineTime},
};
use std::collections::{HashMap, HashSet};

pub(in crate::timeline) mod item;
pub(in crate::timeline) mod placement;
pub(in crate::timeline) mod properties;

fn mixed_runtime_id(seed: u64, item_id: ItemId) -> ItemId {
    if seed == 0 {
        return item_id;
    }
    let mixed = seed.wrapping_mul(0x9E37_79B1_85EB_CA87).rotate_left(17)
        ^ item_id.get().wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    ItemId(mixed.max(1))
}

pub(in crate::timeline) fn unique_runtime_id(
    seed: u64,
    item_id: ItemId,
    used: &mut HashSet<ItemId>,
) -> ItemId {
    let mut salt = 0_u64;
    loop {
        let candidate = mixed_runtime_id(seed.wrapping_add(salt), item_id);
        if used.insert(candidate) {
            return candidate;
        }
        salt = salt.wrapping_add(0x9E37_79B1_85EB_CA87);
    }
}

pub(in crate::timeline) fn scene_runtime_seed(
    parent_seed: u64,
    item_id: ItemId,
    scene_id: SceneId,
) -> u64 {
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
    pub(in crate::timeline) fn from_bounds(start: u64, end: u64) -> Option<Self> {
        (end > start).then(|| Self {
            start: Frame::new(start),
            duration: FrameDuration::new_saturating(end - start),
        })
    }

    pub fn end_exclusive(self) -> Frame {
        Frame::new(self.start.get().saturating_add(self.duration.get()))
    }
}

/// Hierarchical timeline read model used to flatten evaluated editor items.
/// `path` contains stable source IDs from the outermost scene instance to this
/// node; placement-specific runtime IDs remain available on `item`, including
/// scene instances that own effect inputs.
#[derive(Clone, Debug)]
pub struct EvaluatedSceneNode {
    /// Layer in the composition that directly contains this node.
    pub local_layer: LayerId,
    /// Root timeline layer used by visibility and flattened media evaluation.
    pub layer: LayerId,
    pub path: Vec<ItemId>,
    pub item: crate::timeline::EvaluatedItem,
    pub children: Vec<EvaluatedSceneNode>,
    pub calculation_errors: Vec<crate::timeline::CalculationDiagnostic>,
}

fn evaluated_scene_graph_at_time(
    scene: &SceneDefinition,
    scenes: &HashMap<SceneId, SceneDefinition>,
    time: TimelineTime,
) -> Vec<EvaluatedSceneNode> {
    use crate::timeline::evaluation::scene::placement::{
        PlacedItem, SceneVisitor, SceneWalker, WalkMode,
    };

    let frame_rate = scene.document().frame_rate();

    struct RenderVisitor {
        used_ids: HashSet<ItemId>,
        frame_rate: crate::timeline::FrameRate,
    }

    impl SceneVisitor for RenderVisitor {
        type State = crate::timeline::evaluation::scene::properties::ResolvedItem;
        type Output = EvaluatedSceneNode;

        fn enter(
            &mut self,
            placed: &PlacedItem,
            time: Option<TimelineTime>,
            bindings: &SceneBindings<'_>,
        ) -> Self::State {
            let mut resolved = resolve_render_item(
                bindings,
                &placed.source,
                time.expect("render sample"),
                self.frame_rate,
            );
            resolved.value.runtime_id =
                unique_runtime_id(placed.seed, placed.source.id, &mut self.used_ids);
            resolved.value.placement = placed.clip;
            resolved
        }

        fn exit(
            &mut self,
            placed: PlacedItem,
            resolved: Self::State,
            children: Vec<Self::Output>,
        ) -> Option<Self::Output> {
            Some(EvaluatedSceneNode {
                local_layer: placed.local_layer,
                layer: placed.layer,
                path: placed.path,
                item: resolved.value,
                children,
                calculation_errors: resolved.diagnostics,
            })
        }
    }

    let bindings = SceneBindings::new(scenes, scene, None);
    SceneWalker { scenes, frame_rate }.walk(
        scene,
        WalkMode::Sample(time),
        &bindings,
        &mut RenderVisitor {
            used_ids: HashSet::new(),
            frame_rate,
        },
    )
}

pub(crate) fn evaluated_items_at_time(
    scene: &SceneDefinition,
    scenes: &HashMap<SceneId, SceneDefinition>,
    time: TimelineTime,
) -> Vec<(LayerId, crate::timeline::EvaluatedItem)> {
    fn flatten(
        nodes: Vec<EvaluatedSceneNode>,
        output: &mut Vec<(LayerId, crate::timeline::EvaluatedItem)>,
    ) {
        for node in nodes {
            if node.item.scene_id().is_some() {
                flatten(node.children, output);
            } else {
                output.push((node.layer, node.item));
            }
        }
    }

    let graph = evaluated_scene_graph_at_time(scene, scenes, time);
    let mut output = Vec::new();
    flatten(graph, &mut output);
    output
}
