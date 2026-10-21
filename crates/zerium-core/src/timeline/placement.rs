//! Consumer-independent scene placement, clipping and traversal.
use super::{
    ItemId, LayerId, SceneDefinition, SceneId, TimelineItem, TimelineTime,
    evaluation::{EvaluatedClip, scene_runtime_seed},
    properties::SceneBindings,
    visibility::PreviewVisibility,
};
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub(super) enum WalkMode {
    Sample(TimelineTime),
    AudioPlan,
    Occupied,
    Definitions,
}

pub(super) struct PlacedItem {
    pub source: TimelineItem,
    pub local_layer: LayerId,
    pub layer: LayerId,
    pub clip: EvaluatedClip,
    pub offset: u64,
    pub seed: u64,
    pub path: Vec<ItemId>,
}

pub(super) trait SceneVisitor {
    type State;
    type Output;

    fn prepare(&mut self, _source: &mut TimelineItem, _path: &[ItemId]) {}

    fn enter(
        &mut self,
        item: &PlacedItem,
        time: Option<TimelineTime>,
        bindings: Option<&SceneBindings<'_>>,
    ) -> Self::State;

    fn exit(
        &mut self,
        item: PlacedItem,
        state: Self::State,
        children: Vec<Self::Output>,
    ) -> Option<Self::Output>;
}

#[derive(Default)]
struct Scope {
    offset: u64,
    end: Option<u64>,
    seed: u64,
    layer: Option<LayerId>,
    path: Vec<ItemId>,
}

pub(super) struct SceneWalker<'a> {
    pub scenes: &'a HashMap<SceneId, SceneDefinition>,
    pub visibility: Option<&'a PreviewVisibility>,
}

impl SceneWalker<'_> {
    pub(super) fn walk<V: SceneVisitor>(
        &self,
        items: Vec<(LayerId, TimelineItem)>,
        mode: WalkMode,
        bindings: Option<&SceneBindings<'_>>,
        visitor: &mut V,
    ) -> Vec<V::Output> {
        self.scope(items, mode, bindings, Scope::default(), visitor)
    }

    fn scope<V: SceneVisitor>(
        &self,
        items: Vec<(LayerId, TimelineItem)>,
        mode: WalkMode,
        bindings: Option<&SceneBindings<'_>>,
        scope: Scope,
        visitor: &mut V,
    ) -> Vec<V::Output> {
        let mut output = Vec::new();
        for (local_layer, mut source) in items {
            if scope.path.is_empty()
                && self
                    .visibility
                    .is_some_and(|v| !v.is_item_visible(local_layer, source.id))
            {
                continue;
            }
            let mut path = scope.path.clone();
            path.push(source.id);
            visitor.prepare(&mut source, &path);
            // Complete definitions are independent of placed offsets and clips.
            // Large instance positions must not hide authored file references.
            let start = if matches!(mode, WalkMode::Definitions) {
                source.start.get()
            } else {
                scope.offset.saturating_add(source.start.get())
            };
            let end = start.saturating_add(source.duration.get());
            let end = scope.end.map_or(end, |clip| clip.min(end));
            let Some(clip) = EvaluatedClip::from_bounds(start, end) else {
                continue;
            };
            let time = match mode {
                WalkMode::Sample(time) => Some(time),
                WalkMode::AudioPlan => Some(TimelineTime::from_frame(source.start)),
                WalkMode::Occupied | WalkMode::Definitions => None,
            };
            let scene = source.scene_id().and_then(|id| self.scenes.get(&id));
            if source.scene_id().is_some() && scene.is_none() {
                continue;
            }
            let child_mode = match mode {
                WalkMode::Sample(time) => {
                    let local =
                        TimelineTime::from_frames(time.frames() - source.start.get() as f64);
                    if scene.is_some()
                        && (local.frames() < 0. || local.frames() >= source.duration.get() as f64)
                    {
                        continue;
                    }
                    WalkMode::Sample(local)
                }
                _ => mode,
            };
            let item = PlacedItem {
                source,
                local_layer,
                layer: scope.layer.unwrap_or(local_layer),
                clip,
                offset: scope.offset,
                seed: scope.seed,
                path,
            };
            let state = visitor.enter(&item, time, bindings);
            let children = if let Some(scene) = scene {
                let children = match child_mode {
                    WalkMode::Definitions => scene.document().source_items(),
                    WalkMode::Sample(time) => scene.expanded_items(&item.source, Some(time)),
                    _ => scene.expanded_items(&item.source, None),
                };
                let child_bindings =
                    SceneBindings::for_instance(self.scenes, scene, &item.source, time, bindings);
                self.scope(
                    children,
                    child_mode,
                    Some(&child_bindings),
                    Scope {
                        offset: start,
                        end: (!matches!(mode, WalkMode::Definitions)).then_some(end),
                        seed: scene_runtime_seed(scope.seed, item.source.id, scene.id),
                        layer: Some(item.layer),
                        path: item.path.clone(),
                    },
                    visitor,
                )
            } else {
                Vec::new()
            };
            if let Some(result) = visitor.exit(item, state, children) {
                output.push(result);
            }
        }
        output
    }
}
