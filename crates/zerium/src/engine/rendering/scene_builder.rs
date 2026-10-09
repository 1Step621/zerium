use super::scene::MediaFrameRequest;
use super::scene::{
    RenderEffect, RenderItem, RenderItemSource, RenderNodeContent, RenderTemporalSample, RenderView,
};
use super::scene::{RenderError, RenderNode, RenderQuality, RenderScene, RenderSize, SceneNodeId};
use super::surface::{SurfaceRect, item_bounds};
use super::text::TextFrameRequest;
use super::text::TextSourceId;
use crate::engine::frame::RgbaFrame;
use std::{collections::HashMap, sync::Arc};
use zerium_core::plugin::{EffectPassSchema, TextureInput};
use zerium_core::timeline::{
    EffectInstance, EffectInstanceId, LayerId, RenderResultSettings, TimelineItem, TimelineItemKind,
};
use zerium_core::timeline::{EvaluatedSceneNode, ItemId, TimelineTime, TimelineView};
use zerium_shader::ItemShaderId;

const MAX_TEMPORAL_DEPTH: usize = 4;
const MAX_TEMPORAL_RENDER_NODES: usize = 4_096;

type MediaFrameCache =
    HashMap<(ItemId, Option<EffectInstanceId>, usize, u64, RenderSize), Option<Arc<RgbaFrame>>>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RenderCacheKey {
    path: Vec<ItemId>,
    effect_count: usize,
    time_bits: u64,
    temporal_depth: usize,
}

struct SceneCapturePlan {
    source_layer: LayerId,
    range: RenderResultSettings,
    nodes: Vec<usize>,
}

/// Resolves which nodes appear normally and in each render_result input for one scene scope.
struct SceneCompositionPlan {
    nodes: Vec<EvaluatedSceneNode>,
    normal_nodes: Vec<usize>,
    captures: Vec<SceneCapturePlan>,
}

impl SceneCompositionPlan {
    fn new(nodes: Vec<EvaluatedSceneNode>) -> Self {
        let ranges = nodes
            .iter()
            .flat_map(|node| {
                node.item
                    .render_result_ranges()
                    .map(|range| (node.local_layer, range))
            })
            .collect::<Vec<_>>();
        let normal_nodes = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                !ranges.iter().any(|(source, range)| {
                    range.hide_original && range.includes(*source, node.local_layer)
                })
            })
            .map(|(index, _)| index)
            .collect();
        let captures = ranges
            .iter()
            .map(|&(source_layer, range)| {
                let captured = nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, node)| {
                        range.includes(source_layer, node.local_layer)
                            && !ranges.iter().any(|(nested_layer, nested_range)| {
                                *nested_layer != node.local_layer
                                    && range.includes(source_layer, *nested_layer)
                                    && nested_range.hide_original
                                    && nested_range.includes(*nested_layer, node.local_layer)
                            })
                    })
                    .map(|(index, _)| index)
                    .collect();
                SceneCapturePlan {
                    source_layer,
                    range,
                    nodes: captured,
                }
            })
            .collect();
        Self {
            nodes,
            normal_nodes,
            captures,
        }
    }

    fn normal_nodes(&self) -> impl Iterator<Item = &EvaluatedSceneNode> + '_ {
        self.normal_nodes.iter().map(|&index| &self.nodes[index])
    }

    fn captured_nodes(
        &self,
        source_layer: LayerId,
        range: RenderResultSettings,
    ) -> impl Iterator<Item = &EvaluatedSceneNode> + '_ {
        self.captures
            .iter()
            .find(|capture| capture.source_layer == source_layer && capture.range == range)
            .into_iter()
            .flat_map(|capture| capture.nodes.iter().map(|&index| &self.nodes[index]))
    }
}

/// An item's or effect's texture inputs and their shared property values.
struct InputSource<'a> {
    item_id: ItemId,
    effect_id: Option<EffectInstanceId>,
    inputs: &'a [TextureInput],
    properties: &'a zerium_core::property::PropertyValues,
    label: &'a str,
}

impl<'a> InputSource<'a> {
    fn item(item: &'a TimelineItem) -> Self {
        let schema = item.schema().expect("renderable item has a schema");
        Self {
            item_id: item.id,
            effect_id: None,
            inputs: schema.inputs(),
            properties: &item.properties,
            label: schema.label(),
        }
    }

    fn effect(item_id: ItemId, effect: &'a EffectInstance) -> Self {
        let schema = effect.schema();
        Self {
            item_id,
            effect_id: Some(effect.id),
            inputs: schema.inputs(),
            properties: &effect.properties,
            label: schema.label(),
        }
    }
}

/// One frame's CPU render graph. All dependencies refer to shared node IDs;
/// item and scene effects use the same temporal sampling and texture input resolver.
pub(super) struct SceneBuilder<'a, M, T> {
    timeline: &'a dyn TimelineView,
    size: RenderSize,
    quality: RenderQuality,
    nodes: Vec<RenderNode>,
    graph_cache: HashMap<u64, Arc<Vec<EvaluatedSceneNode>>>,
    compositions: HashMap<(u64, Vec<ItemId>), Arc<SceneCompositionPlan>>,
    node_cache: HashMap<RenderCacheKey, Option<SceneNodeId>>,
    media_cache: MediaFrameCache,
    temporal_nodes_remaining: usize,
    media_frame: M,
    text_frame: T,
}

impl<E, M, T> SceneBuilder<'_, M, T>
where
    E: From<RenderError>,
    M: FnMut(MediaFrameRequest<'_>) -> Result<Option<Arc<RgbaFrame>>, E>,
    T: FnMut(TextFrameRequest<'_>) -> Result<Arc<RgbaFrame>, RenderError>,
{
    pub(super) fn build(
        timeline: &dyn TimelineView,
        time: TimelineTime,
        size: RenderSize,
        quality: RenderQuality,
        media_frame: M,
        text_frame: T,
    ) -> Result<RenderScene, E> {
        let mut builder = SceneBuilder {
            timeline,
            size,
            quality,
            nodes: Vec::new(),
            graph_cache: HashMap::new(),
            compositions: HashMap::new(),
            node_cache: HashMap::new(),
            media_cache: HashMap::new(),
            temporal_nodes_remaining: MAX_TEMPORAL_RENDER_NODES,
            media_frame,
            text_frame,
        };
        let composition = builder
            .composition(time, &[])
            .expect("root scope always exists");
        let roots = builder.render_scope(&composition, time, 0)?;
        Ok(RenderScene::from_graph(
            size,
            RenderSize::from(timeline.resolution()),
            builder.nodes,
            roots,
        )?)
    }

    fn composition(
        &mut self,
        time: TimelineTime,
        path: &[ItemId],
    ) -> Option<Arc<SceneCompositionPlan>> {
        let key = (time.frames().to_bits(), path.to_vec());
        if let Some(plan) = self.compositions.get(&key) {
            return Some(plan.clone());
        }
        let graph = self
            .graph_cache
            .entry(key.0)
            .or_insert_with(|| Arc::new(self.timeline.active_scene_graph_at_time(time)));
        let mut scope = graph.as_slice();
        for id in path {
            let node = scope.iter().find(|node| node.path.last() == Some(id))?;
            node.item.scene_id()?;
            scope = &node.children;
        }
        let plan = Arc::new(SceneCompositionPlan::new(scope.to_vec()));
        self.compositions.insert(key, plan.clone());
        Some(plan)
    }

    fn render_scope(
        &mut self,
        composition: &SceneCompositionPlan,
        time: TimelineTime,
        depth: usize,
    ) -> Result<Vec<SceneNodeId>, E> {
        let mut children = Vec::new();
        for child in composition.normal_nodes() {
            if let Some(id) = self.render_node(&child.path, None, time, depth)? {
                children.push(id);
            }
        }
        Ok(children)
    }

    fn render_node(
        &mut self,
        path: &[ItemId],
        effect_count: Option<usize>,
        time: TimelineTime,
        depth: usize,
    ) -> Result<Option<SceneNodeId>, E> {
        let Some(composition) = self.composition(time, &path[..path.len() - 1]) else {
            return Ok(None);
        };
        let Some(node) = composition.nodes.iter().find(|node| node.path == path) else {
            return Ok(None);
        };
        let owner = &node.item;
        let effect_count = effect_count.unwrap_or(owner.effects.len());
        let key = RenderCacheKey {
            path: path.to_vec(),
            effect_count,
            time_bits: time.frames().to_bits(),
            temporal_depth: depth,
        };
        if let Some(id) = self.node_cache.get(&key) {
            return Ok(*id);
        }
        let render_scale = owner
            .effects
            .iter()
            .take(effect_count)
            .map(|effect| effect.schema().render().scale)
            .max()
            .unwrap_or(1);
        let mut content = match &node.item.kind {
            TimelineItemKind::Plugin {
                plugin_id,
                item_id,
                schema,
            } => {
                let item = &node.item;
                let Some(render) = schema.render() else {
                    self.node_cache.insert(key, None);
                    return Ok(None);
                };
                let target_size = self.scaled_size(render_scale)?;
                let inputs = self.render_inputs(
                    InputSource::item(item),
                    node,
                    &composition,
                    time,
                    target_size,
                    depth,
                )?;
                RenderNodeContent::Item(RenderItem {
                    shader: ItemShaderId::plugin_item(plugin_id, item_id),
                    source: RenderItemSource::Shader,
                    inputs,
                    properties: RenderScene::pack_item_properties(item, schema),
                    effects: Vec::new(),
                    target_size,
                    render_scale,
                    output_bounds: item_bounds(
                        &render.bounds,
                        &item.properties,
                        RenderSize::from(self.timeline.resolution()),
                    ),
                })
            }
            TimelineItemKind::Scene { .. } => {
                let child_scope = self
                    .composition(time, path)
                    .expect("evaluated scene has a scope");
                let children = self.render_scope(&child_scope, time, depth)?;
                RenderNodeContent::Scene {
                    children,
                    view: RenderView::default(),
                    effects: Vec::new(),
                    render_scale,
                }
            }
        };
        let source_scale = self.content_scale(&content);
        let effects =
            self.render_effects(node, &composition, effect_count, time, source_scale, depth)?;
        match &mut content {
            RenderNodeContent::Item(item) => item.effects = effects,
            RenderNodeContent::Scene {
                effects: target, ..
            } => *target = effects,
        }
        let id = self.push_node(content);
        self.node_cache.insert(key, Some(id));
        Ok(Some(id))
    }

    fn scaled_size(&self, scale: u32) -> Result<RenderSize, RenderError> {
        self.size
            .checked_scale(scale)
            .ok_or_else(|| RenderError::resource_limit("render size overflows"))
    }

    fn content_scale(&self, content: &RenderNodeContent) -> u32 {
        let (scale, inputs, effects) = match content {
            RenderNodeContent::Item(item) => (item.render_scale, &item.inputs, &item.effects),
            RenderNodeContent::Scene {
                children,
                effects,
                render_scale,
                ..
            } => (*render_scale, children, effects),
        };
        inputs
            .iter()
            .chain(effects.iter().flat_map(|effect| &effect.inputs))
            .map(|id| self.nodes[*id].render_scale)
            .fold(scale, u32::max)
    }

    fn push_node(&mut self, content: RenderNodeContent) -> SceneNodeId {
        let render_scale = self.content_scale(&content);
        let id = self.nodes.len();
        self.nodes.push(RenderNode {
            content,
            render_scale,
        });
        id
    }

    fn render_effects(
        &mut self,
        node: &EvaluatedSceneNode,
        composition: &SceneCompositionPlan,
        count: usize,
        time: TimelineTime,
        scale: u32,
        depth: usize,
    ) -> Result<Vec<RenderEffect>, E> {
        let mut effects = Vec::with_capacity(count);
        for (effect_index, instance) in node.item.effects.iter().take(count).enumerate() {
            let mut samples = Vec::new();
            for pass in &instance.schema().render().passes {
                samples.push(match pass {
                    EffectPassSchema::Temporal { sampling, .. } => sampling
                        .sample_offsets(&instance.properties)
                        .map(|offsets| {
                            self.temporal_samples(&node.path, effect_index, time, offsets, depth)
                        })
                        .transpose()?,
                    _ => None,
                });
            }
            let mut effect = RenderScene::render_effect(instance, samples);
            effect.inputs = self.render_inputs(
                InputSource::effect(node.item.id, instance),
                node,
                composition,
                time,
                self.scaled_size(scale)?,
                depth,
            )?;
            effects.push(effect);
        }
        Ok(effects)
    }

    fn temporal_samples(
        &mut self,
        path: &[ItemId],
        effect_index: usize,
        time: TimelineTime,
        offsets: Vec<f64>,
        depth: usize,
    ) -> Result<Vec<RenderTemporalSample>, E> {
        if depth >= MAX_TEMPORAL_DEPTH {
            return Err(RenderError::resource_limit(format!(
                "temporal effect depth exceeds {MAX_TEMPORAL_DEPTH}"
            ))
            .into());
        }
        self.quality
            .temporal_offsets(offsets)
            .into_iter()
            .map(|offset| {
                self.temporal_nodes_remaining = self
                    .temporal_nodes_remaining
                    .checked_sub(1)
                    .ok_or_else(|| {
                        RenderError::resource_limit(format!(
                            "temporal render graph exceeds {MAX_TEMPORAL_RENDER_NODES} nodes"
                        ))
                    })?;
                let sample_time = time.offset(offset);
                let input = self.render_node(path, Some(effect_index), sample_time, depth + 1)?;
                Ok(RenderTemporalSample {
                    frame_offset: offset as f32,
                    input,
                })
            })
            .collect()
    }

    fn render_inputs(
        &mut self,
        source: InputSource<'_>,
        node: &EvaluatedSceneNode,
        composition: &SceneCompositionPlan,
        time: TimelineTime,
        target_size: RenderSize,
        depth: usize,
    ) -> Result<Vec<SceneNodeId>, E> {
        let mut inputs = Vec::with_capacity(source.inputs.len());
        for (index, capability) in source.inputs.iter().enumerate() {
            let input = match capability {
                TextureInput::Media { .. } => {
                    let key = (
                        source.item_id,
                        source.effect_id,
                        index,
                        time.frames().to_bits(),
                        target_size,
                    );
                    let frame = match self.media_cache.entry(key) {
                        std::collections::hash_map::Entry::Occupied(entry) => entry.get().clone(),
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            let frame = (self.media_frame)(MediaFrameRequest {
                                item_id: source.item_id,
                                effect_id: source.effect_id,
                                input_id: capability.id(),
                                time,
                                target_size,
                            })?;
                            entry.insert(frame.clone());
                            frame
                        }
                    }
                    .unwrap_or_else(|| {
                        Arc::new(RgbaFrame {
                            width: 1,
                            height: 1,
                            rgba: Arc::from([0u8; 4]),
                        })
                    });
                    self.frame_node(frame, target_size)
                }
                TextureInput::Text { .. } | TextureInput::Number { .. } => {
                    let frame = (self.text_frame)(TextFrameRequest {
                        id: TextSourceId {
                            item_id: source.item_id,
                            effect_id: source.effect_id,
                            capability_index: index,
                        },
                        capability,
                        properties: source.properties,
                        label: source.label,
                        target_size,
                    })?;
                    self.frame_node(frame, target_size)
                }
                TextureInput::RenderResult {
                    start_offset,
                    end_offset,
                    hide_original,
                    view,
                    ..
                } => {
                    let settings = RenderResultSettings::from_properties(
                        source.properties,
                        start_offset,
                        end_offset,
                        hide_original,
                    )
                    .expect("render_result properties come from validated schemas");
                    let mut children = Vec::new();
                    for candidate in composition.captured_nodes(node.local_layer, settings) {
                        if let Some(child) = self.render_node(&candidate.path, None, time, depth)? {
                            children.push(child);
                        }
                    }
                    self.push_node(RenderNodeContent::Scene {
                        children,
                        view: view.as_ref().map_or_else(RenderView::default, |view| {
                            let value = |id: &str, index| {
                                source
                                    .properties
                                    .property(id)
                                    .and_then(|value| value.scalar_at(index))
                                    .and_then(|value| value.as_f32())
                                    .expect("capture view properties come from validated schemas")
                            };
                            RenderView {
                                position: [
                                    value(&view.position, Some(0)),
                                    value(&view.position, Some(1)),
                                ],
                                zoom: value(&view.zoom, None) / 100.,
                                angle: value(&view.angle, None),
                            }
                        }),
                        effects: Vec::new(),
                        render_scale: 1,
                    })
                }
            };
            inputs.push(input);
        }
        Ok(inputs)
    }

    fn frame_node(&mut self, frame: Arc<RgbaFrame>, target_size: RenderSize) -> SceneNodeId {
        self.push_node(RenderNodeContent::Item(RenderItem {
            shader: ItemShaderId::host_capability_frame(),
            source: RenderItemSource::Texture(vec![frame]),
            inputs: Vec::new(),
            properties: Vec::new(),
            effects: Vec::new(),
            target_size,
            render_scale: 1,
            output_bounds: SurfaceRect::viewport(RenderSize::from(self.timeline.resolution())),
        }))
    }
}
