use super::scene::{EffectProperties, RenderTextureInput};
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
        shader: TextureShaderId,
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
        shader: TextureShaderId,
    },
    RenderedTexture {
        input: RenderNodeId,
        index: usize,
        shader: TextureShaderId,
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
    pub(super) fn temporal_depth(&self, nodes: &[Self]) -> usize {
        match &self.kind {
            RenderNodeCommandKind::Source(RenderSourceCommand::RenderedTexture {
                input, ..
            }) => nodes[*input].temporal_depth(nodes),
            RenderNodeCommandKind::Source(_) => 0,
            RenderNodeCommandKind::Composite { children } => children
                .iter()
                .map(|child| nodes[*child].temporal_depth(nodes))
                .max()
                .unwrap_or(0),
            RenderNodeCommandKind::Effect { input, .. } => nodes[*input].temporal_depth(nodes),
            RenderNodeCommandKind::TemporalEffect { samples, .. } => {
                1 + samples
                    .iter()
                    .map(|(sample, _)| nodes[*sample].temporal_depth(nodes))
                    .max()
                    .unwrap_or(0)
            }
        }
    }

    pub(super) fn composition_depth(&self, nodes: &[Self]) -> usize {
        match &self.kind {
            RenderNodeCommandKind::Source(RenderSourceCommand::RenderedTexture {
                input, ..
            }) => nodes[*input].composition_depth(nodes),
            RenderNodeCommandKind::Source(_) => 0,
            RenderNodeCommandKind::Composite { children } => {
                1 + children
                    .iter()
                    .map(|child| nodes[*child].composition_depth(nodes))
                    .max()
                    .unwrap_or(0)
            }
            RenderNodeCommandKind::Effect { input, .. } => nodes[*input].composition_depth(nodes),
            RenderNodeCommandKind::TemporalEffect { samples } => samples
                .iter()
                .map(|(sample, _)| nodes[*sample].composition_depth(nodes))
                .max()
                .unwrap_or(0),
        }
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

pub(super) struct EncodedTexture {
    pub(super) shader: TextureShaderId,
    pub(super) input: EncodedTextureInput,
    pub(super) properties: ItemProperties,
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
        shader: TextureShaderId,
        frames: Vec<usize>,
        properties: Vec<u8>,
        target_size: RenderSize,
        render_scale: u32,
    },
    RenderedTexture {
        shader: TextureShaderId,
        input: Arc<RenderNodeKey>,
        properties: Vec<u8>,
        target_size: RenderSize,
        render_scale: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum EffectPassKey {
    Render {
        shader: EffectShaderId,
        properties: Vec<u8>,
        captures_source: bool,
    },
    Compute {
        shader: EffectShaderId,
        properties: Vec<u8>,
        dispatch: [ComputeDispatchDimension; 3],
        captures_source: bool,
    },
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
    properties: &EffectProperties,
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
    effect_properties.extend_from_slice(properties.as_bytes());
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
        match item {
            RenderItem::Shader(item) => {
                let key = SourceKey::Item {
                    shader: item.shader.clone(),
                    properties: item.properties.as_bytes().to_vec(),
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
                self.properties
                    .extend_from_slice(item.properties.as_bytes());
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
            RenderItem::Texture(item) => {
                let (input, render_input, frame_ids) = match &item.input {
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
                        properties: item.properties.as_bytes().to_vec(),
                        target_size: item.target_size,
                        render_scale: item.render_scale,
                    },
                    None => SourceKey::Texture {
                        shader: item.shader.clone(),
                        frames: frame_ids,
                        properties: item.properties.as_bytes().to_vec(),
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
        properties: &EffectProperties,
        sample_count: usize,
        sample_index: usize,
        sample: &RenderTemporalSample,
    ) -> Result<TemporalReduceCommand, RenderError> {
        let instance = u32::try_from(self.effects.len())
            .map_err(|_| RenderError::backend("too many effect passes in one frame"))?;
        let property_offset = u32::try_from(self.effect_properties.len() / PROPERTY_WORD_SIZE)
            .map_err(|_| RenderError::backend("effect property offset exceeds u32"))?;
        self.effect_properties
            .extend_from_slice(properties.as_bytes());
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
                match pass {
                    RenderEffectPass::Temporal {
                        reducer,
                        properties,
                        samples,
                    } => {
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
                                        reducer,
                                        properties,
                                        sample_count,
                                        sample_index,
                                        sample,
                                    )?,
                                    sample.frame_offset.to_bits(),
                                ))
                            })
                            .collect::<Result<Vec<_>, RenderError>>()?;
                        let key = RenderNodeKey::Temporal {
                            reducer: reducer.clone(),
                            properties: properties.as_bytes().to_vec(),
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
                    }
                    RenderEffectPass::Render { shader, properties } => {
                        let starts_regular_chain = regular_passes.is_empty();
                        regular_passes.push(encode_effect_pass(
                            shader,
                            properties,
                            EffectPassCommandKind::Render,
                            starts_regular_chain,
                            self.effects,
                            self.effect_properties,
                            self.composition_size,
                        )?);
                        regular_keys.push(EffectPassKey::Render {
                            shader: shader.clone(),
                            properties: properties.as_bytes().to_vec(),
                            captures_source: starts_regular_chain,
                        });
                    }
                    RenderEffectPass::Compute {
                        shader,
                        properties,
                        dispatch,
                    } => {
                        let starts_regular_chain = regular_passes.is_empty();
                        regular_passes.push(encode_effect_pass(
                            shader,
                            properties,
                            EffectPassCommandKind::Compute {
                                dispatch: *dispatch,
                            },
                            starts_regular_chain,
                            self.effects,
                            self.effect_properties,
                            self.composition_size,
                        )?);
                        regular_keys.push(EffectPassKey::Compute {
                            shader: shader.clone(),
                            properties: properties.as_bytes().to_vec(),
                            dispatch: *dispatch,
                            captures_source: starts_regular_chain,
                        });
                    }
                }
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
                let effects = match item {
                    RenderItem::Shader(item) => &item.effects,
                    RenderItem::Texture(item) => &item.effects,
                };
                let (source, key) = self.encode_source(item)?;
                let source = self.intern_node(
                    RenderNodeKey::Source(key),
                    node.metadata.clone(),
                    RenderNodeCommandKind::Source(source),
                );
                self.encode_effects(source, effects)
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
            RenderNodeContent::Item(RenderItem::Shader(item)) => (
                !item.effects.is_empty(),
                Some(RenderItem::Shader(item.clone())),
            ),
            RenderNodeContent::Item(RenderItem::Texture(item)) => (
                !item.effects.is_empty() || matches!(&item.input, RenderTextureInput::Rendered(_)),
                Some(RenderItem::Texture(item.clone())),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::rendering::scene::RenderShaderItem;

    fn metadata() -> RenderNodeMetadata {
        RenderNodeMetadata {
            layer: LayerId::new(0),
            clip_start: TimelineTime::from_frame(crate::domain::timeline::Frame::new(0)),
            clip_end: TimelineTime::from_frame(crate::domain::timeline::Frame::new(1)),
            path: Vec::new(),
        }
    }

    #[test]
    fn only_reused_complex_nodes_receive_cache_slots() {
        let nodes = vec![
            RenderNodeCommand {
                metadata: metadata(),
                kind: RenderNodeCommandKind::Source(RenderSourceCommand::Transparent),
            },
            RenderNodeCommand {
                metadata: metadata(),
                kind: RenderNodeCommandKind::Composite { children: vec![0] },
            },
        ];
        let commands = vec![
            RenderCommand::Effected {
                node: 1,
                render_scale: 1,
            },
            RenderCommand::Effected {
                node: 1,
                render_scale: 1,
            },
        ];

        assert_eq!(
            shared_node_slots(&nodes, &commands, usize::MAX),
            vec![None, Some(0)]
        );
    }

    #[test]
    fn shared_cache_respects_the_texture_memory_budget() {
        assert_eq!(
            shared_node_cache_capacity(RenderSize {
                width: 3_840,
                height: 2_160,
            }),
            2
        );
        assert_eq!(
            shared_node_cache_capacity(RenderSize {
                width: 7_680,
                height: 4_320,
            }),
            0
        );
    }

    fn shader_item() -> RenderItem {
        RenderItem::Shader(RenderShaderItem {
            shader: ItemShaderId::plugin_item("test", "item"),
            properties: ItemProperties::from_bytes([1, 2, 3, 4]),
            effects: Vec::new(),
            target_size: RenderSize {
                width: 64,
                height: 64,
            },
            render_scale: 1,
        })
    }

    #[test]
    fn identical_root_items_share_gpu_data_without_losing_a_draw() {
        let item = shader_item();
        let scene = RenderScene::from_roots(
            RenderSize {
                width: 64,
                height: 64,
            },
            RenderSize {
                width: 64,
                height: 64,
            },
            [0.; 4],
            vec![
                RenderNode::item(metadata(), item.clone()),
                RenderNode::item(metadata(), item),
            ],
        )
        .unwrap();

        let encoded = encode_items(&scene).unwrap();
        assert_eq!(encoded.items.len(), 1);
        assert_eq!(encoded.commands.len(), 2);
        assert!(encoded.commands.iter().all(|command| matches!(
            command,
            RenderCommand::Items(ItemBatch { instances, .. }) if instances == &(0..1)
        )));
    }

    #[test]
    fn identical_composites_are_interned_as_one_render_node() {
        let child = RenderNode::item(metadata(), shader_item());
        let composite = RenderNode::scene(metadata(), vec![child], Vec::new(), 1);
        let scene = RenderScene::from_roots(
            RenderSize {
                width: 64,
                height: 64,
            },
            RenderSize {
                width: 64,
                height: 64,
            },
            [0.; 4],
            vec![composite.clone(), composite],
        )
        .unwrap();

        let encoded = encode_items(&scene).unwrap();
        let roots = encoded
            .commands
            .iter()
            .map(|command| match command {
                RenderCommand::Effected { node, .. } => *node,
                _ => panic!("scene roots must be encoded as render nodes"),
            })
            .collect::<Vec<_>>();
        assert_eq!(roots[0], roots[1]);
        assert!(encoded.shared_node_slots[roots[0]].is_some());
    }
}
