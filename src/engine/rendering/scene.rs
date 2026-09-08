use super::*;

const MAX_TEMPORAL_DEPTH: usize = 4;
const MAX_TEMPORAL_RENDER_NODES: usize = 4_096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct RenderCacheKey {
    item_id: ItemId,
    effect_count: usize,
    time_bits: u64,
    render_scale: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// Opaque, variable-sized parameters supplied to one item instance.
///
/// The schema packer is the only production constructor, keeping layout and
/// byte offsets private to the host-generated WGSL interface.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ItemParams {
    bytes: Vec<u8>,
}

impl ItemParams {
    pub(crate) fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
        }
    }

    pub(super) fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(super) fn len(&self) -> usize {
        self.bytes.len()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderShaderItem {
    pub shader: ItemShaderId,
    pub params: ItemParams,
    pub effects: Vec<RenderEffect>,
    pub target_size: RenderSize,
    pub render_scale: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderTextureItem {
    pub shader: TextureShaderId,
    /// Texture inputs are positional in completed WGSL. The plugin schema ID is
    /// retained only in `MediaFrameRequest` diagnostics and never becomes a WGSL symbol.
    pub frames: Vec<Arc<RgbaFrame>>,
    pub params: ItemParams,
    pub effects: Vec<RenderEffect>,
    pub target_size: RenderSize,
    pub render_scale: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RenderItem {
    Shader(RenderShaderItem),
    Texture(RenderTextureItem),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderEffect {
    pub passes: Vec<RenderEffectPass>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderTemporalSample {
    pub frame_offset: f32,
    pub time: TimelineTime,
    /// A missing node is an intentionally transparent sample at a clip boundary.
    pub input: Option<Box<RenderNode>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RenderEffectPass {
    Render {
        shader: EffectShaderId,
        params: EffectParams,
    },
    Compute {
        shader: EffectShaderId,
        params: EffectParams,
        dispatch: [ComputeDispatchDimension; 3],
    },
    Temporal {
        reducer: EffectShaderId,
        params: EffectParams,
        samples: Vec<RenderTemporalSample>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct EffectParams {
    bytes: Vec<u8>,
}

impl EffectParams {
    pub(super) fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
        }
    }

    pub(super) fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(super) fn len(&self) -> usize {
        self.bytes.len()
    }
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
        match &self.content {
            RenderNodeContent::Item(RenderItem::Shader(item)) => item.render_scale,
            RenderNodeContent::Item(RenderItem::Texture(item)) => item.render_scale,
            RenderNodeContent::Scene {
                children,
                render_scale,
                ..
            } => (*render_scale).max(
                children
                    .iter()
                    .map(Self::required_render_scale)
                    .max()
                    .unwrap_or(1),
            ),
        }
    }

    fn leaf(layer: LayerId, source: &TimelineItem, item: RenderItem) -> Self {
        Self {
            metadata: RenderNodeMetadata {
                layer,
                clip_start: TimelineTime::from_frame(source.start),
                clip_end: TimelineTime::from_frame(source.end_exclusive()),
                path: vec![source.id],
            },
            content: RenderNodeContent::Item(item),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MediaFrameRequest<'a> {
    pub item_id: ItemId,
    pub input_id: &'a str,
    pub time: TimelineTime,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderScene {
    pub size: RenderSize,
    pub composition_size: RenderSize,
    pub effect_size: RenderSize,
    pub background: [f64; 4],
    pub roots: Vec<RenderNode>,
}

impl RenderScene {
    pub(super) fn render_items(&self) -> Vec<&RenderItem> {
        fn collect<'a>(nodes: &'a [RenderNode], output: &mut Vec<&'a RenderItem>) {
            for node in nodes {
                match &node.content {
                    RenderNodeContent::Item(item) => output.push(item),
                    RenderNodeContent::Scene { children, .. } => collect(children, output),
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
                    RenderNodeContent::Item(RenderItem::Shader(item)) => {
                        output.extend(&item.effects)
                    }
                    RenderNodeContent::Item(RenderItem::Texture(item)) => {
                        output.extend(&item.effects)
                    }
                    RenderNodeContent::Scene {
                        children, effects, ..
                    } => {
                        output.extend(effects);
                        collect(children, output);
                    }
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

    pub(crate) fn from_timeline(
        timeline: &dyn TimelineView,
        time: TimelineTime,
        size: RenderSize,
        mut media_frame: impl FnMut(MediaFrameRequest<'_>) -> Option<Arc<RgbaFrame>>,
        mut text_frame: impl FnMut(
            &TimelineItem,
            &ItemSchema,
            RenderSize,
        ) -> Result<Arc<RgbaFrame>, RenderError>,
    ) -> Result<Self, RenderError> {
        let graph = timeline.active_scene_graph_at_time(time);
        let mut graph_cache = HashMap::from([(time.frames().to_bits(), graph.clone())]);
        let mut timeline_cache =
            HashMap::from([(time.frames().to_bits(), timeline.active_items_at_time(time))]);
        let mut render_cache = HashMap::new();
        let mut media_cache = HashMap::new();
        let mut temporal_nodes_remaining = MAX_TEMPORAL_RENDER_NODES;
        let mut roots = Vec::with_capacity(graph.len());
        for node in &graph {
            if let Some(rendered) = Self::render_evaluated_node(
                node,
                None,
                timeline,
                time,
                size,
                0,
                &mut temporal_nodes_remaining,
                &mut graph_cache,
                &mut timeline_cache,
                &mut render_cache,
                &mut media_cache,
                &mut media_frame,
                &mut text_frame,
            )? {
                roots.push(rendered);
            }
        }
        Self::from_roots(
            size,
            RenderSize::from(timeline.resolution()),
            [0.008, 0.006, 0.005, 1.],
            roots,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_evaluated_node(
        node: &EvaluatedSceneNode,
        scene_effect_count: Option<usize>,
        timeline: &dyn TimelineView,
        time: TimelineTime,
        size: RenderSize,
        temporal_depth: usize,
        temporal_nodes_remaining: &mut usize,
        graph_cache: &mut HashMap<u64, Vec<EvaluatedSceneNode>>,
        timeline_cache: &mut HashMap<u64, Vec<(LayerId, TimelineItem)>>,
        render_cache: &mut HashMap<RenderCacheKey, Option<RenderItem>>,
        media_cache: &mut HashMap<(ItemId, usize, u64), Option<Arc<RgbaFrame>>>,
        media_frame: &mut impl FnMut(MediaFrameRequest<'_>) -> Option<Arc<RgbaFrame>>,
        text_frame: &mut impl FnMut(
            &TimelineItem,
            &ItemSchema,
            RenderSize,
        ) -> Result<Arc<RgbaFrame>, RenderError>,
    ) -> Result<Option<RenderNode>, RenderError> {
        let metadata = RenderNodeMetadata {
            layer: node.layer,
            clip_start: TimelineTime::from_frame(node.clip.start),
            clip_end: TimelineTime::from_frame(node.clip.end_exclusive()),
            path: node.path.clone(),
        };
        match &node.kind {
            EvaluatedSceneNodeKind::Item(item) => {
                let render_scale = item
                    .effects
                    .iter()
                    .map(|effect| effect.schema().render_scale())
                    .max()
                    .unwrap_or(1);
                Self::render_item(
                    item,
                    item.effects.len(),
                    timeline,
                    time,
                    size,
                    render_scale,
                    temporal_depth,
                    temporal_nodes_remaining,
                    render_cache,
                    timeline_cache,
                    media_cache,
                    media_frame,
                    text_frame,
                )
                .map(|item| item.map(|item| RenderNode::item(metadata, item)))
            }
            EvaluatedSceneNodeKind::Scene {
                scene_id,
                instance,
                children,
            } => {
                debug_assert_eq!(instance.scene_id(), Some(*scene_id));
                let effect_count = scene_effect_count.unwrap_or(instance.effects.len());
                let mut rendered_children = Vec::with_capacity(children.len());
                for child in children {
                    if let Some(rendered) = Self::render_evaluated_node(
                        child,
                        None,
                        timeline,
                        time,
                        size,
                        temporal_depth,
                        temporal_nodes_remaining,
                        graph_cache,
                        timeline_cache,
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
                for (effect_index, effect) in instance.effects.iter().take(effect_count).enumerate()
                {
                    let temporal_samples = effect
                        .schema()
                        .passes()
                        .iter()
                        .map(|pass| {
                            let Some(offsets) = effect
                                .schema()
                                .temporal_sample_offsets(pass, &effect.parameters)
                            else {
                                return Ok(None);
                            };
                            if temporal_depth >= MAX_TEMPORAL_DEPTH {
                                return Err(RenderError::resource_limit(format!(
                                    "temporal effect depth exceeds {MAX_TEMPORAL_DEPTH}"
                                )));
                            }
                            offsets
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
                                        Self::find_evaluated_node(graph, &node.path).cloned()
                                    };
                                    let input = sampled
                                        .as_ref()
                                        .map(|sampled| {
                                            Self::render_evaluated_node(
                                                sampled,
                                                Some(effect_index),
                                                timeline,
                                                sample_time,
                                                size,
                                                temporal_depth + 1,
                                                temporal_nodes_remaining,
                                                graph_cache,
                                                timeline_cache,
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
                                .collect::<Result<Vec<_>, RenderError>>()
                                .map(Some)
                        })
                        .collect::<Result<Vec<_>, RenderError>>()?;
                    effects.push(Self::render_effect(effect, temporal_samples));
                }
                Ok(Some(RenderNode::scene(
                    metadata,
                    rendered_children,
                    effects,
                    render_scale,
                )))
            }
        }
    }

    fn find_evaluated_node<'a>(
        nodes: &'a [EvaluatedSceneNode],
        path: &[ItemId],
    ) -> Option<&'a EvaluatedSceneNode> {
        for node in nodes {
            if node.path == path {
                return Some(node);
            }
            if let EvaluatedSceneNodeKind::Scene { children, .. } = &node.kind
                && let Some(found) = Self::find_evaluated_node(children, path)
            {
                return Some(found);
            }
        }
        None
    }

    pub(crate) fn effect_render_size_for_timeline(
        timeline: &dyn TimelineView,
        time: TimelineTime,
        size: RenderSize,
    ) -> Result<RenderSize, RenderError> {
        fn max_scale(nodes: &[EvaluatedSceneNode]) -> u32 {
            nodes
                .iter()
                .map(|node| {
                    node.item()
                        .effects
                        .iter()
                        .map(|effect| effect.schema().render_scale())
                        .max()
                        .unwrap_or(1)
                        .max(max_scale(node.children()))
                })
                .max()
                .unwrap_or(1)
        }

        let scale = max_scale(&timeline.active_scene_graph_at_time(time));
        size.checked_scale(scale).ok_or_else(|| {
            RenderError::resource_limit(format!(
                "effect render size {}x{} at scale {scale} overflows",
                size.width, size.height
            ))
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn render_item(
        item: &TimelineItem,
        effect_count: usize,
        timeline: &dyn TimelineView,
        time: TimelineTime,
        size: RenderSize,
        render_scale: u32,
        temporal_depth: usize,
        temporal_nodes_remaining: &mut usize,
        render_cache: &mut HashMap<RenderCacheKey, Option<RenderItem>>,
        timeline_cache: &mut HashMap<u64, Vec<(LayerId, TimelineItem)>>,
        media_cache: &mut HashMap<(ItemId, usize, u64), Option<Arc<RgbaFrame>>>,
        media_frame: &mut impl FnMut(MediaFrameRequest<'_>) -> Option<Arc<RgbaFrame>>,
        text_frame: &mut impl FnMut(
            &TimelineItem,
            &ItemSchema,
            RenderSize,
        ) -> Result<Arc<RgbaFrame>, RenderError>,
    ) -> Result<Option<RenderItem>, RenderError> {
        let cache_key = RenderCacheKey {
            item_id: item.id,
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
        let Some(visual) = schema.visual() else {
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
                            match effect
                                .schema()
                                .temporal_sample_offsets(pass, &effect.parameters)
                            {
                                Some(offsets) => Some(
                                    offsets
                                        .into_iter()
                                        .map(|offset| {
                                            if temporal_depth >= MAX_TEMPORAL_DEPTH {
                                                return Err(RenderError::resource_limit(format!(
                                                    "temporal effect depth exceeds {MAX_TEMPORAL_DEPTH}"
                                                )));
                                            }
                                            let sample_time = time.offset(offset);
                                            let sample = timeline_cache
                                                .entry(sample_time.frames().to_bits())
                                                .or_insert_with(|| {
                                                    timeline.active_items_at_time(sample_time)
                                                })
                                                .iter()
                                                .find_map(|(_, candidate)| {
                                                    (candidate.id == item.id)
                                                        .then(|| candidate.clone())
                                                });
                                            let input = sample
                                                .as_ref()
                                                .map(|sample| {
                                                    *temporal_nodes_remaining =
                                                        temporal_nodes_remaining
                                                            .checked_sub(1)
                                                            .ok_or_else(|| {
                                                                RenderError::resource_limit(format!(
                                                                    "temporal render graph exceeds {MAX_TEMPORAL_RENDER_NODES} nodes"
                                                                ))
                                                            })?;
                                                    Self::render_item(
                                                        sample,
                                                        effect_index,
                                                        timeline,
                                                        sample_time,
                                                        size,
                                                        render_scale,
                                                        temporal_depth + 1,
                                                        temporal_nodes_remaining,
                                                        render_cache,
                                                        timeline_cache,
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
                                                input: input.map(|input| {
                                                    Box::new(RenderNode::leaf(
                                                        LayerId::new(0),
                                                        item,
                                                        input,
                                                    ))
                                                }),
                                            })
                                        })
                                        .collect::<Result<Vec<_>, RenderError>>()?,
                                ),
                                None => None,
                            },
                        )
                    })
                    .collect::<Result<Vec<_>, RenderError>>()?;
                Ok(Self::render_effect(effect, temporal_samples))
            })
            .collect::<Result<Vec<_>, RenderError>>()?;
        let params = Self::pack_item_params(item, schema);
        let render_item = match visual {
            VisualCapability::Procedural { .. } => RenderItem::Shader(RenderShaderItem {
                shader: ItemShaderId::new(format!(
                    "{}::item::{}",
                    item.plugin_id().unwrap_or_default(),
                    item.item_id().unwrap_or_default()
                )),
                params,
                effects,
                target_size,
                render_scale,
            }),
            VisualCapability::Media { .. } => {
                let frames = schema
                    .texture_inputs()
                    .enumerate()
                    .map(|(input_slot, input)| {
                        let key = (item.id, input_slot, time.frames().to_bits());
                        media_cache
                            .entry(key)
                            .or_insert_with(|| {
                                media_frame(MediaFrameRequest {
                                    item_id: item.id,
                                    input_id: input.id(),
                                    time,
                                })
                            })
                            .clone()
                    })
                    .collect::<Option<Vec<_>>>();
                let Some(frames) = frames.filter(|frames| !frames.is_empty()) else {
                    return Ok(None);
                };
                RenderItem::Texture(RenderTextureItem {
                    shader: TextureShaderId::new(format!(
                        "{}::item::{}",
                        item.plugin_id().unwrap_or_default(),
                        item.item_id().unwrap_or_default()
                    )),
                    frames,
                    params,
                    effects,
                    target_size,
                    render_scale,
                })
            }
            VisualCapability::Text { .. } => RenderItem::Texture(RenderTextureItem {
                shader: TextureShaderId::new(format!(
                    "{}::item::{}",
                    item.plugin_id().unwrap_or_default(),
                    item.item_id().unwrap_or_default()
                )),
                frames: vec![text_frame(item, schema, target_size)?],
                params,
                effects,
                target_size,
                render_scale,
            }),
        };
        render_cache.insert(cache_key, Some(render_item.clone()));
        Ok(Some(render_item))
    }

    pub(crate) fn render_effect(
        effect: &EffectInstance,
        temporal_samples: Vec<Option<Vec<RenderTemporalSample>>>,
    ) -> RenderEffect {
        let schema = effect.schema();
        let passes = schema
            .pack_pass_parameters(&effect.parameters)
            .expect("timeline effect parameters come from the validated schema")
            .into_iter()
            .zip(schema.passes())
            .zip(temporal_samples)
            .enumerate()
            .map(|(pass_index, ((bytes, pass), temporal_samples))| {
                let pass_shader = EffectShaderId::new(format!(
                    "{}::effect::{}::pass::{pass_index}",
                    effect.plugin_id, effect.effect_id
                ));
                let params = EffectParams::from_bytes(bytes);
                match pass {
                    EffectPassSchema::Render { .. } => RenderEffectPass::Render {
                        shader: pass_shader,
                        params,
                    },
                    EffectPassSchema::Compute { dispatch, .. } => RenderEffectPass::Compute {
                        shader: pass_shader,
                        params,
                        dispatch: *dispatch,
                    },
                    EffectPassSchema::Temporal { .. } => RenderEffectPass::Temporal {
                        reducer: pass_shader,
                        params,
                        samples: temporal_samples.expect("temporal passes have rendered samples"),
                    },
                }
            })
            .collect();
        RenderEffect { passes }
    }

    pub(super) fn pack_item_params(item: &TimelineItem, schema: &ItemSchema) -> ItemParams {
        let bytes = schema
            .pack_parameter_values(&item.parameters)
            .expect("validated plugin parameters must match their schema");
        ItemParams::from_bytes(bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenderError {
    ResourceLimit(String),
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

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ResourceLimit(message) | Self::Backend(message) => message,
        })
    }
}

impl Error for RenderError {}
