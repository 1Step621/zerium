use super::*;

const MAX_TEMPORAL_DEPTH: usize = 4;
const MAX_TEMPORAL_RENDER_NODES: usize = 4_096;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RenderQuality {
    #[default]
    Full,
    Realtime {
        max_temporal_samples: usize,
    },
}

impl RenderQuality {
    fn temporal_offsets(self, offsets: Vec<f64>) -> Vec<f64> {
        let Self::Realtime {
            max_temporal_samples,
        } = self
        else {
            return offsets;
        };
        let limit = max_temporal_samples.max(1);
        if offsets.len() <= limit {
            return offsets;
        }
        let count = offsets.len();
        (0..limit)
            .map(|index| {
                let bucket_center = (2 * index + 1) * count;
                let source_index = (bucket_center / (2 * limit)).min(count - 1);
                offsets[source_index]
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RenderCacheKey {
    item_id: ItemId,
    path: Vec<ItemId>,
    effect_count: usize,
    time_bits: u64,
    render_scale: u32,
}

/// Values shared by every capability renderer, regardless of whether the
/// capability belongs to an item shader or an effect pass.
struct CapabilitySource<'a> {
    item_id: ItemId,
    effect_id: Option<EffectInstanceId>,
    plugin_id: &'a str,
    owner_kind: shader::PluginShaderOwnerKind,
    owner_id: &'a str,
    capabilities: &'a [Capability],
    properties: &'a crate::domain::property::PropertyValues,
    label: &'a str,
}

impl<'a> CapabilitySource<'a> {
    fn item(item: &'a TimelineItem) -> Self {
        let schema = item.schema().expect("renderable item has a schema");
        Self {
            item_id: item.id,
            effect_id: None,
            plugin_id: item.plugin_id().unwrap_or_default(),
            owner_kind: shader::PluginShaderOwnerKind::Item,
            owner_id: schema.id(),
            capabilities: schema.capabilities(),
            properties: &item.properties,
            label: schema.label(),
        }
    }

    fn effect(item_id: ItemId, effect: &'a EffectInstance) -> Self {
        let schema = effect.schema();
        Self {
            item_id,
            effect_id: Some(effect.id),
            plugin_id: &effect.plugin_id,
            owner_kind: shader::PluginShaderOwnerKind::Effect,
            owner_id: schema.id(),
            capabilities: schema.capabilities(),
            properties: &effect.properties,
            label: schema.label(),
        }
    }

    fn shader_id(&self, capability_id: &str) -> ItemShaderId {
        ItemShaderId::plugin_capability(
            self.plugin_id,
            self.owner_kind,
            self.owner_id,
            capability_id,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RenderSize {
    pub width: u32,
    pub height: u32,
}

impl RenderSize {
    pub(super) fn checked_scale(self, scale: u32) -> Option<Self> {
        Some(Self {
            width: self.width.checked_mul(scale)?,
            height: self.height.checked_mul(scale)?,
        })
    }
}

impl From<ProjectResolution> for RenderSize {
    fn from(resolution: ProjectResolution) -> Self {
        Self {
            width: resolution.width(),
            height: resolution.height(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RenderItemSource {
    Shader,
    Texture(Vec<Arc<RgbaFrame>>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderItem {
    pub shader: ItemShaderId,
    pub source: RenderItemSource,
    pub inputs: Vec<RenderNode>,
    pub properties: Vec<u8>,
    pub effects: Vec<RenderEffect>,
    pub target_size: RenderSize,
    pub render_scale: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderEffect {
    pub passes: Vec<RenderEffectPass>,
    pub inputs: Vec<RenderNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderTemporalSample {
    pub frame_offset: f32,
    pub time: TimelineTime,
    /// A missing node is an intentionally transparent sample at a clip boundary.
    pub input: Option<Box<RenderNode>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderEffectPass {
    pub shader: EffectShaderId,
    pub properties: Vec<u8>,
    pub kind: RenderEffectPassKind,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RenderEffectPassKind {
    Render,
    Compute([ComputeDispatchDimension; 3]),
    Temporal(Vec<RenderTemporalSample>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderNodeMetadata {
    pub layer: LayerId,
    pub clip_start: TimelineTime,
    pub clip_end: TimelineTime,
    pub path: Vec<ItemId>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RenderNodeContent {
    Item(RenderItem),
    Scene {
        children: Vec<RenderNode>,
        effects: Vec<RenderEffect>,
        render_scale: u32,
    },
}

/// A scene-linear compositing node. Scene-instance effects belong to the
/// `Scene` node and are therefore applied once after all children are blended.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderNode {
    pub metadata: RenderNodeMetadata,
    pub content: RenderNodeContent,
}

impl RenderNode {
    pub(crate) fn item(metadata: RenderNodeMetadata, item: RenderItem) -> Self {
        Self {
            metadata,
            content: RenderNodeContent::Item(item),
        }
    }

    pub(crate) fn scene(
        metadata: RenderNodeMetadata,
        children: Vec<RenderNode>,
        effects: Vec<RenderEffect>,
        render_scale: u32,
    ) -> Self {
        Self {
            metadata,
            content: RenderNodeContent::Scene {
                children,
                effects,
                render_scale,
            },
        }
    }

    pub(super) fn required_render_scale(&self) -> u32 {
        let effect_scale = |effects: &[RenderEffect]| {
            effects
                .iter()
                .flat_map(|effect| {
                    effect
                        .inputs
                        .iter()
                        .map(|input| input.required_render_scale())
                })
                .max()
                .unwrap_or(1)
        };
        match &self.content {
            RenderNodeContent::Item(item) => item
                .render_scale
                .max(
                    item.inputs
                        .iter()
                        .map(|input| input.required_render_scale())
                        .max()
                        .unwrap_or(1),
                )
                .max(effect_scale(&item.effects)),
            RenderNodeContent::Scene {
                children,
                effects,
                render_scale,
                ..
            } => (*render_scale).max(effect_scale(effects)).max(
                children
                    .iter()
                    .map(Self::required_render_scale)
                    .max()
                    .unwrap_or(1),
            ),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MediaFrameRequest<'a> {
    pub item_id: ItemId,
    pub effect_id: Option<EffectInstanceId>,
    pub input_id: &'a str,
    pub time: TimelineTime,
    pub target_size: RenderSize,
}

type MediaFrameCache =
    HashMap<(ItemId, Option<EffectInstanceId>, usize, u64), Option<Arc<RgbaFrame>>>;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderScene {
    pub size: RenderSize,
    pub composition_size: RenderSize,
    pub effect_size: RenderSize,
    pub background: [f64; 4],
    pub roots: Vec<RenderNode>,
}

impl RenderScene {
    pub(crate) fn render_size_for_item(
        item: &TimelineItem,
        size: RenderSize,
    ) -> Result<RenderSize, RenderError> {
        let render_scale = item
            .effects
            .iter()
            .map(|effect| effect.schema().render_scale())
            .max()
            .unwrap_or(1);
        size.checked_scale(render_scale).ok_or_else(|| {
            RenderError::resource_limit(format!(
                "item '{}' render size at scale {render_scale} overflows",
                item.intrinsic_label()
                    .unwrap_or_else(|| "unknown".to_owned())
            ))
        })
    }

    fn original_is_hidden(node: &EvaluatedSceneNode, scope: &[EvaluatedSceneNode]) -> bool {
        scope.iter().any(|render_result| {
            render_result.item().render_result_ranges().any(|settings| {
                settings.hide_original
                    && settings.includes(render_result.local_layer, node.local_layer)
            })
        })
    }

    fn is_hidden_by_nested_render_result(
        node: &EvaluatedSceneNode,
        scope: &[EvaluatedSceneNode],
        containing_range: RenderResultSettings,
        source_layer: LayerId,
    ) -> bool {
        scope.iter().any(|render_result| {
            render_result.local_layer != node.local_layer
                && containing_range.includes(source_layer, render_result.local_layer)
                && render_result.item().render_result_ranges().any(|settings| {
                    settings.hide_original
                        && settings.includes(render_result.local_layer, node.local_layer)
                })
        })
    }

    pub(super) fn render_items(&self) -> Vec<&RenderItem> {
        fn collect<'a>(nodes: &'a [RenderNode], output: &mut Vec<&'a RenderItem>) {
            for node in nodes {
                match &node.content {
                    RenderNodeContent::Item(item) => {
                        for input in &item.inputs {
                            collect(std::slice::from_ref(input), output);
                        }
                        collect_effect_inputs(&item.effects, output);
                        output.push(item);
                    }
                    RenderNodeContent::Scene {
                        children, effects, ..
                    } => {
                        collect_effect_inputs(effects, output);
                        collect(children, output);
                    }
                }
            }
        }

        fn collect_effect_inputs<'a>(
            effects: &'a [RenderEffect],
            output: &mut Vec<&'a RenderItem>,
        ) {
            for effect in effects {
                for input in &effect.inputs {
                    collect(std::slice::from_ref(input), output);
                }
            }
        }

        let mut output = Vec::new();
        collect(&self.roots, &mut output);
        output
    }

    pub(super) fn render_effects(&self) -> Vec<&RenderEffect> {
        fn collect<'a>(nodes: &'a [RenderNode], output: &mut Vec<&'a RenderEffect>) {
            for node in nodes {
                match &node.content {
                    RenderNodeContent::Item(item) => {
                        for input in &item.inputs {
                            collect(std::slice::from_ref(input), output);
                        }
                        collect_effect_inputs(&item.effects, output);
                        output.extend(&item.effects)
                    }
                    RenderNodeContent::Scene {
                        children, effects, ..
                    } => {
                        collect_effect_inputs(effects, output);
                        output.extend(effects);
                        collect(children, output);
                    }
                }
            }
        }

        fn collect_effect_inputs<'a>(
            effects: &'a [RenderEffect],
            output: &mut Vec<&'a RenderEffect>,
        ) {
            for effect in effects {
                for input in &effect.inputs {
                    collect(std::slice::from_ref(input), output);
                }
            }
        }

        let mut output = Vec::new();
        collect(&self.roots, &mut output);
        output
    }

    pub(crate) fn from_roots(
        size: RenderSize,
        composition_size: RenderSize,
        background: [f64; 4],
        roots: Vec<RenderNode>,
    ) -> Result<Self, RenderError> {
        let effect_scale = roots
            .iter()
            .map(RenderNode::required_render_scale)
            .max()
            .unwrap_or(1);
        let effect_size = size.checked_scale(effect_scale).ok_or_else(|| {
            RenderError::resource_limit("hierarchical scene effect size overflows")
        })?;
        Ok(Self {
            size,
            composition_size,
            effect_size,
            background,
            roots,
        })
    }

    pub(crate) fn from_timeline<E: From<RenderError>>(
        timeline: &dyn TimelineView,
        time: TimelineTime,
        size: RenderSize,
        quality: RenderQuality,
        mut media_frame: impl FnMut(MediaFrameRequest<'_>) -> Result<Option<Arc<RgbaFrame>>, E>,
        mut text_frame: impl FnMut(TextFrameRequest<'_>) -> Result<Arc<RgbaFrame>, RenderError>,
    ) -> Result<Self, E> {
        let graph = timeline.active_scene_graph_at_time(time);
        let mut graph_cache = HashMap::from([(time.frames().to_bits(), graph.clone())]);
        let mut render_cache = HashMap::new();
        let mut media_cache = HashMap::new();
        let mut temporal_nodes_remaining = MAX_TEMPORAL_RENDER_NODES;
        let mut roots = Vec::with_capacity(graph.len());
        for node in &graph {
            if Self::original_is_hidden(node, &graph) {
                continue;
            }
            if let Some(rendered) = Self::render_evaluated_node(
                node,
                &graph,
                None,
                timeline,
                time,
                size,
                quality,
                0,
                &mut temporal_nodes_remaining,
                &mut graph_cache,
                &mut render_cache,
                &mut media_cache,
                &mut media_frame,
                &mut text_frame,
            )? {
                roots.push(rendered);
            }
        }
        Ok(Self::from_roots(
            size,
            RenderSize::from(timeline.resolution()),
            [0.008, 0.006, 0.005, 1.],
            roots,
        )?)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_capabilities<E: From<RenderError>>(
        source: CapabilitySource<'_>,
        packed_properties: &[u8],
        node: &EvaluatedSceneNode,
        scope: &[EvaluatedSceneNode],
        timeline: &dyn TimelineView,
        time: TimelineTime,
        size: RenderSize,
        target_size: RenderSize,
        quality: RenderQuality,
        temporal_depth: usize,
        temporal_nodes_remaining: &mut usize,
        graph_cache: &mut HashMap<u64, Vec<EvaluatedSceneNode>>,
        render_cache: &mut HashMap<RenderCacheKey, Option<RenderItem>>,
        media_cache: &mut MediaFrameCache,
        media_frame: &mut impl FnMut(MediaFrameRequest<'_>) -> Result<Option<Arc<RgbaFrame>>, E>,
        text_frame: &mut impl FnMut(TextFrameRequest<'_>) -> Result<Arc<RgbaFrame>, RenderError>,
    ) -> Result<Vec<RenderNode>, E> {
        let metadata = RenderNodeMetadata {
            layer: node.layer,
            clip_start: TimelineTime::from_frame(node.clip.start),
            clip_end: TimelineTime::from_frame(node.clip.end_exclusive()),
            path: node.path.clone(),
        };
        let mut inputs = Vec::with_capacity(source.capabilities.len());
        for (index, capability) in source.capabilities.iter().enumerate() {
            let input = match capability {
                Capability::Shader { .. } => RenderNode::item(
                    metadata.clone(),
                    RenderItem {
                        shader: source.shader_id(capability.id()),
                        source: RenderItemSource::Shader,
                        inputs: Vec::new(),
                        properties: packed_properties.to_vec(),
                        effects: Vec::new(),
                        target_size,
                        render_scale: 1,
                    },
                ),
                Capability::Media { .. } => {
                    let item_id = source.item_id;
                    let effect_id = source.effect_id;
                    let key = (item_id, effect_id, index, time.frames().to_bits());
                    let frame = match media_cache.entry(key) {
                        std::collections::hash_map::Entry::Occupied(entry) => {
                            entry.into_mut().clone()
                        }
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            let frame = media_frame(MediaFrameRequest {
                                item_id,
                                effect_id,
                                input_id: capability.id(),
                                time,
                                target_size,
                            })?;
                            entry.insert(frame.clone()).clone()
                        }
                    };
                    let frame = frame.unwrap_or_else(|| {
                        Arc::new(RgbaFrame {
                            width: 1,
                            height: 1,
                            rgba: Arc::from([0u8; 4]),
                        })
                    });
                    Self::frame_capability_node(metadata.clone(), frame, target_size)
                }
                Capability::Text { .. } => {
                    let frame = text_frame(TextFrameRequest {
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
                    Self::frame_capability_node(metadata.clone(), frame, target_size)
                }
                Capability::RenderResult {
                    start_offset,
                    end_offset,
                    hide_original,
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
                    for candidate in scope {
                        if !settings.includes(node.local_layer, candidate.local_layer)
                            || Self::is_hidden_by_nested_render_result(
                                candidate,
                                scope,
                                settings,
                                node.local_layer,
                            )
                        {
                            continue;
                        }
                        if let Some(child) = Self::render_evaluated_node(
                            candidate,
                            scope,
                            None,
                            timeline,
                            time,
                            size,
                            quality,
                            temporal_depth,
                            temporal_nodes_remaining,
                            graph_cache,
                            render_cache,
                            media_cache,
                            media_frame,
                            text_frame,
                        )? {
                            children.push(child);
                        }
                    }
                    RenderNode::scene(metadata.clone(), children, Vec::new(), 1)
                }
            };
            inputs.push(input);
        }
        Ok(inputs)
    }

    fn frame_capability_node(
        metadata: RenderNodeMetadata,
        frame: Arc<RgbaFrame>,
        target_size: RenderSize,
    ) -> RenderNode {
        RenderNode::item(
            metadata,
            RenderItem {
                shader: ItemShaderId::host_capability_frame(),
                source: RenderItemSource::Texture(vec![frame]),
                inputs: Vec::new(),
                properties: Vec::new(),
                effects: Vec::new(),
                target_size,
                render_scale: 1,
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_evaluated_node<E: From<RenderError>>(
        node: &EvaluatedSceneNode,
        scope: &[EvaluatedSceneNode],
        scene_effect_count: Option<usize>,
        timeline: &dyn TimelineView,
        time: TimelineTime,
        size: RenderSize,
        quality: RenderQuality,
        temporal_depth: usize,
        temporal_nodes_remaining: &mut usize,
        graph_cache: &mut HashMap<u64, Vec<EvaluatedSceneNode>>,
        render_cache: &mut HashMap<RenderCacheKey, Option<RenderItem>>,
        media_cache: &mut MediaFrameCache,
        media_frame: &mut impl FnMut(MediaFrameRequest<'_>) -> Result<Option<Arc<RgbaFrame>>, E>,
        text_frame: &mut impl FnMut(TextFrameRequest<'_>) -> Result<Arc<RgbaFrame>, RenderError>,
    ) -> Result<Option<RenderNode>, E> {
        let metadata = RenderNodeMetadata {
            layer: node.layer,
            clip_start: TimelineTime::from_frame(node.clip.start),
            clip_end: TimelineTime::from_frame(node.clip.end_exclusive()),
            path: node.path.clone(),
        };
        let mut rendered = match &node.kind {
            EvaluatedSceneNodeKind::Item(item) => {
                let effect_count = scene_effect_count.unwrap_or(item.effects.len());
                let render_scale = item
                    .effects
                    .iter()
                    .take(effect_count)
                    .map(|effect| effect.schema().render_scale())
                    .max()
                    .unwrap_or(1);
                let Some(mut rendered_item) = Self::render_item(
                    item,
                    &node.path,
                    effect_count,
                    timeline,
                    time,
                    size,
                    render_scale,
                    quality,
                    temporal_depth,
                    temporal_nodes_remaining,
                    graph_cache,
                    render_cache,
                    media_cache,
                    media_frame,
                    text_frame,
                )?
                else {
                    return Ok(None);
                };
                let inputs = Self::render_capabilities(
                    CapabilitySource::item(item),
                    &rendered_item.properties,
                    node,
                    scope,
                    timeline,
                    time,
                    size,
                    rendered_item.target_size,
                    quality,
                    temporal_depth,
                    temporal_nodes_remaining,
                    graph_cache,
                    render_cache,
                    media_cache,
                    media_frame,
                    text_frame,
                )?;
                rendered_item.inputs = inputs;
                Ok(Some(RenderNode::item(metadata, rendered_item)))
            }
            EvaluatedSceneNodeKind::Scene {
                scene_id,
                instance,
                children,
            } => {
                debug_assert_eq!(instance.scene_id(), Some(*scene_id));
                Self::render_composite_node(
                    node,
                    instance,
                    children,
                    scene_effect_count.unwrap_or(instance.effects.len()),
                    timeline,
                    time,
                    size,
                    quality,
                    temporal_depth,
                    temporal_nodes_remaining,
                    graph_cache,
                    render_cache,
                    media_cache,
                    media_frame,
                    text_frame,
                )
            }
        }?;
        if let Some(rendered) = &mut rendered {
            let source_render_scale = rendered.required_render_scale();
            let effects = match &mut rendered.content {
                RenderNodeContent::Item(item) => &mut item.effects,
                RenderNodeContent::Scene { effects, .. } => effects,
            };
            for (effect, instance) in effects.iter_mut().zip(&node.item().effects) {
                let schema = instance.schema();
                let target_size = size.checked_scale(source_render_scale).ok_or_else(|| {
                    E::from(RenderError::resource_limit(format!(
                        "effect '{}' render size overflows",
                        schema.label()
                    )))
                })?;
                let properties = schema
                    .property_layout()
                    .pack(
                        "effect",
                        schema.id(),
                        schema.properties(),
                        &instance.properties,
                    )
                    .expect("effect properties come from a validated schema");
                effect.inputs = Self::render_capabilities(
                    CapabilitySource::effect(node.item().id, instance),
                    &properties,
                    node,
                    scope,
                    timeline,
                    time,
                    size,
                    target_size,
                    quality,
                    temporal_depth,
                    temporal_nodes_remaining,
                    graph_cache,
                    render_cache,
                    media_cache,
                    media_frame,
                    text_frame,
                )?;
            }
        }
        Ok(rendered)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_composite_node<E: From<RenderError>>(
        node: &EvaluatedSceneNode,
        instance: &TimelineItem,
        child_scope: &[EvaluatedSceneNode],
        effect_count: usize,
        timeline: &dyn TimelineView,
        time: TimelineTime,
        size: RenderSize,
        quality: RenderQuality,
        temporal_depth: usize,
        temporal_nodes_remaining: &mut usize,
        graph_cache: &mut HashMap<u64, Vec<EvaluatedSceneNode>>,
        render_cache: &mut HashMap<RenderCacheKey, Option<RenderItem>>,
        media_cache: &mut MediaFrameCache,
        media_frame: &mut impl FnMut(MediaFrameRequest<'_>) -> Result<Option<Arc<RgbaFrame>>, E>,
        text_frame: &mut impl FnMut(TextFrameRequest<'_>) -> Result<Arc<RgbaFrame>, RenderError>,
    ) -> Result<Option<RenderNode>, E> {
        let mut rendered_children = Vec::with_capacity(child_scope.len());
        for child in child_scope {
            if Self::original_is_hidden(child, child_scope) {
                continue;
            }
            if let Some(rendered) = Self::render_evaluated_node(
                child,
                child_scope,
                None,
                timeline,
                time,
                size,
                quality,
                temporal_depth,
                temporal_nodes_remaining,
                graph_cache,
                render_cache,
                media_cache,
                media_frame,
                text_frame,
            )? {
                rendered_children.push(rendered);
            }
        }
        let render_scale = instance
            .effects
            .iter()
            .take(effect_count)
            .map(|effect| effect.schema().render_scale())
            .max()
            .unwrap_or(1);
        let mut effects = Vec::with_capacity(effect_count);
        for (effect_index, effect) in instance.effects.iter().take(effect_count).enumerate() {
            let temporal_samples = effect
                .schema()
                .passes()
                .iter()
                .map(|pass| {
                    let Some(offsets) = pass.temporal_sample_offsets(&effect.properties) else {
                        return Ok(None);
                    };
                    if temporal_depth >= MAX_TEMPORAL_DEPTH {
                        return Err(E::from(RenderError::resource_limit(format!(
                            "temporal effect depth exceeds {MAX_TEMPORAL_DEPTH}"
                        ))));
                    }
                    quality
                        .temporal_offsets(offsets)
                        .into_iter()
                        .map(|offset| {
                            *temporal_nodes_remaining = temporal_nodes_remaining
                                .checked_sub(1)
                                .ok_or_else(|| {
                                    RenderError::resource_limit(format!(
                                        "temporal render graph exceeds {MAX_TEMPORAL_RENDER_NODES} nodes"
                                    ))
                                })?;
                            let sample_time = time.offset(offset);
                            let sampled = {
                                let graph = graph_cache
                                    .entry(sample_time.frames().to_bits())
                                    .or_insert_with(|| {
                                        timeline.active_scene_graph_at_time(sample_time)
                                    });
                                Self::find_evaluated_node(graph, &node.path)
                                    .map(|(sampled, scope)| (sampled.clone(), scope.to_vec()))
                            };
                            let input = sampled
                                .as_ref()
                                .map(|(sampled, scope)| {
                                    Self::render_evaluated_node(
                                        sampled,
                                        scope,
                                        Some(effect_index),
                                        timeline,
                                        sample_time,
                                        size,
                                        quality,
                                        temporal_depth + 1,
                                        temporal_nodes_remaining,
                                        graph_cache,
                                        render_cache,
                                        media_cache,
                                        media_frame,
                                        text_frame,
                                    )
                                })
                                .transpose()?
                                .flatten()
                                .map(Box::new);
                            Ok(RenderTemporalSample {
                                frame_offset: offset as f32,
                                time: sample_time,
                                input,
                            })
                        })
                        .collect::<Result<Vec<_>, E>>()
                        .map(Some)
                })
                .collect::<Result<Vec<_>, E>>()?;
            effects.push(Self::render_effect(effect, temporal_samples));
        }
        let metadata = RenderNodeMetadata {
            layer: node.layer,
            clip_start: TimelineTime::from_frame(node.clip.start),
            clip_end: TimelineTime::from_frame(node.clip.end_exclusive()),
            path: node.path.clone(),
        };
        Ok(Some(RenderNode::scene(
            metadata,
            rendered_children,
            effects,
            render_scale,
        )))
    }

    fn find_evaluated_node<'a>(
        nodes: &'a [EvaluatedSceneNode],
        path: &[ItemId],
    ) -> Option<(&'a EvaluatedSceneNode, &'a [EvaluatedSceneNode])> {
        for node in nodes {
            if node.path == path {
                return Some((node, nodes));
            }
            if let EvaluatedSceneNodeKind::Scene { children, .. } = &node.kind
                && let Some(found) = Self::find_evaluated_node(children, path)
            {
                return Some(found);
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn render_item<E: From<RenderError>>(
        item: &TimelineItem,
        path: &[ItemId],
        effect_count: usize,
        timeline: &dyn TimelineView,
        time: TimelineTime,
        size: RenderSize,
        render_scale: u32,
        quality: RenderQuality,
        temporal_depth: usize,
        temporal_nodes_remaining: &mut usize,
        graph_cache: &mut HashMap<u64, Vec<EvaluatedSceneNode>>,
        render_cache: &mut HashMap<RenderCacheKey, Option<RenderItem>>,
        media_cache: &mut MediaFrameCache,
        media_frame: &mut impl FnMut(MediaFrameRequest<'_>) -> Result<Option<Arc<RgbaFrame>>, E>,
        text_frame: &mut impl FnMut(TextFrameRequest<'_>) -> Result<Arc<RgbaFrame>, RenderError>,
    ) -> Result<Option<RenderItem>, E> {
        let cache_key = RenderCacheKey {
            item_id: item.id,
            path: path.to_vec(),
            effect_count,
            time_bits: time.frames().to_bits(),
            render_scale,
        };
        if let Some(cached) = render_cache.get(&cache_key) {
            return Ok(cached.clone());
        }
        let Some(schema) = item.schema() else {
            return Ok(None);
        };
        let Some(_shader) = schema.shader() else {
            return Ok(None);
        };
        let target_size = size.checked_scale(render_scale).ok_or_else(|| {
            let item_label = item
                .intrinsic_label()
                .unwrap_or_else(|| schema.label().to_owned());
            RenderError::resource_limit(format!(
                "item '{}' render size at scale {render_scale} overflows",
                item_label
            ))
        })?;
        let effects = item
            .effects
            .iter()
            .take(effect_count)
            .enumerate()
            .map(|(effect_index, effect)| {
                let temporal_samples = effect
                    .schema()
                    .passes()
                    .iter()
                    .map(|pass| {
                        Ok(
                            match pass.temporal_sample_offsets(&effect.properties) {
                                Some(offsets) => Some(
                                    quality
                                        .temporal_offsets(offsets)
                                        .into_iter()
                                        .map(|offset| {
                                            if temporal_depth >= MAX_TEMPORAL_DEPTH {
                                                return Err(E::from(
                                                    RenderError::resource_limit(format!(
                                                        "temporal effect depth exceeds {MAX_TEMPORAL_DEPTH}"
                                                    )),
                                                ));
                                            }
                                            let sample_time = time.offset(offset);
                                            let sampled = graph_cache
                                                .entry(sample_time.frames().to_bits())
                                                .or_insert_with(|| {
                                                    timeline.active_scene_graph_at_time(sample_time)
                                                });
                                            let sampled = Self::find_evaluated_node(sampled, path)
                                                .map(|(node, scope)| (node.clone(), scope.to_vec()));
                                            let input = sampled
                                                .as_ref()
                                                .map(|(sample, scope)| {
                                                    *temporal_nodes_remaining =
                                                        temporal_nodes_remaining
                                                            .checked_sub(1)
                                                            .ok_or_else(|| {
                                                                RenderError::resource_limit(format!(
                                                                    "temporal render graph exceeds {MAX_TEMPORAL_RENDER_NODES} nodes"
                                                                ))
                                                            })?;
                                                    Self::render_evaluated_node(
                                                        sample,
                                                        scope,
                                                        Some(effect_index),
                                                        timeline,
                                                        sample_time,
                                                        size,
                                                        quality,
                                                        temporal_depth + 1,
                                                        temporal_nodes_remaining,
                                                        graph_cache,
                                                        render_cache,
                                                        media_cache,
                                                        media_frame,
                                                        text_frame,
                                                    )
                                                })
                                                .transpose()
                                                .map(Option::flatten)?;
                                            Ok(RenderTemporalSample {
                                                frame_offset: offset as f32,
                                                time: sample_time,
                                                input: input.map(Box::new),
                                            })
                                        })
                                        .collect::<Result<Vec<_>, E>>()?,
                                ),
                                None => None,
                            },
                        )
                    })
                    .collect::<Result<Vec<_>, E>>()?;
                Ok(Self::render_effect(effect, temporal_samples))
            })
            .collect::<Result<Vec<_>, E>>()?;
        let properties = Self::pack_item_properties(item, schema);
        let source = RenderItemSource::Shader;
        let render_item = RenderItem {
            shader: ItemShaderId::plugin_item(
                item.plugin_id().unwrap_or_default(),
                item.item_id().unwrap_or_default(),
            ),
            source,
            inputs: Vec::new(),
            properties,
            effects,
            target_size,
            render_scale,
        };
        render_cache.insert(cache_key, Some(render_item.clone()));
        Ok(Some(render_item))
    }

    pub(crate) fn render_effect(
        effect: &EffectInstance,
        temporal_samples: Vec<Option<Vec<RenderTemporalSample>>>,
    ) -> RenderEffect {
        let schema = effect.schema();
        let properties = schema
            .property_layout()
            .pack(
                "effect",
                schema.id(),
                schema.properties(),
                &effect.properties,
            )
            .expect("timeline effect properties come from the validated schema");
        let passes = schema
            .passes()
            .iter()
            .zip(temporal_samples)
            .enumerate()
            .map(|(pass_index, (pass, temporal_samples))| {
                let pass_shader =
                    EffectShaderId::plugin_pass(&effect.plugin_id, &effect.effect_id, pass_index);
                let kind = match pass {
                    EffectPassSchema::Render { .. } => RenderEffectPassKind::Render,
                    EffectPassSchema::Compute { dispatch, .. } => {
                        RenderEffectPassKind::Compute(*dispatch)
                    }
                    EffectPassSchema::Temporal { .. } => RenderEffectPassKind::Temporal(
                        temporal_samples.expect("temporal passes have rendered samples"),
                    ),
                };
                RenderEffectPass {
                    shader: pass_shader,
                    properties: properties.clone(),
                    kind,
                }
            })
            .collect();
        RenderEffect {
            passes,
            inputs: Vec::new(),
        }
    }

    pub(super) fn pack_item_properties(item: &TimelineItem, schema: &ItemSchema) -> Vec<u8> {
        schema
            .property_layout()
            .pack("item", schema.id(), schema.properties(), &item.properties)
            .expect("validated plugin properties must match their schema")
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub(crate) enum RenderError {
    #[error("{0}")]
    ResourceLimit(String),
    #[error("{0}")]
    Backend(String),
}

impl RenderError {
    pub(crate) fn backend(message: impl Into<String>) -> Self {
        Self::Backend(message.into())
    }

    pub(super) fn resource_limit(message: impl Into<String>) -> Self {
        Self::ResourceLimit(message.into())
    }
}
