use super::PROPERTY_WORD_SIZE;
use super::scene::{
    RenderEffect, RenderEffectPassKind, RenderError, RenderItem, RenderItemSource,
    RenderNodeContent, RenderScene, RenderSize, RenderTemporalSample, RenderView, SceneNodeId,
};
use super::surface::SurfaceRect;
use crate::engine::frame::RgbaFrame;
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use zerium_core::plugin::{ComputeDispatchDimension, EffectInputSpace};
use zerium_core::timeline::BlendMode;
use zerium_shader::{EffectShaderId, ItemShaderId};

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
    pub(super) surface_min: [f32; 2],
    pub(super) surface_size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuEffect {
    pub(super) property_offset: u32,
    pub(super) property_size: u32,
    pub(super) sample_index: u32,
    pub(super) sample_count: u32,
    pub(super) frame_offset: f32,
    pub(super) sample_progress: f32,
    pub(super) composition_size: [f32; 2],
    pub(super) surface_min: [f32; 2],
    pub(super) surface_size: [f32; 2],
    pub(super) input_min: [f32; 2],
    pub(super) input_size: [f32; 2],
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
    pub(super) surface_min: [f32; 2],
    pub(super) surface_size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuComposite {
    pub(super) input_size: [u32; 2],
    pub(super) output_size: [u32; 2],
    pub(super) uv_x: [f32; 4],
    pub(super) uv_y: [f32; 4],
    pub(super) blend_mode: [u32; 4],
}

impl GpuComposite {
    pub(super) fn new(
        input_size: RenderSize,
        input: SurfaceRect,
        output_size: RenderSize,
        output: SurfaceRect,
        view: RenderView,
        blend_mode: BlendMode,
    ) -> Self {
        let (sin, cos) = view.angle.to_radians().sin_cos();
        let inverse = [
            [cos / view.zoom, -sin / view.zoom],
            [sin / view.zoom, cos / view.zoom],
        ];
        let row = |axis: usize| {
            let extent = input.max[axis] - input.min[axis];
            [
                inverse[axis][0] * ((output.max[0] - output.min[0]) / extent) as f32
                    / output_size.width as f32,
                inverse[axis][1] * ((output.max[1] - output.min[1]) / extent) as f32
                    / output_size.height as f32,
                ((f64::from(view.position[axis])
                    + f64::from(inverse[axis][0]) * output.min[0]
                    + f64::from(inverse[axis][1]) * output.min[1]
                    - input.min[axis])
                    / extent) as f32,
                0.,
            ]
        };
        Self {
            input_size: [input_size.width, input_size.height],
            output_size: [output_size.width, output_size.height],
            uv_x: row(0),
            uv_y: row(1),
            blend_mode: [blend_mode as u32, 0, 0, 0],
        }
    }
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
    Surface {
        layer: RenderLayer,
        render_scale: u32,
    },
}

/// Blending belongs to the edge into a scene, not to the reusable input surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RenderLayer {
    pub(super) node: RenderNodeId,
    pub(super) blend_mode: BlendMode,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum RenderSourceCommand {
    Transparent,
    Item {
        shader: ItemShaderId,
        instance: u32,
        capabilities: Vec<RenderNodeId>,
    },
    Texture {
        index: usize,
        shader: ItemShaderId,
    },
}

#[derive(Debug, PartialEq)]
pub(super) struct RenderNodeCommand {
    pub(super) kind: RenderNodeCommandKind,
    /// Pixels with a possible non-transparent contribution from this node.
    pub(super) bounds: SurfaceRect,
}

#[derive(Debug, PartialEq)]
pub(super) enum RenderNodeCommandKind {
    Source(RenderSourceCommand),
    Composite {
        children: Vec<RenderLayer>,
        view: RenderView,
    },
    Effect {
        input: RenderNodeId,
        capabilities: Vec<RenderNodeId>,
        input_space: EffectInputSpace,
        passes: Vec<EffectPassCommand>,
    },
    TemporalEffect {
        // Every branch includes the effects preceding this temporal effect.
        samples: Vec<(RenderNodeId, TemporalReduceCommand)>,
        capabilities: Vec<RenderNodeId>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct EffectPassCommand {
    pub(super) shader: EffectShaderId,
    pub(super) instance: u32,
    pub(super) kind: EffectPassCommandKind,
    pub(super) captures_source: bool,
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

#[derive(Default)]
pub(super) struct EncodedScene {
    pub(super) background: [f64; 4],
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
    pub(super) shader: ItemShaderId,
    pub(super) frames: Vec<Arc<RgbaFrame>>,
    pub(super) properties: Vec<u8>,
    pub(super) target_size: RenderSize,
    pub(super) composition_size: RenderSize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum SourceKey {
    Item {
        shader: ItemShaderId,
        capabilities: Vec<Arc<RenderNodeKey>>,
        properties: Vec<u8>,
        target_size: RenderSize,
        render_scale: u32,
        bounds: [u64; 4],
    },
    Texture {
        shader: ItemShaderId,
        frames: Vec<usize>,
        properties: Vec<u8>,
        target_size: RenderSize,
        render_scale: u32,
        bounds: [u64; 4],
    },
}

fn bounds_key(bounds: SurfaceRect) -> [u64; 4] {
    [
        bounds.min[0].to_bits(),
        bounds.min[1].to_bits(),
        bounds.max[0].to_bits(),
        bounds.max[1].to_bits(),
    ]
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
    Transparent {
        owner: Arc<RenderNodeKey>,
    },
    Composite {
        children: Vec<(Arc<RenderNodeKey>, BlendMode)>,
        view: [u32; 4],
    },
    Effect {
        input: Arc<RenderNodeKey>,
        capabilities: Vec<Arc<RenderNodeKey>>,
        input_space: EffectInputSpace,
        passes: Vec<EffectPassKey>,
    },
    Temporal {
        reducer: EffectShaderId,
        properties: Vec<u8>,
        samples: Vec<(Arc<RenderNodeKey>, u32)>,
        capabilities: Vec<Arc<RenderNodeKey>>,
    },
}

struct SceneEncoder<'a> {
    scene: &'a RenderScene,
    output: EncodedScene,
    encoded_nodes: HashMap<SceneNodeId, RenderNodeId>,
    node_cache: HashMap<Arc<RenderNodeKey>, RenderNodeId>,
    source_cache: HashMap<SourceKey, RenderSourceCommand>,
}

impl SceneEncoder<'_> {
    fn push_effect(&mut self, properties: &[u8]) -> Result<u32, RenderError> {
        let instance = u32::try_from(self.output.effects.len())
            .map_err(|_| RenderError::backend("too many effect passes in one frame"))?;
        let property_offset =
            u32::try_from(self.output.effect_properties.len() / PROPERTY_WORD_SIZE)
                .map_err(|_| RenderError::backend("effect property offset exceeds u32"))?;
        let property_size = u32::try_from(properties.len())
            .map_err(|_| RenderError::backend("effect property size exceeds u32"))?;
        self.output.effect_properties.extend_from_slice(properties);
        self.output.effect_properties.resize(
            self.output
                .effect_properties
                .len()
                .next_multiple_of(PROPERTY_WORD_SIZE),
            0,
        );
        self.output.effects.push(GpuEffect {
            property_offset,
            property_size,
            composition_size: [
                self.scene.composition_size.width as f32,
                self.scene.composition_size.height as f32,
            ],
            ..GpuEffect::zeroed()
        });
        Ok(instance)
    }

    fn intern_node(
        &mut self,
        key: RenderNodeKey,
        kind: RenderNodeCommandKind,
        bounds: SurfaceRect,
    ) -> RenderNodeId {
        let key = Arc::new(key);
        if let Some(id) = self.node_cache.get(&key) {
            return *id;
        }
        let id = self.output.nodes.len();
        self.output.nodes.push(RenderNodeCommand { kind, bounds });
        self.output.node_keys.push(key.clone());
        self.node_cache.insert(key, id);
        id
    }

    fn encode_source(
        &mut self,
        item: &RenderItem,
    ) -> Result<(RenderSourceCommand, SourceKey), RenderError> {
        match &item.source {
            RenderItemSource::Shader => {
                let capabilities = item
                    .inputs
                    .iter()
                    .map(|input| self.encode_node(*input))
                    .collect::<Result<Vec<_>, _>>()?;
                let key = SourceKey::Item {
                    shader: item.shader.clone(),
                    capabilities: capabilities
                        .iter()
                        .map(|id| self.output.node_keys[*id].clone())
                        .collect(),
                    properties: item.properties.clone(),
                    target_size: item.target_size,
                    render_scale: item.render_scale,
                    bounds: bounds_key(item.output_bounds),
                };
                if let Some(source) = self.source_cache.get(&key) {
                    return Ok((source.clone(), key));
                }
                let instance = u32::try_from(self.output.items.len())
                    .map_err(|_| RenderError::backend("too many visible items in one frame"))?;
                let property_offset =
                    u32::try_from(self.output.properties.len() / PROPERTY_WORD_SIZE)
                        .map_err(|_| RenderError::backend("item property offset exceeds u32"))?;
                let property_size = u32::try_from(item.properties.len())
                    .map_err(|_| RenderError::backend("item property size exceeds u32"))?;
                self.output.properties.extend_from_slice(&item.properties);
                self.output.properties.resize(
                    self.output
                        .properties
                        .len()
                        .next_multiple_of(PROPERTY_WORD_SIZE),
                    0,
                );
                self.output.items.push(GpuItem {
                    property_offset,
                    property_size,
                    output_size: [
                        item.target_size.width as f32,
                        item.target_size.height as f32,
                    ],
                    composition_size: [
                        self.scene.composition_size.width as f32,
                        self.scene.composition_size.height as f32,
                    ],
                    surface_min: [
                        -(self.scene.composition_size.width as f32) * 0.5,
                        -(self.scene.composition_size.height as f32) * 0.5,
                    ],
                    surface_size: [
                        self.scene.composition_size.width as f32,
                        self.scene.composition_size.height as f32,
                    ],
                });
                let source = RenderSourceCommand::Item {
                    shader: item.shader.clone(),
                    instance,
                    capabilities,
                };
                self.source_cache.insert(key.clone(), source.clone());
                Ok((source, key))
            }
            RenderItemSource::Texture(frames) => {
                let key = SourceKey::Texture {
                    shader: item.shader.clone(),
                    frames: frames
                        .iter()
                        .map(|frame| Arc::as_ptr(frame) as usize)
                        .collect(),
                    properties: item.properties.clone(),
                    target_size: item.target_size,
                    render_scale: item.render_scale,
                    bounds: bounds_key(item.output_bounds),
                };
                if let Some(source) = self.source_cache.get(&key) {
                    return Ok((source.clone(), key));
                }
                let index = self.output.textures.len();
                self.output.textures.push(EncodedTexture {
                    shader: item.shader.clone(),
                    frames: frames.clone(),
                    properties: item.properties.clone(),
                    target_size: item.target_size,
                    composition_size: self.scene.composition_size,
                });
                let source = RenderSourceCommand::Texture {
                    index,
                    shader: item.shader.clone(),
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
        let sample_index = u32::try_from(sample_index)
            .map_err(|_| RenderError::backend("temporal sample index exceeds u32"))?;
        let sample_count = u32::try_from(sample_count)
            .map_err(|_| RenderError::backend("temporal sample count exceeds u32"))?;
        let instance = self.push_effect(properties)?;
        let effect = &mut self.output.effects[instance as usize];
        effect.sample_index = sample_index;
        effect.sample_count = sample_count;
        effect.frame_offset = sample.frame_offset;
        effect.sample_progress = (sample_index as f32 + 0.5) / sample_count.max(1) as f32;
        Ok(TemporalReduceCommand {
            reducer: reducer.clone(),
            instance,
        })
    }

    fn encode_effects(
        &mut self,
        mut node: RenderNodeId,
        effects: &[RenderEffect],
        fixed_bounds: Option<SurfaceRect>,
    ) -> Result<RenderNodeId, RenderError> {
        for effect in effects {
            let has_regular_pass = effect
                .passes
                .iter()
                .any(|pass| !matches!(&pass.kind, RenderEffectPassKind::Temporal(_)));
            let capabilities = effect
                .inputs
                .iter()
                .map(|input| self.encode_node(*input))
                .collect::<Result<Vec<_>, _>>()?;
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
                                    Some(sample) => self.encode_node(*sample)?,
                                    None => self.intern_node(
                                        RenderNodeKey::Transparent {
                                            owner: self.output.node_keys[node].clone(),
                                        },
                                        RenderNodeCommandKind::Source(
                                            RenderSourceCommand::Transparent,
                                        ),
                                        self.output.nodes[node].bounds,
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
                                    (self.output.node_keys[*sample].clone(), *offset)
                                })
                                .collect(),
                            capabilities: capabilities
                                .iter()
                                .map(|id| self.output.node_keys[*id].clone())
                                .collect(),
                        };
                        let samples = encoded_samples
                            .into_iter()
                            .map(|(sample, reduce, _)| (sample, reduce))
                            .collect::<Vec<_>>();
                        let temporal_bounds = fixed_bounds.unwrap_or_else(|| {
                            let input_bounds = samples
                                .iter()
                                .filter(|(sample, _)| {
                                    !matches!(
                                        self.output.nodes[*sample].kind,
                                        RenderNodeCommandKind::Source(
                                            RenderSourceCommand::Transparent
                                        )
                                    )
                                })
                                .map(|(sample, _)| self.output.nodes[*sample].bounds)
                                .reduce(SurfaceRect::union)
                                .unwrap_or(self.output.nodes[node].bounds);
                            if has_regular_pass {
                                input_bounds
                            } else {
                                effect.output_bounds.apply(
                                    input_bounds,
                                    SurfaceRect::viewport(self.scene.composition_size),
                                )
                            }
                        });
                        node = self.intern_node(
                            key,
                            RenderNodeCommandKind::TemporalEffect {
                                samples,
                                capabilities: capabilities.clone(),
                            },
                            temporal_bounds,
                        );
                        continue;
                    }
                    RenderEffectPassKind::Render => EffectPassCommandKind::Render,
                    RenderEffectPassKind::Compute(dispatch) => EffectPassCommandKind::Compute {
                        dispatch: *dispatch,
                    },
                };
                let starts_regular_chain = regular_passes.is_empty();
                let instance = self.push_effect(&pass.properties)?;
                regular_passes.push(EffectPassCommand {
                    shader: pass.shader.clone(),
                    instance,
                    kind,
                    captures_source: starts_regular_chain,
                });
                regular_keys.push(EffectPassKey {
                    shader: pass.shader.clone(),
                    properties: pass.properties.clone(),
                    kind,
                    captures_source: starts_regular_chain,
                });
            }
            if !regular_passes.is_empty() {
                let bounds = fixed_bounds.unwrap_or_else(|| {
                    effect.output_bounds.apply(
                        self.output.nodes[node].bounds,
                        SurfaceRect::viewport(self.scene.composition_size),
                    )
                });
                node = self.intern_node(
                    RenderNodeKey::Effect {
                        input: self.output.node_keys[node].clone(),
                        capabilities: capabilities
                            .iter()
                            .map(|id| self.output.node_keys[*id].clone())
                            .collect(),
                        input_space: effect.input_space,
                        passes: regular_keys,
                    },
                    RenderNodeCommandKind::Effect {
                        input: node,
                        capabilities,
                        input_space: effect.input_space,
                        passes: regular_passes,
                    },
                    bounds,
                );
            }
        }
        Ok(node)
    }

    fn encode_node(&mut self, id: SceneNodeId) -> Result<RenderNodeId, RenderError> {
        if let Some(encoded) = self.encoded_nodes.get(&id) {
            return Ok(*encoded);
        }
        let nodes = &self.scene.nodes;
        let node = &nodes[id];
        let encoded = match &node.content {
            RenderNodeContent::Item(item) => {
                let (source, key) = self.encode_source(item)?;
                let source = self.intern_node(
                    RenderNodeKey::Source(key),
                    RenderNodeCommandKind::Source(source),
                    item.output_bounds,
                );
                self.encode_effects(source, &item.effects, None)
            }
            RenderNodeContent::Scene {
                children,
                view,
                effects,
                ..
            } => {
                let children = children
                    .iter()
                    .map(|child| {
                        Ok(RenderLayer {
                            node: self.encode_node(*child)?,
                            blend_mode: self.scene.nodes[*child].blend_mode,
                        })
                    })
                    .collect::<Result<Vec<_>, RenderError>>()?;
                let composite = self.intern_node(
                    RenderNodeKey::Composite {
                        children: children
                            .iter()
                            .map(|child| {
                                (self.output.node_keys[child.node].clone(), child.blend_mode)
                            })
                            .collect(),
                        view: [
                            view.position[0].to_bits(),
                            view.position[1].to_bits(),
                            view.zoom.to_bits(),
                            view.angle.to_bits(),
                        ],
                    },
                    RenderNodeCommandKind::Composite {
                        children,
                        view: *view,
                    },
                    SurfaceRect::viewport(self.scene.composition_size),
                );
                self.encode_effects(
                    composite,
                    effects,
                    Some(SurfaceRect::viewport(self.scene.composition_size)),
                )
            }
        }?;
        self.encoded_nodes.insert(id, encoded);
        Ok(encoded)
    }
}

pub(super) fn encode_scene(scene: &RenderScene) -> Result<EncodedScene, RenderError> {
    let mut encoder = SceneEncoder {
        scene,
        output: EncodedScene {
            background: scene.background,
            ..EncodedScene::default()
        },
        encoded_nodes: HashMap::new(),
        node_cache: HashMap::new(),
        source_cache: HashMap::new(),
    };
    let viewport = SurfaceRect::viewport(scene.composition_size);
    for node in &scene.roots {
        let root = encoder.encode_node(*node)?;
        let blend_mode = scene.nodes[*node].blend_mode;
        match &encoder.output.nodes[root].kind {
            RenderNodeCommandKind::Source(RenderSourceCommand::Item {
                shader,
                instance,
                capabilities,
            }) if capabilities.is_empty() && blend_mode == BlendMode::Normal => {
                match encoder.output.commands.last_mut() {
                    Some(RenderCommand::Items(batch))
                        if batch.shader == *shader && batch.instances.end == *instance =>
                    {
                        batch.instances.end = instance + 1;
                    }
                    _ => encoder
                        .output
                        .commands
                        .push(RenderCommand::Items(ItemBatch {
                            shader: shader.clone(),
                            instances: *instance..instance + 1,
                        })),
                }
            }
            RenderNodeCommandKind::Source(RenderSourceCommand::Texture { index, shader })
                if encoder.output.nodes[root].bounds == viewport
                    && blend_mode == BlendMode::Normal =>
            {
                encoder.output.commands.push(RenderCommand::Texture {
                    index: *index,
                    shader: shader.clone(),
                });
            }
            _ => encoder.output.commands.push(RenderCommand::Surface {
                layer: RenderLayer {
                    node: root,
                    blend_mode,
                },
                render_scale: scene.nodes[*node].render_scale,
            }),
        }
    }

    encoder.output.shared_node_slots = shared_node_slots(
        &encoder.output.nodes,
        &encoder.output.commands,
        viewport,
        shared_node_cache_capacity(scene.effect_size),
    );
    Ok(encoder.output)
}

fn shared_node_slots(
    nodes: &[RenderNodeCommand],
    commands: &[RenderCommand],
    viewport: SurfaceRect,
    capacity: usize,
) -> Vec<Option<usize>> {
    let mut references = vec![0_usize; nodes.len()];
    for command in commands {
        if let RenderCommand::Surface { layer, .. } = command {
            references[layer.node] += 1;
        }
    }
    for node in nodes {
        match &node.kind {
            RenderNodeCommandKind::Source(RenderSourceCommand::Item { capabilities, .. }) => {
                for capability in capabilities {
                    references[*capability] += 1;
                }
            }
            RenderNodeCommandKind::Effect {
                input,
                capabilities,
                ..
            } => {
                references[*input] += 1;
                for capability in capabilities {
                    references[*capability] += 1;
                }
            }
            RenderNodeCommandKind::Composite { children, .. } => {
                for child in children {
                    references[child.node] += 1;
                }
            }
            RenderNodeCommandKind::TemporalEffect {
                samples,
                capabilities,
                ..
            } => {
                for (sample, _) in samples {
                    references[*sample] += 1;
                }
                for capability in capabilities {
                    references[*capability] += 1;
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
                RenderNodeCommandKind::Composite { .. }
                    | RenderNodeCommandKind::Effect { .. }
                    | RenderNodeCommandKind::TemporalEffect { .. }
            );
            (*references > 1 && cacheable && node.bounds == viewport)
                .then_some((index, *references))
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
