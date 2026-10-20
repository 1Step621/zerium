use super::composition::SceneCompositionPlan;
use super::geometry::BoundsOperation;
use super::text::TextFrameRequest;
use super::text::TextSourceId;
use super::{
    RenderEffect, RenderEffectPass, RenderEffectPassKind, RenderInput, RenderItem,
    RenderNodeContent, RenderTemporalSample,
};
use super::{RenderError, RenderNode, RenderScene, RenderSize, SceneNodeId};
use crate::engine::frame::VisualFrame;
use crate::engine::media::{MediaFrameRequest, MediaInputId, RasterSize};
use std::{collections::HashMap, sync::Arc};
use zerium_core::plugin::{EffectPassSchema, TextureInput};
use zerium_core::timeline::{
    BlendMode, EffectInstance, EffectInstanceId, TimelineItem, TimelineItemKind,
};
use zerium_core::timeline::{
    EvaluatedSceneNode, ItemId, ProjectResolution, TimelineTime, TimelineView,
};
use zerium_shader::{EffectShaderId, ItemShaderId};

const MAX_TEMPORAL_DEPTH: usize = 4;
const MAX_TEMPORAL_RENDER_NODES: usize = 4_096;

type MediaFrameCache =
    HashMap<(ItemId, Option<EffectInstanceId>, usize, u64, RenderSize), Option<Arc<VisualFrame>>>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RenderCacheKey {
    path: Vec<ItemId>,
    effect_count: usize,
    time_bits: u64,
    temporal_depth: usize,
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

    /// The media capability declares its placement; the source decides how many
    /// of the requested pixels it can produce.
    fn media_raster_size(
        &self,
        input: &TextureInput,
        target_size: RenderSize,
        composition_size: RenderSize,
    ) -> RenderSize {
        let TextureInput::Media {
            placement: Some(placement),
            ..
        } = input
        else {
            return target_size;
        };
        let Some(size_value) = self.properties.property(&placement.size) else {
            return target_size;
        };
        let displayed = [0, 1].map(|index| {
            size_value
                .scalar_at(Some(index))
                .and_then(|value| value.numeric_scalar())
                .unwrap_or(0.)
        });
        let dimension = |displayed: f64, target: u32, composition: u32| {
            let needed = (displayed * f64::from(target) / f64::from(composition.max(1))).ceil();
            if needed.is_finite() && needed > 0. {
                target.max(needed.min(f64::from(ProjectResolution::MAX_DIMENSION)) as u32)
            } else {
                target
            }
        };
        RenderSize {
            width: dimension(displayed[0], target_size.width, composition_size.width),
            height: dimension(displayed[1], target_size.height, composition_size.height),
        }
    }
}

/// One frame's CPU render graph. All dependencies refer to shared node IDs;
/// item and scene effects use the same temporal sampling and texture input resolver.
pub(super) struct SceneGraphBuilder<'a, M, T> {
    timeline: &'a dyn TimelineView,
    size: RenderSize,
    nodes: Vec<RenderNode>,
    graph_cache: HashMap<u64, Arc<Vec<EvaluatedSceneNode>>>,
    compositions: HashMap<(u64, Vec<ItemId>), Arc<SceneCompositionPlan>>,
    node_cache: HashMap<RenderCacheKey, Option<SceneNodeId>>,
    media_cache: MediaFrameCache,
    temporal_nodes_remaining: usize,
    media_frame: M,
    text_frame: T,
}

impl<E, M, T> SceneGraphBuilder<'_, M, T>
where
    E: From<RenderError>,
    M: FnMut(MediaFrameRequest) -> Result<Option<Arc<VisualFrame>>, E>,
    T: FnMut(TextFrameRequest<'_>) -> Result<Arc<VisualFrame>, RenderError>,
{
    pub(super) fn build(
        timeline: &dyn TimelineView,
        time: TimelineTime,
        size: RenderSize,
        media_frame: M,
        text_frame: T,
    ) -> Result<RenderScene, E> {
        let mut builder = SceneGraphBuilder {
            timeline,
            size,
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
        ))
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
                    inputs,
                    properties: schema
                        .property_layout()
                        .pack("item", schema.id(), &item.properties)
                        .expect("validated plugin properties must match their schema"),
                    effects: Vec::new(),
                    render_scale,
                    output_bounds: BoundsOperation::from_schema(
                        &render.bounds,
                        &item.properties,
                        &Default::default(),
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
            RenderNodeContent::View { .. } | RenderNodeContent::Frame(_) => {
                unreachable!("timeline items are not input nodes")
            }
        }

        let id = self.push_node(content, owner.blend_mode);
        self.node_cache.insert(key, Some(id));
        Ok(Some(id))
    }

    fn scaled_size(&self, scale: u32) -> Result<RenderSize, RenderError> {
        self.size
            .checked_scale(scale)
            .ok_or_else(|| RenderError::resource_limit("render size overflows"))
    }

    fn content_scale(&self, content: &RenderNodeContent) -> u32 {
        let scale_for = |id: SceneNodeId| self.nodes[id].render_scale;
        let (scale, effects): (u32, &[RenderEffect]) = match content {
            RenderNodeContent::Item(item) => (
                item.inputs
                    .iter()
                    .map(|input| scale_for(input.node))
                    .fold(item.render_scale, u32::max),
                &item.effects,
            ),
            RenderNodeContent::Scene {
                children,
                effects,
                render_scale,
            } => (
                children
                    .iter()
                    .copied()
                    .map(scale_for)
                    .fold(*render_scale, u32::max),
                effects,
            ),
            RenderNodeContent::Frame(_) => return 1,
            RenderNodeContent::View { input, .. } => return scale_for(*input).max(1),
        };
        effects
            .iter()
            .flat_map(|effect| &effect.inputs)
            .map(|input| scale_for(input.node))
            .fold(scale, u32::max)
    }

    fn push_node(&mut self, content: RenderNodeContent, blend_mode: BlendMode) -> SceneNodeId {
        let render_scale = self.content_scale(&content);
        let id = self.nodes.len();
        self.nodes.push(RenderNode {
            content,
            render_scale,
            blend_mode,
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
            let schema = instance.schema();
            let properties = schema
                .property_layout()
                .pack("effect", schema.id(), &instance.properties)
                .expect("validated effect properties must match their schema");
            let mut passes = Vec::with_capacity(schema.render().passes.len());
            for (pass_index, pass) in schema.render().passes.iter().enumerate() {
                let kind = match pass {
                    EffectPassSchema::Render { .. } => RenderEffectPassKind::Render,
                    EffectPassSchema::Compute { dispatch, .. } => {
                        RenderEffectPassKind::Compute(*dispatch)
                    }
                    EffectPassSchema::Temporal { sampling, .. } => {
                        let offsets = sampling
                            .sample_offsets(&instance.properties)
                            .expect("validated temporal sampling properties");
                        RenderEffectPassKind::Temporal(self.temporal_samples(
                            &node.path,
                            effect_index,
                            time,
                            offsets,
                            depth,
                        )?)
                    }
                };
                passes.push(RenderEffectPass {
                    shader: EffectShaderId::plugin_pass(
                        &instance.plugin_id,
                        &instance.effect_id,
                        pass_index,
                    ),
                    properties: properties.clone(),
                    kind,
                    output_bounds: BoundsOperation::from_schema(
                        pass.bounds(),
                        &instance.properties,
                        pass.overrides(),
                    ),
                });
            }
            let inputs = self.render_inputs(
                InputSource::effect(node.item.id, instance),
                node,
                composition,
                time,
                self.scaled_size(scale)?,
                depth,
            )?;
            effects.push(RenderEffect { passes, inputs });
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
        offsets
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
    ) -> Result<Vec<RenderInput>, E> {
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
                            let input = MediaInputId {
                                item_id: source.item_id,
                                effect_id: source.effect_id,
                                input_id: capability.id().to_owned(),
                            };
                            let size = source.media_raster_size(
                                capability,
                                target_size,
                                RenderSize::from(self.timeline.resolution()),
                            );
                            let request = MediaFrameRequest::from_input(
                                &node.item,
                                input,
                                node.item.local_seconds(time, self.timeline.frame_rate()),
                                self.timeline.media_cache(),
                                RasterSize {
                                    max_width: size.width,
                                    max_height: size.height,
                                },
                            );
                            let frame = match request {
                                Some(request) => (self.media_frame)(request)?,
                                None => None,
                            };
                            entry.insert(frame.clone());
                            frame
                        }
                    }
                    .unwrap_or_else(|| Arc::new(VisualFrame::rgba(1, 1, [0u8; 4])));
                    self.frame_node(frame)
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
                    self.frame_node(frame)
                }

                TextureInput::RenderResult { .. } => {
                    let capture =
                        composition.capture(source.item_id, source.effect_id, capability.id());
                    let mut children = Vec::new();
                    for candidate in composition.captured_nodes(capture) {
                        if let Some(child) = self.render_node(&candidate.path, None, time, depth)? {
                            children.push(child);
                        }
                    }
                    let view = capture.view;
                    let input = self.push_node(
                        RenderNodeContent::Scene {
                            children,
                            effects: Vec::new(),
                            render_scale: 1,
                        },
                        BlendMode::Normal,
                    );
                    view.map_or(input, |view| {
                        self.push_node(RenderNodeContent::View { input, view }, BlendMode::Normal)
                    })
                }
            };
            inputs.push(RenderInput {
                id: capability.id().to_owned(),
                node: input,
            });
        }

        Ok(inputs)
    }

    fn frame_node(&mut self, frame: Arc<VisualFrame>) -> SceneNodeId {
        self.push_node(RenderNodeContent::Frame(frame), BlendMode::Normal)
    }
}
