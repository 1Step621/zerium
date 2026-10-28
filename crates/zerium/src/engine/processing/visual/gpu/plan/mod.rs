//! Logical frame graph and its lowering to ordered surface work.
pub(super) mod execution;
pub(super) mod surfaces;

use crate::engine::{
    input::VisualFrame,
    processing::visual::scene::{
        BoundsContext, RenderEffect, RenderEffectPassKind, RenderError, RenderItem,
        RenderNodeContent, RenderScene, RenderSize, SceneNodeId, SurfaceRect,
    },
};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::{Arc, Weak},
};
use zerium_core::{plugin::ComputeDispatchDimension, timeline::BlendMode};
use zerium_shader::{EffectShaderId, ImageBinding, ItemShaderId};

pub(super) type RenderNodeId = usize;

/// A root occurrence preserves composition order even when its node is shared.
#[derive(Clone, Copy, Debug)]
pub(super) struct RenderRoot {
    pub layer: RenderLayer,
    pub render_scale: u32,
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

#[derive(Debug, PartialEq)]
pub(super) struct RenderNode {
    pub identity: RenderNodeIdentity,
    /// The complete node description also existed in the previous frame.
    pub unchanged: bool,
    pub kind: RenderNodeKind,
    pub bounds: SurfaceRect,
}

#[derive(Debug, PartialEq)]
pub(super) enum RenderNodeKind {
    Transparent,
    Item {
        shader: ItemShaderId,
        properties: Arc<[u8]>,
        images: Vec<ImageInput>,
    },
    Frame {
        frame: Arc<VisualFrame>,
    },
    Composite {
        children: Vec<RenderLayer>,
    },
    View {
        input: RenderNodeId,
        view: crate::engine::processing::visual::scene::RenderView,
    },
    Pass {
        images: Vec<ImageInput>,
        pass: ShaderPass,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ShaderPass {
    pub shader: EffectShaderId,
    pub kind: PassKind,
    pub properties: Arc<[u8]>,
    pub temporal: [u32; 4],
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(super) enum PassKind {
    Render,
    Compute {
        dispatch: [ComputeDispatchDimension; 3],
    },
}

#[derive(Default)]
pub(super) struct RenderGraph {
    pub background: [f64; 4],
    pub nodes: Vec<RenderNode>,
    pub roots: Vec<RenderRoot>,
}

/// Reserve the Arc allocation's address while its identity is cached, without
/// retaining decoded pixels when a cached result no longer needs its inputs.
#[derive(Clone, Debug)]
struct FrameIdentity(Weak<VisualFrame>);

impl PartialEq for FrameIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }
}

impl Eq for FrameIdentity {}

impl Hash for FrameIdentity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_ptr().hash(state);
    }
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
    Transparent {
        bounds: [u64; 4],
    },
    Item {
        shader: ItemShaderId,
        images: Vec<(ImageBinding, RenderNodeIdentity)>,
        properties: Arc<[u8]>,
        bounds: [u64; 4],
    },
    Frame {
        frame: FrameIdentity,
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
        properties: Arc<[u8]>,
        kind: PassKind,
        temporal: [u32; 4],
        bounds: [u64; 4],
    },
}

impl RenderNodeKind {
    /// Keys are derived from the same definition that lowering consumes. Child
    /// identities keep equality independent of the size of dependency subtrees.
    fn key(&self, bounds: SurfaceRect, nodes: &[RenderNode]) -> RenderNodeKey {
        let bounds = bounds_key(bounds);
        let image_keys = |images: &[ImageInput]| {
            images
                .iter()
                .map(|image| (image.binding, nodes[image.node].identity))
                .collect()
        };
        match self {
            Self::Transparent => RenderNodeKey::Transparent { bounds },
            Self::Item {
                shader,
                properties,
                images,
            } => RenderNodeKey::Item {
                shader: shader.clone(),
                properties: properties.clone(),
                images: image_keys(images),
                bounds,
            },
            Self::Frame { frame } => RenderNodeKey::Frame {
                frame: FrameIdentity(Arc::downgrade(frame)),
                bounds,
            },
            Self::Composite { children } => RenderNodeKey::Composite {
                children: children
                    .iter()
                    .map(|child| (nodes[child.node].identity, child.blend_mode))
                    .collect(),
                bounds,
            },
            Self::View { input, view } => RenderNodeKey::View {
                input: nodes[*input].identity,
                view: [
                    view.position[0].to_bits(),
                    view.position[1].to_bits(),
                    view.zoom.to_bits(),
                    view.angle.to_bits(),
                ],
                bounds,
            },
            Self::Pass { images, pass } => RenderNodeKey::Pass {
                images: image_keys(images),
                shader: pass.shader.clone(),
                properties: pass.properties.clone(),
                kind: pass.kind,
                temporal: pass.temporal,
                bounds,
            },
        }
    }
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

struct RenderGraphBuilder<'a> {
    identities: &'a mut NodeIdentityCache,
    scene: &'a RenderScene,
    output: RenderGraph,
    encoded_nodes: Vec<Option<RenderNodeId>>,
    node_cache: HashMap<RenderNodeKey, RenderNodeId>,
}

impl RenderGraphBuilder<'_> {
    fn intern_node(&mut self, kind: RenderNodeKind, bounds: SurfaceRect) -> RenderNodeId {
        let key = kind.key(bounds, &self.output.nodes);
        if let Some(id) = self.node_cache.get(&key) {
            return *id;
        }

        let id = self.output.nodes.len();
        let previous = self.identities.previous.get(&key).copied();
        let identity = previous.unwrap_or_else(|| {
            self.identities.next = self
                .identities
                .next
                .checked_add(1)
                .expect("render node identity space exhausted");
            RenderNodeIdentity(self.identities.next)
        });
        self.output.nodes.push(RenderNode {
            identity,
            unchanged: previous.is_some(),
            kind,
            bounds,
        });
        self.node_cache.insert(key, id);
        id
    }

    fn encode_source(&mut self, item: &RenderItem) -> Result<RenderNodeId, RenderError> {
        let mut images = Vec::with_capacity(item.inputs.len());
        let mut image_bounds = Vec::with_capacity(item.inputs.len());
        for (index, input) in item.inputs.iter().enumerate() {
            let node = self.encode_node(input.node)?;
            images.push(ImageInput {
                node,
                binding: ImageBinding::named(index),
            });
            image_bounds.push((input.id.clone(), self.output.nodes[node].bounds));
        }
        let viewport = SurfaceRect::viewport(self.scene.composition_size);
        let bounds = item
            .output_bounds
            .evaluate(&BoundsContext {
                input: viewport,
                source: viewport,
                viewport,
                images: &image_bounds,
            })
            .map_err(|error| {
                RenderError::backend(format!("Item {:?} bounds: {error}", item.shader))
            })?;
        let kind = RenderNodeKind::Item {
            shader: item.shader.clone(),
            properties: item.properties.as_slice().into(),
            images,
        };
        Ok(self.intern_node(kind, bounds))
    }

    fn encode_frame(&mut self, frame: &Arc<VisualFrame>) -> RenderNodeId {
        let bounds = SurfaceRect::viewport(RenderSize {
            width: frame.width,
            height: frame.height,
        });
        self.intern_node(
            RenderNodeKind::Frame {
                frame: frame.clone(),
            },
            bounds,
        )
    }

    fn transparent(&mut self, bounds: SurfaceRect) -> RenderNodeId {
        self.intern_node(RenderNodeKind::Transparent, bounds)
    }

    fn encode_effect(
        &mut self,
        input: Option<RenderNodeId>,
        effect: &RenderEffect,
    ) -> Result<RenderNodeId, RenderError> {
        let mut node = input.unwrap_or_else(|| self.transparent(SurfaceRect::EMPTY));
        let mut capabilities = Vec::with_capacity(effect.inputs.len());
        let mut image_bounds = Vec::with_capacity(effect.inputs.len());
        for input in &effect.inputs {
            let node = self.encode_node(input.node)?;
            capabilities.push(node);
            image_bounds.push((input.id.clone(), self.output.nodes[node].bounds));
        }
        let mut temporal_inputs = Vec::new();
        if let Some(pass) = effect.passes.first()
            && let RenderEffectPassKind::Temporal(samples) = &pass.kind
        {
            for sample in samples {
                temporal_inputs.push((
                    sample
                        .input
                        .map(|input| self.encode_node(input))
                        .transpose()?,
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
            let (input_bounds, source_bounds) = if let RenderEffectPassKind::Temporal(_) = pass.kind
            {
                (basis, basis)
            } else {
                (
                    self.output.nodes[node].bounds,
                    self.output.nodes[source].bounds,
                )
            };
            let bounds = pass
                .output_bounds
                .evaluate(&BoundsContext {
                    input: input_bounds,
                    source: source_bounds,
                    viewport,
                    images: &image_bounds,
                })
                .map_err(|error| {
                    RenderError::backend(format!("Effect pass {:?} bounds: {error}", pass.shader))
                })?;

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
                            PassKind::Render,
                            temporal,
                            bounds,
                        );
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
                    )
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
                    )
                }
            }
        }

        Ok(node)
    }

    fn encode_pass(
        &mut self,
        inputs: PassInputs<'_>,
        pass: &crate::engine::processing::visual::scene::RenderEffectPass,
        kind: PassKind,
        temporal: [u32; 4],
        bounds: SurfaceRect,
    ) -> RenderNodeId {
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
        let pass = ShaderPass {
            shader: pass.shader.clone(),
            kind,
            properties: pass.properties.as_slice().into(),
            temporal,
        };
        self.intern_node(RenderNodeKind::Pass { images, pass }, bounds)
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
        Ok(self.intern_node(RenderNodeKind::Composite { children }, bounds))
    }

    fn encode_node(&mut self, id: SceneNodeId) -> Result<RenderNodeId, RenderError> {
        if let Some(plan) = self.encoded_nodes[id] {
            return Ok(plan);
        }

        let node = &self.scene.nodes[id];
        let plan = match &node.content {
            RenderNodeContent::Frame(frame) => self.encode_frame(frame),
            RenderNodeContent::Item(item) => self.encode_source(item)?,
            RenderNodeContent::Scene { children } => self.encode_composite(children)?,
            RenderNodeContent::Effect { input, effect } => {
                let input = input.map(|input| self.encode_node(input)).transpose()?;
                self.encode_effect(input, effect)?
            }
            RenderNodeContent::View { input, view } => {
                let input = self.encode_node(*input)?;
                let bounds = SurfaceRect::viewport(self.scene.composition_size);
                self.intern_node(RenderNodeKind::View { input, view: *view }, bounds)
            }
        };
        self.encoded_nodes[id] = Some(plan);
        Ok(plan)
    }
}

pub(super) fn build_render_graph(
    scene: &RenderScene,
    identities: &mut NodeIdentityCache,
) -> Result<RenderGraph, RenderError> {
    let mut encoder = RenderGraphBuilder {
        identities,
        scene,
        output: RenderGraph {
            background: scene.background,
            ..RenderGraph::default()
        },
        encoded_nodes: vec![None; scene.nodes.len()],
        node_cache: HashMap::new(),
    };
    for node in &scene.roots {
        let root = encoder.encode_node(*node)?;
        encoder.output.roots.push(RenderRoot {
            layer: RenderLayer {
                node: root,
                blend_mode: scene.nodes[*node].blend_mode,
            },
            render_scale: scene.nodes[*node].render_scale,
        });
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
        .map(|(key, node)| (key, encoder.output.nodes[node].identity))
        .collect();
    Ok(encoder.output)
}
