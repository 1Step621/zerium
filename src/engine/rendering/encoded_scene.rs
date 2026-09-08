use super::scene::EffectParams;
use super::*;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuItem {
    pub(super) params_offset: u32,
    pub(super) params_size: u32,
    pub(super) output_size: [f32; 2],
    pub(super) composition_size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuEffect {
    pub(super) params_offset: u32,
    pub(super) params_size: u32,
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
    pub(super) params_offset: u32,
    pub(super) params_size: u32,
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
        node: RenderNodeCommand,
        render_scale: u32,
    },
}

#[derive(Debug, PartialEq)]
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
        children: Vec<RenderNodeCommand>,
    },
    Effect {
        input: Box<RenderNodeCommand>,
        passes: Vec<EffectPassCommand>,
    },
    TemporalEffect {
        // Every branch includes the effects preceding this temporal effect.
        samples: Vec<(RenderNodeCommand, TemporalReduceCommand)>,
    },
}

impl RenderNodeCommand {
    pub(super) fn temporal_depth(&self) -> usize {
        match &self.kind {
            RenderNodeCommandKind::Source(_) => 0,
            RenderNodeCommandKind::Composite { children } => {
                children.iter().map(Self::temporal_depth).max().unwrap_or(0)
            }
            RenderNodeCommandKind::Effect { input, .. } => input.temporal_depth(),
            RenderNodeCommandKind::TemporalEffect { samples, .. } => {
                1 + samples
                    .iter()
                    .map(|(sample, _)| sample.temporal_depth())
                    .max()
                    .unwrap_or(0)
            }
        }
    }

    pub(super) fn composition_depth(&self) -> usize {
        match &self.kind {
            RenderNodeCommandKind::Source(_) => 0,
            RenderNodeCommandKind::Composite { children } => {
                1 + children
                    .iter()
                    .map(Self::composition_depth)
                    .max()
                    .unwrap_or(0)
            }
            RenderNodeCommandKind::Effect { input, .. } => input.composition_depth(),
            RenderNodeCommandKind::TemporalEffect { samples } => samples
                .iter()
                .map(|(sample, _)| sample.composition_depth())
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
    pub(super) params_offset: u32,
    pub(super) params_size: u32,
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
    pub(super) params: Vec<u8>,
    pub(super) effects: Vec<GpuEffect>,
    pub(super) effect_params: Vec<u8>,
    pub(super) textures: Vec<EncodedTexture>,
    pub(super) commands: Vec<RenderCommand>,
}

pub(super) struct EncodedTexture {
    pub(super) shader: TextureShaderId,
    pub(super) frames: Vec<Arc<RgbaFrame>>,
    pub(super) params: ItemParams,
    pub(super) target_size: RenderSize,
    pub(super) composition_size: RenderSize,
}

fn encode_effect_pass(
    shader: &EffectShaderId,
    params: &EffectParams,
    kind: EffectPassCommandKind,
    captures_source: bool,
    effects: &mut Vec<GpuEffect>,
    effect_params: &mut Vec<u8>,
    composition_size: RenderSize,
) -> Result<EffectPassCommand, RenderError> {
    let effect_instance = u32::try_from(effects.len())
        .map_err(|_| RenderError::backend("too many effect passes in one frame"))?;
    let params_offset = u32::try_from(effect_params.len() / PARAM_WORD_SIZE)
        .map_err(|_| RenderError::backend("effect parameter offset exceeds u32"))?;
    let params_size = u32::try_from(params.len())
        .map_err(|_| RenderError::backend("effect parameter size exceeds u32"))?;
    effect_params.extend_from_slice(params.as_bytes());
    effect_params.resize(effect_params.len().next_multiple_of(PARAM_WORD_SIZE), 0);
    effects.push(GpuEffect {
        params_offset,
        params_size,
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
        params_offset,
        params_size,
    })
}

struct EncodeContext<'a> {
    items: &'a mut Vec<GpuItem>,
    params: &'a mut Vec<u8>,
    effects: &'a mut Vec<GpuEffect>,
    effect_params: &'a mut Vec<u8>,
    textures: &'a mut Vec<EncodedTexture>,
    composition_size: RenderSize,
}

impl EncodeContext<'_> {
    fn encode_source(&mut self, item: &RenderItem) -> Result<RenderSourceCommand, RenderError> {
        match item {
            RenderItem::Shader(item) => {
                let instance = u32::try_from(self.items.len())
                    .map_err(|_| RenderError::backend("too many visible items in one frame"))?;
                let params_offset = u32::try_from(self.params.len() / PARAM_WORD_SIZE)
                    .map_err(|_| RenderError::backend("item parameter offset exceeds u32"))?;
                let params_size = u32::try_from(item.params.len())
                    .map_err(|_| RenderError::backend("item parameter size exceeds u32"))?;
                self.params.extend_from_slice(item.params.as_bytes());
                self.params
                    .resize(self.params.len().next_multiple_of(PARAM_WORD_SIZE), 0);
                self.items.push(GpuItem {
                    params_offset,
                    params_size,
                    output_size: [
                        item.target_size.width as f32,
                        item.target_size.height as f32,
                    ],
                    composition_size: [
                        self.composition_size.width as f32,
                        self.composition_size.height as f32,
                    ],
                });
                Ok(RenderSourceCommand::Item {
                    shader: item.shader.clone(),
                    instance,
                })
            }
            RenderItem::Texture(item) => {
                let index = self.textures.len();
                self.textures.push(EncodedTexture {
                    shader: item.shader.clone(),
                    frames: item.frames.clone(),
                    params: item.params.clone(),
                    target_size: item.target_size,
                    composition_size: self.composition_size,
                });
                Ok(RenderSourceCommand::Texture {
                    index,
                    shader: item.shader.clone(),
                })
            }
        }
    }

    fn temporal_reduce_command(
        &mut self,
        reducer: &EffectShaderId,
        params: &EffectParams,
        sample_count: usize,
        sample_index: usize,
        sample: &RenderTemporalSample,
    ) -> Result<TemporalReduceCommand, RenderError> {
        let instance = u32::try_from(self.effects.len())
            .map_err(|_| RenderError::backend("too many effect passes in one frame"))?;
        let params_offset = u32::try_from(self.effect_params.len() / PARAM_WORD_SIZE)
            .map_err(|_| RenderError::backend("effect parameter offset exceeds u32"))?;
        self.effect_params.extend_from_slice(params.as_bytes());
        self.effect_params.resize(
            self.effect_params.len().next_multiple_of(PARAM_WORD_SIZE),
            0,
        );
        let params_size = u32::try_from(params.len())
            .map_err(|_| RenderError::backend("effect parameter size exceeds u32"))?;
        let sample_index = u32::try_from(sample_index)
            .map_err(|_| RenderError::backend("temporal sample index exceeds u32"))?;
        let sample_count = u32::try_from(sample_count)
            .map_err(|_| RenderError::backend("temporal sample count exceeds u32"))?;
        self.effects.push(GpuEffect {
            params_offset,
            params_size,
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
        mut node: RenderNodeCommand,
        effects: &[RenderEffect],
    ) -> Result<RenderNodeCommand, RenderError> {
        for effect in effects {
            let mut regular_passes = Vec::new();
            for pass in &effect.passes {
                match pass {
                    RenderEffectPass::Temporal {
                        reducer,
                        params,
                        samples,
                    } => {
                        debug_assert!(regular_passes.is_empty());
                        let sample_count = samples.len();
                        let samples = samples
                            .iter()
                            .enumerate()
                            .map(|(sample_index, sample)| {
                                let node = match &sample.input {
                                    Some(sample) => self.encode_node(sample)?,
                                    None => RenderNodeCommand {
                                        metadata: node.metadata.clone(),
                                        kind: RenderNodeCommandKind::Source(
                                            RenderSourceCommand::Transparent,
                                        ),
                                    },
                                };
                                Ok((
                                    node,
                                    self.temporal_reduce_command(
                                        reducer,
                                        params,
                                        sample_count,
                                        sample_index,
                                        sample,
                                    )?,
                                ))
                            })
                            .collect::<Result<Vec<_>, RenderError>>()?;
                        node = RenderNodeCommand {
                            metadata: node.metadata.clone(),
                            kind: RenderNodeCommandKind::TemporalEffect { samples },
                        };
                    }
                    RenderEffectPass::Render { shader, params } => {
                        let starts_regular_chain = regular_passes.is_empty();
                        regular_passes.push(encode_effect_pass(
                            shader,
                            params,
                            EffectPassCommandKind::Render,
                            starts_regular_chain,
                            self.effects,
                            self.effect_params,
                            self.composition_size,
                        )?);
                    }
                    RenderEffectPass::Compute {
                        shader,
                        params,
                        dispatch,
                    } => {
                        let starts_regular_chain = regular_passes.is_empty();
                        regular_passes.push(encode_effect_pass(
                            shader,
                            params,
                            EffectPassCommandKind::Compute {
                                dispatch: *dispatch,
                            },
                            starts_regular_chain,
                            self.effects,
                            self.effect_params,
                            self.composition_size,
                        )?);
                    }
                }
            }
            if !regular_passes.is_empty() {
                node = RenderNodeCommand {
                    metadata: node.metadata.clone(),
                    kind: RenderNodeCommandKind::Effect {
                        input: Box::new(node),
                        passes: regular_passes,
                    },
                };
            }
        }
        Ok(node)
    }

    fn encode_node(&mut self, node: &RenderNode) -> Result<RenderNodeCommand, RenderError> {
        match &node.content {
            RenderNodeContent::Item(item) => {
                let effects = match item {
                    RenderItem::Shader(item) => &item.effects,
                    RenderItem::Texture(item) => &item.effects,
                };
                let source = RenderNodeCommand {
                    metadata: node.metadata.clone(),
                    kind: RenderNodeCommandKind::Source(self.encode_source(item)?),
                };
                self.encode_effects(source, effects)
            }
            RenderNodeContent::Scene {
                children, effects, ..
            } => {
                let children = children
                    .iter()
                    .map(|child| self.encode_node(child))
                    .collect::<Result<Vec<_>, _>>()?;
                let composite = RenderNodeCommand {
                    metadata: node.metadata.clone(),
                    kind: RenderNodeCommandKind::Composite { children },
                };
                self.encode_effects(composite, effects)
            }
        }
    }
}

pub(super) fn encode_items(scene: &RenderScene) -> Result<EncodedScene, RenderError> {
    let mut items = Vec::new();
    let mut params = Vec::new();
    let mut effects = Vec::new();
    let mut effect_params = Vec::new();
    let mut textures = Vec::new();
    let mut commands: Vec<RenderCommand> = Vec::new();

    for node in &scene.roots {
        let (has_effects, item) = match &node.content {
            RenderNodeContent::Item(RenderItem::Shader(item)) => (
                !item.effects.is_empty(),
                Some(RenderItem::Shader(item.clone())),
            ),
            RenderNodeContent::Item(RenderItem::Texture(item)) => (
                !item.effects.is_empty(),
                Some(RenderItem::Texture(item.clone())),
            ),
            RenderNodeContent::Scene { effects, .. } => (!effects.is_empty(), None),
        };
        let mut context = EncodeContext {
            items: &mut items,
            params: &mut params,
            effects: &mut effects,
            effect_params: &mut effect_params,
            textures: &mut textures,
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
        match context.encode_source(item)? {
            RenderSourceCommand::Item { shader, instance } => match commands.last_mut() {
                Some(RenderCommand::Items(batch)) if batch.shader == shader => {
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
            RenderSourceCommand::Transparent => unreachable!("scene items are never transparent"),
        }
    }

    Ok(EncodedScene {
        items,
        params,
        effects,
        effect_params,
        textures,
        commands,
    })
}
