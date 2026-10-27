//! Consumer-independent scene placement, clipping and traversal.
use super::{EvaluatedClip, properties::SceneBindings, scene_runtime_seed};
use crate::timeline::{ItemId, LayerId, SceneDefinition, SceneId, TimelineItem, TimelineTime};
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub(in crate::timeline) enum WalkMode {
    Sample(TimelineTime),
    AudioPlan,
    Occupied,
    Definitions { include_hidden: bool },
}

pub(in crate::timeline) struct PlacedItem {
    pub source: TimelineItem,
    pub local_layer: LayerId,
    pub layer: LayerId,
    pub clip: EvaluatedClip,
    pub offset: u64,
    pub seed: u64,
    pub path: Vec<ItemId>,
}

pub(in crate::timeline) trait SceneVisitor {
    type State;
    type Output;

    fn enter(
        &mut self,
        item: &PlacedItem,
        time: Option<TimelineTime>,
        bindings: &SceneBindings<'_>,
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

pub(in crate::timeline) struct SceneWalker<'a> {
    pub frame_rate: crate::timeline::FrameRate,
    pub scenes: &'a HashMap<SceneId, SceneDefinition>,
}

impl SceneWalker<'_> {
    pub(in crate::timeline) fn walk<V: SceneVisitor>(
        &self,
        scene: &SceneDefinition,
        mode: WalkMode,
        bindings: &SceneBindings<'_>,
        visitor: &mut V,
    ) -> Vec<V::Output> {
        let items = if let WalkMode::Sample(time) = mode {
            scene.document().active_source_items_at_time(time)
        } else {
            scene.document().source_items()
        };
        self.scope(scene, items, mode, bindings, Scope::default(), visitor)
    }

    fn scope<V: SceneVisitor>(
        &self,
        scene: &SceneDefinition,
        items: Vec<(LayerId, TimelineItem)>,
        mode: WalkMode,
        bindings: &SceneBindings<'_>,
        scope: Scope,
        visitor: &mut V,
    ) -> Vec<V::Output> {
        let include_hidden = if let WalkMode::Definitions { include_hidden } = mode {
            include_hidden
        } else {
            false
        };
        let mut output = Vec::new();
        for (local_layer, mut source) in items {
            if !include_hidden {
                if !scene.document().is_item_visible(source.id) {
                    continue;
                }
                source.effects.retain(|effect| !effect.hidden);
            }
            let mut path = scope.path.clone();
            path.push(source.id);
            // Complete definitions are independent of placed offsets and clips.
            // Large instance positions must not hide authored file references.
            let start = if let WalkMode::Definitions { .. } = mode {
                source.start().get()
            } else {
                scope.offset.saturating_add(source.start().get())
            };
            let end = start.saturating_add(source.duration(self.frame_rate).get());
            let end = scope.end.map_or(end, |clip| clip.min(end));
            let Some(clip) = EvaluatedClip::from_bounds(start, end) else {
                continue;
            };
            let time = match mode {
                WalkMode::Sample(time) => Some(time),
                WalkMode::AudioPlan => Some(TimelineTime::from_frame(source.start())),
                WalkMode::Occupied | WalkMode::Definitions { .. } => None,
            };
            let scene = source.scene_id().and_then(|id| self.scenes.get(&id));
            if source.scene_id().is_some() && scene.is_none() {
                continue;
            }
            let child_mode = match mode {
                WalkMode::Sample(time) => {
                    let local =
                        TimelineTime::from_frames(time.frames() - source.start().get() as f64);
                    if local.frames() < 0.
                        || local.frames() >= source.duration(self.frame_rate).get() as f64
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
                    WalkMode::Definitions { .. } => scene.document().source_items(),
                    WalkMode::Sample(time) => scene.expanded_items(&item.source, Some(time)),
                    _ => scene.expanded_items(&item.source, None),
                };
                let child_bindings =
                    SceneBindings::for_instance(self.scenes, scene, &item.source, time, bindings);
                self.scope(
                    scene,
                    children,
                    child_mode,
                    &child_bindings,
                    Scope {
                        offset: start,
                        end: if let WalkMode::Definitions { .. } = mode {
                            None
                        } else {
                            Some(end)
                        },
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
