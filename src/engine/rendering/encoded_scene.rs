use super::scene::RenderTextureInput;
use super::*;

pub(super) type RenderNodeId = usize;

const SHARED_NODE_CACHE_BUDGET: usize = 128 * 1024 * 1024;
const MAX_SHARED_NODE_CACHE_ENTRIES: usize = 8;
const SCENE_BYTES_PER_PIXEL: usize = 8;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuItem {
    pub(super) property_offset: u32,
    pub(super) property_size: u32,
    pub(super) output_size: [f32; 2],
    pub(super) composition_size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuEffect {
    pub(super) property_offset: u32,
    pub(super) property_size: u32,
    pub(super) sample_index: u32,
    pub(super) sample_count: u32,
    pub(super) frame_offset: f32,
    pub(super) exposure_progress: f32,
    pub(super) composition_size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuTextureInput {
    pub(super) size: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuCompute {
    pub(super) property_offset: u32,
    pub(super) property_size: u32,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) composition_size: [f32; 2],
    pub(super) padding: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuComposite {
    pub(super) input_size: [u32; 2],
    pub(super) output_size: [u32; 2],
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ItemBatch {
    pub(super) shader: ItemShaderId,
    pub(super) instances: Range<u32>,
}

#[derive(Debug, PartialEq)]
pub(super) enum RenderCommand {
    Items(ItemBatch),
    Texture {
        index: usize,
        shader: ItemShaderId,
    },
    Effected {
        node: RenderNodeId,
        render_scale: u32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum RenderSourceCommand {
    Transparent,
    Item {
        shader: ItemShaderId,
        instance: u32,
    },
    Texture {
        index: usize,
        shader: ItemShaderId,
    },
    RenderedTexture {
        input: RenderNodeId,
        index: usize,
        shader: ItemShaderId,
    },
}

#[derive(Debug, PartialEq)]
pub(super) struct RenderNodeCommand {
    pub(super) metadata: RenderNodeMetadata,
    pub(super) kind: RenderNodeCommandKind,
}

#[derive(Debug, PartialEq)]
pub(super) enum RenderNodeCommandKind {
    Source(RenderSourceCommand),
    Composite {
        children: Vec<RenderNodeId>,
    },
    Effect {
        input: RenderNodeId,
        passes: Vec<EffectPassCommand>,
    },
    TemporalEffect {
        // Every branch includes the effects preceding this temporal effect.
        samples: Vec<(RenderNodeId, TemporalReduceCommand)>,
    },
}

impl RenderNodeCommand {
    pub(super) fn depths(&self, nodes: &[Self]) -> (usize, usize) {
        match &self.kind {
            RenderNodeCommandKind::Source(RenderSourceCommand::RenderedTexture {
                input, ..
            }) => nodes[*input].depths(nodes),
            RenderNodeCommandKind::Source(_) => (0, 0),
            RenderNodeCommandKind::Effect { input, .. } => nodes[*input].depths(nodes),
            RenderNodeCommandKind::Composite { children } => {
                let (temporal, composition) =
                    max_depths(children.iter().map(|child| nodes[*child].depths(nodes)));
                (temporal, composition + 1)
            }
            RenderNodeCommandKind::TemporalEffect { samples } => {
                let (temporal, composition) = max_depths(
                    samples
                        .iter()
                        .map(|(sample, _)| nodes[*sample].depths(nodes)),
                );
                (temporal + 1, composition)
            }
        }
    }
}

fn max_depths(depths: impl Iterator<Item = (usize, usize)>) -> (usize, usize) {
    depths.fold(
        (0, 0),
        |(temporal, composition), (next_temporal, next_composition)| {
            (
                temporal.max(next_temporal),
                composition.max(next_composition),
            )
        },
    )
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct EffectPassCommand {
    pub(super) shader: EffectShaderId,
    pub(super) instance: u32,
    pub(super) kind: EffectPassCommandKind,
    pub(super) captures_source: bool,
    pub(super) property_offset: u32,
    pub(super) property_size: u32,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(super) enum EffectPassCommandKind {
    Render,
    Compute {
        dispatch: [ComputeDispatchDimension; 3],
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct TemporalReduceCommand {
    pub(super) reducer: EffectShaderId,
    pub(super) instance: u32,
}

pub(super) struct EncodedScene {
    pub(super) items: Vec<GpuItem>,
    pub(super) properties: Vec<u8>,
    pub(super) effects: Vec<GpuEffect>,
    pub(super) effect_properties: Vec<u8>,
    pub(super) textures: Vec<EncodedTexture>,
    pub(super) nodes: Vec<RenderNodeCommand>,
    pub(super) node_keys: Vec<Arc<RenderNodeKey>>,
    pub(super) shared_node_slots: Vec<Option<usize>>,
    pub(super) commands: Vec<RenderCommand>,
}

impl EncodedScene {
    pub(super) fn depths(&self) -> (usize, usize) {
        max_depths(self.commands.iter().filter_map(|command| match command {
            RenderCommand::Effected { node, .. } => Some(self.nodes[*node].depths(&self.nodes)),
            RenderCommand::Items(_) | RenderCommand::Texture { .. } => None,
        }))
    }
}

pub(super) struct EncodedTexture {
    pub(super) shader: ItemShaderId,
    pub(super) input: EncodedTextureInput,
    pub(super) properties: Vec<u8>,
    pub(super) target_size: RenderSize,
    pub(super) composition_size: RenderSize,
}

pub(super) enum EncodedTextureInput {
    Frames(Vec<Arc<RgbaFrame>>),
    Rendered,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum SourceKey {
    Transparent,
    Item {
        shader: ItemShaderId,
        properties: Vec<u8>,
        target_size: RenderSize,
        render_scale: u32,
    },
    Texture {
        shader: ItemShaderId,
        frames: Vec<usize>,
        properties: Vec<u8>,
        target_size: RenderSize,
        render_scale: u32,
    },
    RenderedTexture {
        shader: ItemShaderId,
        input: Arc<RenderNodeKey>,
        properties: Vec<u8>,
        target_size: RenderSize,
        render_scale: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct EffectPassKey {
    shader: EffectShaderId,
    properties: Vec<u8>,
    kind: EffectPassCommandKind,
    captures_source: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum RenderNodeKey {
    Source(SourceKey),
    Composite(Vec<Arc<RenderNodeKey>>),
    Effect {
        input: Arc<RenderNodeKey>,
        passes: Vec<EffectPassKey>,
    },
    Temporal {
        reducer: EffectShaderId,
        properties: Vec<u8>,
        samples: Vec<(Arc<RenderNodeKey>, u32)>,
    },
}

fn encode_effect_pass(
    shader: &EffectShaderId,
    properties: &[u8],
    kind: EffectPassCommandKind,
    captures_source: bool,
    effects: &mut Vec<GpuEffect>,
    effect_properties: &mut Vec<u8>,
    composition_size: RenderSize,
) -> Result<EffectPassCommand, RenderError> {
    let effect_instance = u32::try_from(effects.len())
        .map_err(|_| RenderError::backend("too many effect passes in one frame"))?;
    let property_offset = u32::try_from(effect_properties.len() / PROPERTY_WORD_SIZE)
        .map_err(|_| RenderError::backend("effect property offset exceeds u32"))?;
    let property_size = u32::try_from(properties.len())
        .map_err(|_| RenderError::backend("effect property size exceeds u32"))?;
    effect_properties.extend_from_slice(properties);
    effect_properties.resize(
        effect_properties.len().next_multiple_of(PROPERTY_WORD_SIZE),
        0,
    );
    effects.push(GpuEffect {
        property_offset,
        property_size,
        sample_index: 0,
        sample_count: 0,
        frame_offset: 0.,
        exposure_progress: 0.,
        composition_size: [
            composition_size.width as f32,
            composition_size.height as f32,
        ],
    });
    Ok(EffectPassCommand {
        shader: shader.clone(),
        instance: effect_instance,
        kind,
        captures_source,
        property_offset,
        property_size,
    })
}

struct EncodeContext<'a> {
    items: &'a mut Vec<GpuItem>,
    properties: &'a mut Vec<u8>,
    effects: &'a mut Vec<GpuEffect>,
    effect_properties: &'a mut Vec<u8>,
    textures: &'a mut Vec<EncodedTexture>,
    nodes: &'a mut Vec<RenderNodeCommand>,
    node_keys: &'a mut Vec<Arc<RenderNodeKey>>,
    node_cache: &'a mut HashMap<Arc<RenderNodeKey>, RenderNodeId>,
    source_cache: &'a mut HashMap<SourceKey, RenderSourceCommand>,
    composition_size: RenderSize,
}

impl EncodeContext<'_> {
    fn intern_node(
        &mut self,
        key: RenderNodeKey,
        metadata: RenderNodeMetadata,
        kind: RenderNodeCommandKind,
    ) -> RenderNodeId {
        let key = Arc::new(key);
        if let Some(id) = self.node_cache.get(&key) {
            return *id;
        }
        let id = self.nodes.len();
        self.nodes.push(RenderNodeCommand { metadata, kind });
        self.node_keys.push(key.clone());
        self.node_cache.insert(key, id);
        id
    }

    fn encode_source(
        &mut self,
        item: &RenderItem,
    ) -> Result<(RenderSourceCommand, SourceKey), RenderError> {
        match &item.source {
            RenderItemSource::Shader => {
                let key = SourceKey::Item {
                    shader: item.shader.clone(),
                    properties: item.properties.clone(),
                    target_size: item.target_size,
                    render_scale: item.render_scale,
                };
                if let Some(source) = self.source_cache.get(&key) {
                    return Ok((source.clone(), key));
                }
                let instance = u32::try_from(self.items.len())
                    .map_err(|_| RenderError::backend("too many visible items in one frame"))?;
                let property_offset = u32::try_from(self.properties.len() / PROPERTY_WORD_SIZE)
                    .map_err(|_| RenderError::backend("item property offset exceeds u32"))?;
                let property_size = u32::try_from(item.properties.len())
                    .map_err(|_| RenderError::backend("item property size exceeds u32"))?;
                self.properties.extend_from_slice(&item.properties);
                self.properties.resize(
                    self.properties.len().next_multiple_of(PROPERTY_WORD_SIZE),
                    0,
                );
                self.items.push(GpuItem {
                    property_offset,
                    property_size,
                    output_size: [
                        item.target_size.width as f32,
                        item.target_size.height as f32,
                    ],
                    composition_size: [
                        self.composition_size.width as f32,
                        self.composition_size.height as f32,
                    ],
                });
                let source = RenderSourceCommand::Item {
                    shader: item.shader.clone(),
                    instance,
                };
                self.source_cache.insert(key.clone(), source.clone());
                Ok((source, key))
            }
            RenderItemSource::Texture(input) => {
                let (input, render_input, frame_ids) = match input {
                    RenderTextureInput::Rendered(input) => (
                        EncodedTextureInput::Rendered,
                        Some(self.encode_node(input)?),
                        Vec::new(),
                    ),
                    RenderTextureInput::Frames(frames) => (
                        EncodedTextureInput::Frames(frames.clone()),
                        None,
                        frames
                            .iter()
                            .map(|frame| Arc::as_ptr(frame) as usize)
                            .collect(),
                    ),
                };
                let key = match render_input {
                    Some(input) => SourceKey::RenderedTexture {
                        shader: item.shader.clone(),
                        input: self.node_keys[input].clone(),
                        properties: item.properties.clone(),
                        target_size: item.target_size,
                        render_scale: item.render_scale,
                    },
                    None => SourceKey::Texture {
                        shader: item.shader.clone(),
                        frames: frame_ids,
                        properties: item.properties.clone(),
                        target_size: item.target_size,
                        render_scale: item.render_scale,
                    },
                };
                if let Some(source) = self.source_cache.get(&key) {
                    return Ok((source.clone(), key));
                }
                let index = self.textures.len();
                self.textures.push(EncodedTexture {
                    shader: item.shader.clone(),
                    input,
                    properties: item.properties.clone(),
                    target_size: item.target_size,
                    composition_size: self.composition_size,
                });
                let source = match render_input {
                    Some(input) => RenderSourceCommand::RenderedTexture {
                        input,
                        index,
                        shader: item.shader.clone(),
                    },
                    None => RenderSourceCommand::Texture {
                        index,
                        shader: item.shader.clone(),
                    },
                };
                self.source_cache.insert(key.clone(), source.clone());
                Ok((source, key))
            }
        }
    }

    fn temporal_reduce_command(
        &mut self,
        reducer: &EffectShaderId,
        properties: &[u8],
        sample_count: usize,
        sample_index: usize,
        sample: &RenderTemporalSample,
    ) -> Result<TemporalReduceCommand, RenderError> {
        let instance = u32::try_from(self.effects.len())
            .map_err(|_| RenderError::backend("too many effect passes in one frame"))?;
        let property_offset = u32::try_from(self.effect_properties.len() / PROPERTY_WORD_SIZE)
            .map_err(|_| RenderError::backend("effect property offset exceeds u32"))?;
        self.effect_properties.extend_from_slice(properties);
        self.effect_properties.resize(
            self.effect_properties
                .len()
                .next_multiple_of(PROPERTY_WORD_SIZE),
            0,
        );
        let property_size = u32::try_from(properties.len())
            .map_err(|_| RenderError::backend("effect property size exceeds u32"))?;
        let sample_index = u32::try_from(sample_index)
            .map_err(|_| RenderError::backend("temporal sample index exceeds u32"))?;
        let sample_count = u32::try_from(sample_count)
            .map_err(|_| RenderError::backend("temporal sample count exceeds u32"))?;
        self.effects.push(GpuEffect {
            property_offset,
            property_size,
            sample_index,
            sample_count,
            frame_offset: sample.frame_offset,
            exposure_progress: (sample_index as f32 + 0.5) / sample_count.max(1) as f32,
            composition_size: [
                self.composition_size.width as f32,
                self.composition_size.height as f32,
            ],
        });
        Ok(TemporalReduceCommand {
            reducer: reducer.clone(),
            instance,
        })
    }

    fn encode_effects(
        &mut self,
        mut node: RenderNodeId,
        effects: &[RenderEffect],
    ) -> Result<RenderNodeId, RenderError> {
        for effect in effects {
            let mut regular_passes = Vec::new();
            let mut regular_keys = Vec::new();
            for pass in &effect.passes {
                let kind = match &pass.kind {
                    RenderEffectPassKind::Temporal(samples) => {
                        debug_assert!(regular_passes.is_empty());
                        let sample_count = samples.len();
                        let encoded_samples = samples
                            .iter()
                            .enumerate()
                            .map(|(sample_index, sample)| {
                                let node = match &sample.input {
                                    Some(sample) => self.encode_node(sample)?,
                                    None => self.intern_node(
                                        RenderNodeKey::Source(SourceKey::Transparent),
                                        self.nodes[node].metadata.clone(),
                                        RenderNodeCommandKind::Source(
                                            RenderSourceCommand::Transparent,
                                        ),
                                    ),
                                };
                                Ok((
                                    node,
                                    self.temporal_reduce_command(
                                        &pass.shader,
                                        &pass.properties,
                                        sample_count,
                                        sample_index,
                                        sample,
                                    )?,
                                    sample.frame_offset.to_bits(),
                                ))
                            })
                            .collect::<Result<Vec<_>, RenderError>>()?;
                        let key = RenderNodeKey::Temporal {
                            reducer: pass.shader.clone(),
                            properties: pass.properties.clone(),
                            samples: encoded_samples
                                .iter()
                                .map(|(sample, _, offset)| {
                                    (self.node_keys[*sample].clone(), *offset)
                                })
                                .collect(),
                        };
                        let samples = encoded_samples
                            .into_iter()
                            .map(|(sample, reduce, _)| (sample, reduce))
                            .collect();
                        node = self.intern_node(
                            key,
                            self.nodes[node].metadata.clone(),
                            RenderNodeCommandKind::TemporalEffect { samples },
                        );
                        continue;
                    }
                    RenderEffectPassKind::Render => EffectPassCommandKind::Render,
                    RenderEffectPassKind::Compute(dispatch) => EffectPassCommandKind::Compute {
                        dispatch: *dispatch,
                    },
                };
                let starts_regular_chain = regular_passes.is_empty();
                regular_passes.push(encode_effect_pass(
                    &pass.shader,
                    &pass.properties,
                    kind,
                    starts_regular_chain,
                    self.effects,
                    self.effect_properties,
                    self.composition_size,
                )?);
                regular_keys.push(EffectPassKey {
                    shader: pass.shader.clone(),
                    properties: pass.properties.clone(),
                    kind,
                    captures_source: starts_regular_chain,
                });
            }
            if !regular_passes.is_empty() {
                node = self.intern_node(
                    RenderNodeKey::Effect {
                        input: self.node_keys[node].clone(),
                        passes: regular_keys,
                    },
                    self.nodes[node].metadata.clone(),
                    RenderNodeCommandKind::Effect {
                        input: node,
                        passes: regular_passes,
                    },
                );
            }
        }
        Ok(node)
    }

    fn encode_node(&mut self, node: &RenderNode) -> Result<RenderNodeId, RenderError> {
        match &node.content {
            RenderNodeContent::Item(item) => {
                let (source, key) = self.encode_source(item)?;
                let source = self.intern_node(
                    RenderNodeKey::Source(key),
                    node.metadata.clone(),
                    RenderNodeCommandKind::Source(source),
                );
                self.encode_effects(source, &item.effects)
            }
            RenderNodeContent::Scene {
                children, effects, ..
            } => {
                let children = children
                    .iter()
                    .map(|child| self.encode_node(child))
                    .collect::<Result<Vec<_>, _>>()?;
                let composite = self.intern_node(
                    RenderNodeKey::Composite(
                        children
                            .iter()
                            .map(|child| self.node_keys[*child].clone())
                            .collect(),
                    ),
                    node.metadata.clone(),
                    RenderNodeCommandKind::Composite { children },
                );
                self.encode_effects(composite, effects)
            }
        }
    }
}

pub(super) fn encode_items(scene: &RenderScene) -> Result<EncodedScene, RenderError> {
    let mut items = Vec::new();
    let mut properties = Vec::new();
    let mut effects = Vec::new();
    let mut effect_properties = Vec::new();
    let mut textures = Vec::new();
    let mut nodes = Vec::new();
    let mut node_keys = Vec::new();
    let mut node_cache = HashMap::new();
    let mut source_cache = HashMap::new();
    let mut commands: Vec<RenderCommand> = Vec::new();

    for node in &scene.roots {
        let (has_effects, item) = match &node.content {
            RenderNodeContent::Item(item) => (
                !item.effects.is_empty()
                    || matches!(
                        &item.source,
                        RenderItemSource::Texture(RenderTextureInput::Rendered(_))
                    ),
                Some(item.clone()),
            ),
            RenderNodeContent::Scene { effects, .. } => (!effects.is_empty(), None),
        };
        let mut context = EncodeContext {
            items: &mut items,
            properties: &mut properties,
            effects: &mut effects,
            effect_properties: &mut effect_properties,
            textures: &mut textures,
            nodes: &mut nodes,
            node_keys: &mut node_keys,
            node_cache: &mut node_cache,
            source_cache: &mut source_cache,
            composition_size: scene.composition_size,
        };
        if has_effects || matches!(node.content, RenderNodeContent::Scene { .. }) {
            commands.push(RenderCommand::Effected {
                node: context.encode_node(node)?,
                render_scale: node.required_render_scale(),
            });
            continue;
        }
        let item = item.as_ref().expect("non-scene nodes contain an item");
        match context.encode_source(item)?.0 {
            RenderSourceCommand::Item { shader, instance } => match commands.last_mut() {
                Some(RenderCommand::Items(batch))
                    if batch.shader == shader && batch.instances.end == instance =>
                {
                    batch.instances.end = instance + 1;
                }
                _ => commands.push(RenderCommand::Items(ItemBatch {
                    shader,
                    instances: instance..instance + 1,
                })),
            },
            RenderSourceCommand::Texture { index, shader } => {
                commands.push(RenderCommand::Texture { index, shader });
            }
            RenderSourceCommand::RenderedTexture { .. } => {
                unreachable!("render-result sources are encoded as nested commands")
            }
            RenderSourceCommand::Transparent => unreachable!("scene items are never transparent"),
        }
    }

    let shared_node_slots = shared_node_slots(
        &nodes,
        &commands,
        shared_node_cache_capacity(scene.effect_size),
    );
    Ok(EncodedScene {
        items,
        properties,
        effects,
        effect_properties,
        textures,
        nodes,
        node_keys,
        shared_node_slots,
        commands,
    })
}

fn shared_node_slots(
    nodes: &[RenderNodeCommand],
    commands: &[RenderCommand],
    capacity: usize,
) -> Vec<Option<usize>> {
    let mut references = vec![0_usize; nodes.len()];
    for command in commands {
        if let RenderCommand::Effected { node, .. } = command {
            references[*node] += 1;
        }
    }
    for node in nodes {
        match &node.kind {
            RenderNodeCommandKind::Source(RenderSourceCommand::RenderedTexture {
                input, ..
            })
            | RenderNodeCommandKind::Effect { input, .. } => references[*input] += 1,
            RenderNodeCommandKind::Composite { children } => {
                for child in children {
                    references[*child] += 1;
                }
            }
            RenderNodeCommandKind::TemporalEffect { samples } => {
                for (sample, _) in samples {
                    references[*sample] += 1;
                }
            }
            RenderNodeCommandKind::Source(_) => {}
        }
    }
    let mut candidates = nodes
        .iter()
        .zip(&references)
        .enumerate()
        .filter_map(|(index, (node, references))| {
            let cacheable = matches!(
                node.kind,
                RenderNodeCommandKind::Source(RenderSourceCommand::RenderedTexture { .. })
                    | RenderNodeCommandKind::Composite { .. }
                    | RenderNodeCommandKind::Effect { .. }
                    | RenderNodeCommandKind::TemporalEffect { .. }
            );
            (*references > 1 && cacheable).then_some((index, *references))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(index, references)| (std::cmp::Reverse(*references), *index));
    let mut slots = vec![None; nodes.len()];
    for (slot, (node, _)) in candidates.into_iter().take(capacity).enumerate() {
        slots[node] = Some(slot);
    }
    slots
}

fn shared_node_cache_capacity(size: RenderSize) -> usize {
    let bytes = usize::try_from(size.width)
        .ok()
        .and_then(|width| {
            usize::try_from(size.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(SCENE_BYTES_PER_PIXEL));
    bytes
        .filter(|bytes| *bytes > 0)
        .map(|bytes| SHARED_NODE_CACHE_BUDGET / bytes)
        .unwrap_or(0)
        .min(MAX_SHARED_NODE_CACHE_ENTRIES)
}
