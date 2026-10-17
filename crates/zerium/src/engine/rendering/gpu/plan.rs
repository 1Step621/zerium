use super::PROPERTY_WORD_SIZE;
use super::bindings::{GpuItem, GpuPass};
use super::scene::{
    BoundsContext, RenderEffect, RenderEffectPassKind, RenderError, RenderItem, RenderNodeContent,
    RenderScene, RenderSize, SceneNodeId, SurfaceRect,
};
use crate::engine::frame::VisualFrame;
use bytemuck::Zeroable;
use std::{collections::HashMap, ops::Range, sync::Arc};
use zerium_core::{plugin::ComputeDispatchDimension, timeline::BlendMode};
use zerium_shader::{EffectShaderId, ImageBinding, ItemShaderId};

pub(super) type RenderNodeId = usize;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ItemBatch {
    pub shader: ItemShaderId,
    pub instances: Range<u32>,
}

#[derive(Debug, PartialEq)]
pub(super) enum RenderCommand {
    Items(ItemBatch),
    Frame {
        index: usize,
    },
    Surface {
        layer: RenderLayer,
        render_scale: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RenderLayer {
    pub node: RenderNodeId,
    pub blend_mode: BlendMode,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ImageInput {
    pub node: RenderNodeId,
    pub binding: ImageBinding,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum RenderSourceCommand {
    Transparent,
    Item {
        shader: ItemShaderId,
        instance: u32,
        images: Vec<ImageInput>,
    },
    Frame {
        index: usize,
    },
}

#[derive(Debug, PartialEq)]
pub(super) struct RenderNodeCommand {
    pub kind: RenderNodeCommandKind,
    pub bounds: SurfaceRect,
}

#[derive(Debug, PartialEq)]
pub(super) enum RenderNodeCommandKind {
    Source(RenderSourceCommand),
    Composite {
        children: Vec<RenderLayer>,
    },
    View {
        input: RenderNodeId,
        view: super::scene::RenderView,
    },
    Pass {
        images: Vec<ImageInput>,
        pass: PassCommand,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PassCommand {
    pub shader: EffectShaderId,
    pub instance: u32,
    pub kind: PassKind,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(super) enum PassKind {
    Render,
    Compute {
        dispatch: [ComputeDispatchDimension; 3],
    },
    Temporal,
}

#[derive(Default)]
pub(super) struct RenderPlan {
    pub background: [f64; 4],
    pub items: Vec<GpuItem>,
    pub properties: Vec<u8>,
    pub passes: Vec<GpuPass>,
    pub pass_properties: Vec<u8>,
    pub frames: Vec<Arc<VisualFrame>>,
    pub nodes: Vec<RenderNodeCommand>,
    pub node_keys: Vec<RenderNodeIdentity>,
    pub commands: Vec<RenderCommand>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum SourceKey {
    Item {
        shader: ItemShaderId,
        capabilities: Vec<RenderNodeIdentity>,
        properties: Vec<u8>,
        bounds: [u64; 4],
    },
    Frame {
        frame: usize,
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
enum RenderNodeKey {
    Source(SourceKey),
    Transparent {
        bounds: [u64; 4],
    },
    Composite {
        children: Vec<(RenderNodeIdentity, BlendMode)>,
        bounds: [u64; 4],
    },
    View {
        input: RenderNodeIdentity,
        view: [u32; 4],
        bounds: [u64; 4],
    },
    Pass {
        images: Vec<(ImageBinding, RenderNodeIdentity)>,
        shader: EffectShaderId,
        properties: Vec<u8>,
        kind: PassKind,
        temporal: [u32; 4],
        bounds: [u64; 4],
    },
}

/// Stable across consecutive frames while the node's complete description is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct RenderNodeIdentity(u64);

/// Retains only the previous frame's descriptions. Keys contain child identities,
/// so hashing and equality never traverse the render graph.
#[derive(Default)]
pub(super) struct NodeIdentityCache {
    previous: HashMap<RenderNodeKey, RenderNodeIdentity>,
    next: u64,
}

struct PassInputs<'a> {
    input: RenderNodeId,
    source: RenderNodeId,
    capabilities: &'a [RenderNodeId],
}

struct RenderPlanner<'a> {
    identities: &'a mut NodeIdentityCache,
    scene: &'a RenderScene,
    output: RenderPlan,
    encoded_nodes: HashMap<SceneNodeId, RenderNodeId>,
    node_cache: HashMap<RenderNodeKey, RenderNodeId>,
    source_cache: HashMap<SourceKey, RenderSourceCommand>,
}

impl RenderPlanner<'_> {
    fn push_pass(&mut self, properties: &[u8], temporal: [u32; 4]) -> Result<u32, RenderError> {
        let instance = u32::try_from(self.output.passes.len())
            .map_err(|_| RenderError::resource_limit("too many passes"))?;
        let offset = u32::try_from(self.output.pass_properties.len() / PROPERTY_WORD_SIZE)
            .map_err(|_| RenderError::resource_limit("pass properties overflow"))?;
        self.output.pass_properties.extend_from_slice(properties);
        self.output.pass_properties.resize(
            self.output
                .pass_properties
                .len()
                .next_multiple_of(PROPERTY_WORD_SIZE),
            0,
        );
        self.output.passes.push(GpuPass {
            property_offset: offset,
            property_size: properties.len() as u32,
            sample_index: temporal[0],
            sample_count: temporal[1],
            frame_offset: f32::from_bits(temporal[2]),
            sample_progress: f32::from_bits(temporal[3]),
            composition_size: [
                self.scene.composition_size.width as f32,
                self.scene.composition_size.height as f32,
            ],
            ..GpuPass::zeroed()
        });
        Ok(instance)
    }

    fn intern_node(
        &mut self,
        key: RenderNodeKey,
        kind: RenderNodeCommandKind,
        bounds: SurfaceRect,
    ) -> RenderNodeId {
        if let Some(id) = self.node_cache.get(&key) {
            return *id;
        }

        let id = self.output.nodes.len();
        self.output.nodes.push(RenderNodeCommand { kind, bounds });
        let identity = self
            .identities
            .previous
            .get(&key)
            .copied()
            .unwrap_or_else(|| {
                self.identities.next = self
                    .identities
                    .next
                    .checked_add(1)
                    .expect("render node identity space exhausted");
                RenderNodeIdentity(self.identities.next)
            });
        self.output.node_keys.push(identity);
        self.node_cache.insert(key, id);
        id
    }

    fn encode_source(
        &mut self,
        item: &RenderItem,
    ) -> Result<(RenderSourceCommand, SourceKey, SurfaceRect), RenderError> {
        let capabilities = item
            .inputs
            .iter()
            .map(|input| self.encode_node(*input))
            .collect::<Result<Vec<_>, _>>()?;
        let image_bounds = item
            .input_ids
            .iter()
            .cloned()
            .zip(capabilities.iter().map(|id| self.output.nodes[*id].bounds))
            .collect::<Vec<_>>();
        let viewport = SurfaceRect::viewport(self.scene.composition_size);
        let bounds = item.output_bounds.evaluate(&BoundsContext {
            input: viewport,
            source: viewport,
            viewport,
            images: &image_bounds,
        });
        let key = SourceKey::Item {
            shader: item.shader.clone(),
            capabilities: capabilities
                .iter()
                .map(|id| self.output.node_keys[*id])
                .collect(),
            properties: item.properties.clone(),
            bounds: bounds_key(bounds),
        };

        if let Some(source) = self.source_cache.get(&key) {
            return Ok((source.clone(), key, bounds));
        }

        let instance = u32::try_from(self.output.items.len())
            .map_err(|_| RenderError::backend("too many visible items in one frame"))?;
        let property_offset = u32::try_from(self.output.properties.len() / PROPERTY_WORD_SIZE)
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
        });
        let source = RenderSourceCommand::Item {
            shader: item.shader.clone(),
            instance,
            images: capabilities
                .iter()
                .enumerate()
                .map(|(index, &node)| ImageInput {
                    node,
                    binding: ImageBinding::named(index),
                })
                .collect(),
        };
        self.source_cache.insert(key.clone(), source.clone());
        Ok((source, key, bounds))
    }

    fn encode_frame(&mut self, frame: &Arc<VisualFrame>) -> RenderNodeId {
        let bounds = SurfaceRect::viewport(RenderSize {
            width: frame.width,
            height: frame.height,
        });
        let key = SourceKey::Frame {
            frame: Arc::as_ptr(frame) as usize,
            bounds: bounds_key(bounds),
        };
        let source = self
            .source_cache
            .entry(key.clone())
            .or_insert_with(|| {
                let index = self.output.frames.len();
                self.output.frames.push(frame.clone());
                RenderSourceCommand::Frame { index }
            })
            .clone();
        self.intern_node(
            RenderNodeKey::Source(key),
            RenderNodeCommandKind::Source(source),
            bounds,
        )
    }

    fn transparent(&mut self, bounds: SurfaceRect) -> RenderNodeId {
        self.intern_node(
            RenderNodeKey::Transparent {
                bounds: bounds_key(bounds),
            },
            RenderNodeCommandKind::Source(RenderSourceCommand::Transparent),
            bounds,
        )
    }

    fn encode_effects(
        &mut self,
        mut node: RenderNodeId,
        effects: &[RenderEffect],
    ) -> Result<RenderNodeId, RenderError> {
        for effect in effects {
            let capabilities = effect
                .inputs
                .iter()
                .map(|input| self.encode_node(*input))
                .collect::<Result<Vec<_>, _>>()?;
            let image_bounds = effect
                .input_ids
                .iter()
                .cloned()
                .zip(capabilities.iter().map(|id| self.output.nodes[*id].bounds))
                .collect::<Vec<_>>();
            let mut temporal_inputs = Vec::new();
            if let Some(pass) = effect.passes.first()
                && let RenderEffectPassKind::Temporal(samples) = &pass.kind
            {
                for sample in samples {
                    temporal_inputs.push((
                        match sample.input {
                            Some(input) => Some(self.encode_node(input)?),
                            None => None,
                        },
                        sample.frame_offset,
                    ));
                }
            }

            let basis = temporal_inputs
                .iter()
                .filter_map(|(input, _)| input.map(|id| self.output.nodes[id].bounds))
                .filter(|bounds| !bounds.is_empty())
                .reduce(SurfaceRect::union)
                .unwrap_or(self.output.nodes[node].bounds);
            let viewport = SurfaceRect::viewport(self.scene.composition_size);
            let pass_inputs = |input, source| PassInputs {
                input,
                source,
                capabilities: &capabilities,
            };
            let mut source = node;
            for pass in &effect.passes {
                let (input_bounds, source_bounds) =
                    if matches!(pass.kind, RenderEffectPassKind::Temporal(_)) {
                        (basis, basis)
                    } else {
                        (
                            self.output.nodes[node].bounds,
                            self.output.nodes[source].bounds,
                        )
                    };
                let bounds = pass.output_bounds.evaluate(&BoundsContext {
                    input: input_bounds,
                    source: source_bounds,
                    viewport,
                    images: &image_bounds,
                });

                match &pass.kind {
                    RenderEffectPassKind::Temporal(_) => {
                        let count = temporal_inputs.len() as u32;
                        let mut accumulation = self.transparent(bounds);
                        for (index, (input, offset)) in temporal_inputs.iter().enumerate() {
                            let input = input.unwrap_or_else(|| self.transparent(bounds));
                            let temporal = [
                                index as u32,
                                count,
                                offset.to_bits(),
                                ((index as f32 + 0.5) / count.max(1) as f32).to_bits(),
                            ];
                            accumulation = self.encode_pass(
                                pass_inputs(input, accumulation),
                                pass,
                                PassKind::Temporal,
                                temporal,
                                bounds,
                            )?;
                        }

                        node = accumulation;
                        source = node;
                    }

                    RenderEffectPassKind::Render => {
                        node = self.encode_pass(
                            pass_inputs(node, source),
                            pass,
                            PassKind::Render,
                            [0; 4],
                            bounds,
                        )?
                    }
                    RenderEffectPassKind::Compute(dispatch) => {
                        node = self.encode_pass(
                            pass_inputs(node, source),
                            pass,
                            PassKind::Compute {
                                dispatch: *dispatch,
                            },
                            [0; 4],
                            bounds,
                        )?
                    }
                }
            }
        }

        Ok(node)
    }

    fn encode_pass(
        &mut self,
        inputs: PassInputs<'_>,
        pass: &super::scene::RenderEffectPass,
        kind: PassKind,
        temporal: [u32; 4],
        bounds: SurfaceRect,
    ) -> Result<RenderNodeId, RenderError> {
        let mut images = vec![
            ImageInput {
                node: inputs.input,
                binding: ImageBinding::INPUT,
            },
            ImageInput {
                node: inputs.source,
                binding: ImageBinding::SOURCE,
            },
        ];
        images.extend(
            inputs
                .capabilities
                .iter()
                .enumerate()
                .map(|(index, &node)| ImageInput {
                    node,
                    binding: ImageBinding::named(index),
                }),
        );
        let key = RenderNodeKey::Pass {
            images: images
                .iter()
                .map(|image| (image.binding, self.output.node_keys[image.node]))
                .collect(),
            shader: pass.shader.clone(),
            properties: pass.properties.clone(),
            kind,
            temporal,
            bounds: bounds_key(bounds),
        };
        if let Some(id) = self.node_cache.get(&key) {
            return Ok(*id);
        }
        let instance = self.push_pass(&pass.properties, temporal)?;
        Ok(self.intern_node(
            key,
            RenderNodeCommandKind::Pass {
                images,
                pass: PassCommand {
                    shader: pass.shader.clone(),
                    instance,
                    kind,
                },
            },
            bounds,
        ))
    }

    fn encode_composite(&mut self, children: &[SceneNodeId]) -> Result<RenderNodeId, RenderError> {
        let children = children
            .iter()
            .map(|child| {
                Ok(RenderLayer {
                    node: self.encode_node(*child)?,
                    blend_mode: self.scene.nodes[*child].blend_mode,
                })
            })
            .collect::<Result<Vec<_>, RenderError>>()?;
        let bounds = children
            .iter()
            .map(|child| self.output.nodes[child.node].bounds)
            .filter(|bounds| !bounds.is_empty())
            .reduce(SurfaceRect::union)
            .unwrap_or(SurfaceRect::EMPTY);
        Ok(self.intern_node(
            RenderNodeKey::Composite {
                bounds: bounds_key(bounds),
                children: children
                    .iter()
                    .map(|child| (self.output.node_keys[child.node], child.blend_mode))
                    .collect(),
            },
            RenderNodeCommandKind::Composite { children },
            bounds,
        ))
    }

    fn encode_node(&mut self, id: SceneNodeId) -> Result<RenderNodeId, RenderError> {
        if let Some(plan) = self.encoded_nodes.get(&id) {
            return Ok(*plan);
        }

        let node = &self.scene.nodes[id];
        let plan = match &node.content {
            RenderNodeContent::Frame(frame) => Ok(self.encode_frame(frame)),
            RenderNodeContent::Item(item) => {
                let (source, key, bounds) = self.encode_source(item)?;
                let source = self.intern_node(
                    RenderNodeKey::Source(key),
                    RenderNodeCommandKind::Source(source),
                    bounds,
                );
                self.encode_effects(source, &item.effects)
            }
            RenderNodeContent::Scene {
                children, effects, ..
            } => {
                let composite = self.encode_composite(children)?;
                self.encode_effects(composite, effects)
            }
            RenderNodeContent::View { input, view } => {
                let input = self.encode_node(*input)?;
                let bounds = SurfaceRect::viewport(self.scene.composition_size);
                Ok(self.intern_node(
                    RenderNodeKey::View {
                        input: self.output.node_keys[input],
                        view: [
                            view.position[0].to_bits(),
                            view.position[1].to_bits(),
                            view.zoom.to_bits(),
                            view.angle.to_bits(),
                        ],
                        bounds: bounds_key(bounds),
                    },
                    RenderNodeCommandKind::View { input, view: *view },
                    bounds,
                ))
            }
        }?;
        self.encoded_nodes.insert(id, plan);
        Ok(plan)
    }
}

pub(super) fn plan_scene(
    scene: &RenderScene,
    identities: &mut NodeIdentityCache,
) -> Result<RenderPlan, RenderError> {
    let mut encoder = RenderPlanner {
        identities,
        scene,
        output: RenderPlan {
            background: scene.background,
            ..RenderPlan::default()
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
                images,
            }) if images.is_empty()
                && encoder.output.nodes[root].bounds == viewport
                && blend_mode == BlendMode::Normal =>
            {
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

            RenderNodeCommandKind::Source(RenderSourceCommand::Frame { index })
                if encoder.output.nodes[root].bounds == viewport
                    && blend_mode == BlendMode::Normal =>
            {
                encoder
                    .output
                    .commands
                    .push(RenderCommand::Frame { index: *index });
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

    if encoder
        .output
        .nodes
        .iter()
        .any(|node| !node.bounds.is_valid())
    {
        return Err(RenderError::resource_limit(
            "render node has invalid logical bounds",
        ));
    }

    encoder.identities.previous = encoder
        .node_cache
        .into_iter()
        .map(|(key, node)| (key, encoder.output.node_keys[node]))
        .collect();
    Ok(encoder.output)
}
