//! Targeted, demand-driven render evaluation. Placement lookup never runs expressions.
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use super::{
    CalculationDiagnostic, EffectInstanceId, Frame, ItemId, LayerId, SceneDefinition, SceneId,
    TimelineDocument, TimelineItem, TimelineTime,
    evaluation::{scene_runtime_seed, unique_runtime_id},
    properties::{SceneBindings, resolve_property},
    property_address::property_schemas,
    visibility::PreviewVisibility,
};
use crate::{plugin::TextureInput, property::PropertyValues};

#[derive(Clone, Debug)]
pub struct RenderPlacement {
    pub path: Vec<ItemId>,
    pub local_layer: LayerId,
    pub runtime_id: ItemId,
    pub start: Frame,
    pub source: Arc<TimelineItem>,
    pub duration: super::FrameDuration,
}

impl RenderPlacement {
    pub fn local_time(&self, time: TimelineTime) -> TimelineTime {
        TimelineTime::from_frames(time.frames() - self.start.get() as f64)
    }

    pub fn global_time(&self, local: TimelineTime) -> TimelineTime {
        TimelineTime::from_frames(self.start.get() as f64 + local.frames())
    }

    /// An image exists within this item's expanded interval. Parent output
    /// intervals are enforced when their images are composed, not here.
    pub fn contains(&self, time: TimelineTime) -> bool {
        let local = self.local_time(time).frames();
        local >= 0. && local < self.duration.get() as f64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RenderPropertyTarget {
    Item,
    Effect(EffectInstanceId),
    /// Only this owner's capture ranges and original-output visibility.
    CaptureTopology {
        effect: Option<EffectInstanceId>,
    },
}

/// Evaluated values for exactly one owner and purpose, never a partial item.
pub struct ResolvedRenderProperties {
    pub values: PropertyValues,
    pub diagnostics: Vec<CalculationDiagnostic>,
}

/// One canonical expanded placement per instance path, shared by lookup,
/// composition enumeration and property bindings.
struct PlacementEntry {
    placement: RenderPlacement,
    parent: Option<Arc<PlacementEntry>>,
    seed: u64,
}

pub struct RenderEvaluation<'a> {
    document: &'a TimelineDocument,
    scenes: &'a HashMap<SceneId, SceneDefinition>,
    visibility: Option<&'a PreviewVisibility>,
    scene: Option<&'a SceneDefinition>,
    placements: RefCell<HashMap<Vec<ItemId>, Arc<PlacementEntry>>>,
    used_ids: RefCell<HashSet<ItemId>>,
}

impl<'a> RenderEvaluation<'a> {
    pub(super) fn new(
        document: &'a TimelineDocument,
        scenes: &'a HashMap<SceneId, SceneDefinition>,
        visibility: Option<&'a PreviewVisibility>,
        scene: Option<&'a SceneDefinition>,
    ) -> Self {
        Self {
            document,
            scenes,
            visibility,
            scene,
            placements: RefCell::default(),
            used_ids: RefCell::default(),
        }
    }

    fn placement_entry(&self, path: &[ItemId]) -> Option<Arc<PlacementEntry>> {
        if let Some(entry) = self.placements.borrow().get(path) {
            return Some(entry.clone());
        }
        let (&id, scope) = path.split_last()?;
        let (layer, mut source, parent, seed) = if scope.is_empty() {
            let layer = self.document.item_layer(id)?;
            if self
                .visibility
                .is_some_and(|v| !v.is_item_visible(layer, id))
            {
                return None;
            }
            (layer, self.document.item(id)?.clone(), None, 0)
        } else {
            let parent = self.placement_entry(scope)?;
            let scene = self.scenes.get(&parent.placement.source.scene_id()?)?;
            let seed = scene_runtime_seed(parent.seed, parent.placement.source.id, scene.id);
            let (layer, source) = scene.expanded_item(&parent.placement.source, id)?;
            (layer, source, Some(parent), seed)
        };
        if let Some(visibility) = self.visibility {
            visibility.retain_visible_effects(&mut source);
        }
        let start = Frame::new(
            parent
                .as_ref()
                .map_or(0, |parent| parent.placement.start.get())
                .saturating_add(source.start().get()),
        );
        let runtime_id = unique_runtime_id(seed, source.id, &mut self.used_ids.borrow_mut());
        let entry = Arc::new(PlacementEntry {
            placement: RenderPlacement {
                path: path.to_vec(),
                local_layer: layer,
                runtime_id,
                start,
                duration: source.duration(self.document.frame_rate()),
                source: Arc::new(source),
            },
            parent,
            seed,
        });
        self.placements
            .borrow_mut()
            .insert(path.to_vec(), entry.clone());
        Some(entry)
    }

    pub fn placement(&self, path: &[ItemId]) -> Option<RenderPlacement> {
        Some(self.placement_entry(path)?.placement.clone())
    }

    /// Enumerate just one composition; ancestors are resolved directly by ID.
    /// Contents can be captured outside this scope's own output interval. A
    /// request for the scene's image checks that interval before composing it.
    pub fn composition(&self, path: &[ItemId], time: TimelineTime) -> Option<Vec<RenderPlacement>> {
        let ids = if path.is_empty() {
            self.document.active_source_item_ids_at_time(time)
        } else {
            let parent = self.placement_entry(path)?;
            let placement = &parent.placement;
            let scene = self.scenes.get(&placement.source.scene_id()?)?;
            scene.active_expanded_item_ids(&placement.source, placement.local_time(time))
        };
        Some(
            ids.into_iter()
                .filter_map(|id| {
                    let mut child_path = path.to_vec();
                    child_path.push(id);
                    let placed = self.placement(&child_path)?;
                    placed.contains(time).then_some(placed)
                })
                .collect(),
        )
    }

    pub fn properties(
        &self,
        path: &[ItemId],
        time: TimelineTime,
        target: RenderPropertyTarget,
    ) -> Option<ResolvedRenderProperties> {
        let entry = self.placement_entry(path)?;
        let mut chain = Vec::new();
        let mut current = Some(entry.as_ref());
        while let Some(entry) = current {
            chain.push(&entry.placement);
            current = entry.parent.as_deref();
        }
        chain.reverse();
        let bindings = self
            .scene
            .map(|scene| SceneBindings::new(self.scenes, scene, None));
        self.resolve_chain(&chain, 0, time, bindings.as_ref(), target)
    }

    fn resolve_chain(
        &self,
        chain: &[&RenderPlacement],
        index: usize,
        time: TimelineTime,
        bindings: Option<&SceneBindings<'_>>,
        target: RenderPropertyTarget,
    ) -> Option<ResolvedRenderProperties> {
        let placed = &chain[index];
        let parent_start = placed
            .start
            .get()
            .saturating_sub(placed.source.start().get());
        let scoped_time = TimelineTime::from_frames(time.frames() - parent_start as f64);
        if index + 1 < chain.len() {
            let scene = &self.scenes[&placed.source.scene_id().expect("ancestor is a scene")];
            let child_bindings = SceneBindings::for_instance(
                self.scenes,
                scene,
                &placed.source,
                Some(scoped_time),
                bindings,
            );
            return self.resolve_chain(chain, index + 1, time, Some(&child_bindings), target);
        }
        let source = &placed.source;
        let effect = match target {
            RenderPropertyTarget::Item => None,
            RenderPropertyTarget::Effect(id) => Some(id),
            RenderPropertyTarget::CaptureTopology { effect } => effect,
        };
        let inputs = match effect {
            Some(id) => source.effect(id)?.schema().inputs(),
            None => source.schema().map(|schema| schema.inputs()).unwrap_or(&[]),
        };
        let mut diagnostics = Vec::new();
        let mut values = PropertyValues::default();
        for property in property_schemas(self.scenes, source, effect) {
            let used = match target {
                RenderPropertyTarget::Item => source
                    .schema()
                    .and_then(|schema| schema.render())
                    .is_some_and(|render| {
                        render.properties().iter().any(|p| p.id() == property.id())
                    }),
                RenderPropertyTarget::Effect(_) => true,
                RenderPropertyTarget::CaptureTopology { .. } => inputs.iter().any(|input| {
                    if let TextureInput::RenderResult {
                        start_offset,
                        end_offset,
                        hide_original,
                        ..
                    } = input
                    {
                        let id = property.id();
                        id == start_offset || id == end_offset || id == hide_original
                    } else {
                        false
                    }
                }),
            };
            // Scene arguments are resolved lazily through child bindings.
            if !used {
                continue;
            }
            let resolved = resolve_property(bindings, source, effect, property, Some(scoped_time))?;
            diagnostics.extend(resolved.diagnostics);
            values
                .set(property, resolved.value)
                .expect("property contract");
        }
        Some(ResolvedRenderProperties {
            values,
            diagnostics,
        })
    }
}
