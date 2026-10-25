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
    pub(super) fn from_bounds(start: u64, end: u64) -> Option<Self> {
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
    pub item: super::EvaluatedItem,
    pub children: Vec<EvaluatedSceneNode>,
    pub calculation_errors: Vec<super::CalculationDiagnostic>,
}

pub(crate) fn evaluated_scene_graph_at_time(
    items: Vec<(LayerId, TimelineItem)>,
    scenes: &HashMap<SceneId, SceneDefinition>,
    time: TimelineTime,
    visibility: Option<&PreviewVisibility>,
    scene: Option<&SceneDefinition>,
    frame_rate: super::FrameRate,
) -> Vec<EvaluatedSceneNode> {
    use super::placement::{PlacedItem, SceneVisitor, SceneWalker, WalkMode};

    struct RenderVisitor<'a> {
        visibility: Option<&'a PreviewVisibility>,
        used_ids: HashSet<ItemId>,
        frame_rate: super::FrameRate,
    }

    impl SceneVisitor for RenderVisitor<'_> {
        type State = super::properties::ResolvedItem;
        type Output = EvaluatedSceneNode;

        fn prepare(&mut self, source: &mut TimelineItem, _path: &[ItemId]) {
            if let Some(visibility) = self.visibility {
                visibility.retain_visible_effects(source);
            }
        }

        fn enter(
            &mut self,
            placed: &PlacedItem,
            time: Option<TimelineTime>,
            bindings: Option<&SceneBindings<'_>>,
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

    let bindings = scene.map(|scene| SceneBindings::new(scenes, scene, None));
    SceneWalker {
        scenes,
        visibility,
        frame_rate,
    }
    .walk(
        items,
        WalkMode::Sample(time),
        bindings.as_ref(),
        &mut RenderVisitor {
            visibility,
            used_ids: HashSet::new(),
            frame_rate,
        },
    )
}

pub(crate) fn evaluated_items_at_time(
    items: Vec<(LayerId, TimelineItem)>,
    scenes: &HashMap<SceneId, SceneDefinition>,
    time: TimelineTime,
    visibility: Option<&PreviewVisibility>,
    scene: Option<&SceneDefinition>,
    frame_rate: super::FrameRate,
) -> Vec<(LayerId, super::EvaluatedItem)> {
    fn flatten(nodes: Vec<EvaluatedSceneNode>, output: &mut Vec<(LayerId, super::EvaluatedItem)>) {
        for node in nodes {
            if node.item.scene_id().is_some() {
                flatten(node.children, output);
            } else {
                output.push((node.layer, node.item));
            }
        }
    }

    let graph = evaluated_scene_graph_at_time(items, scenes, time, visibility, scene, frame_rate);
    let mut output = Vec::new();
    flatten(graph, &mut output);
    output
}
