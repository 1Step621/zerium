use super::composition::{CaptureDefinition, SceneCompositionPlan};
use super::demand::{ImageDemand, MediaDemandGraph, PlannedImage, SourceFrame};
use super::geometry::BoundsOperation;
use super::text::TextFrameRequest;
use super::{
    RenderEffect, RenderEffectPass, RenderEffectPassKind, RenderError, RenderInput, RenderItem,
    RenderNode, RenderNodeContent, RenderScene, RenderSize, RenderTemporalSample, RenderView,
    SceneNodeId,
};
use crate::engine::{
    frame::VisualFrame,
    media::{MediaFrameRequest, MediaInputId, RasterSize, SamplingBranch},
};
use std::{collections::HashMap, sync::Arc};
use zerium_core::media::MediaMetadataCache;
use zerium_core::plugin::{EffectPassSchema, PlaybackBindings, TextureInput};
use zerium_core::property::PropertyValues;
use zerium_core::timeline::{
    BlendMode, CalculationDiagnostic, EffectInstance, EffectInstanceId, ItemId, ProjectResolution,
    RenderEvaluation, RenderPlacement, RenderPropertyTarget, ResolvedRenderProperties,
    TimelineItemKind, TimelineTime, TimelineView,
};
use zerium_shader::{EffectShaderId, ItemShaderId};

const MAX_TEMPORAL_DEPTH: usize = 4;
const MAX_TEMPORAL_SAMPLES: usize = 4_096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ImageStage {
    Output,
    BeforeEffect(EffectInstanceId),
}

/// An image's semantic request. Time is local to the target, never an offset
/// from the playhead. Decoder scheduling is deliberately absent from its key.
/// Its interval is always the target's own expanded duration, without clipping
/// by ancestors; normal output is gated at each composition boundary.
#[derive(Clone)]
struct ImageRequest {
    path: Vec<ItemId>,
    stage: ImageStage,
    time: TimelineTime,
    scale: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct StageLimit {
    path: Vec<ItemId>,
    before: EffectInstanceId,
}

#[derive(Clone, Default)]
struct RenderContext {
    temporal_depth: usize,
    limits: Vec<StageLimit>,
}

impl RenderContext {
    fn for_sampling(&self, path: &[ItemId], effect: EffectInstanceId) -> Result<Self, RenderError> {
        if self.temporal_depth >= MAX_TEMPORAL_DEPTH {
            return Err(RenderError::resource_limit(format!(
                "temporal effect depth exceeds {MAX_TEMPORAL_DEPTH}"
            )));
        }
        let mut sampled = self.clone();
        sampled.temporal_depth += 1;
        if let Some(limit) = sampled.limits.iter_mut().find(|limit| limit.path == path) {
            limit.before = effect;
        } else {
            sampled.limits.push(StageLimit {
                path: path.to_vec(),
                before: effect,
            });
        }
        Ok(sampled)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ImageKey {
    path: Vec<ItemId>,
    stage: ImageStage,
    time_bits: u64,
    scale: u32,
    limits: Vec<StageLimit>,
}

enum ImageState {
    Resolving,
    Planned(PlannedImage),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CompositionKey {
    path: Vec<ItemId>,
    time_bits: u64,
    limits: Vec<StageLimit>,
}

/// An item's or effect's texture inputs and their shared property values.
struct InputSource<'a> {
    item_id: ItemId,
    effect_id: Option<EffectInstanceId>,
    inputs: &'a [TextureInput],
    properties: &'a PropertyValues,
    playback: &'a PlaybackBindings,
    label: &'a str,
}

impl<'a> InputSource<'a> {
    fn item(placement: &'a RenderPlacement, properties: &'a PropertyValues) -> Self {
        let schema = placement
            .source
            .schema()
            .expect("renderable item has a schema");
        Self {
            item_id: placement.runtime_id,
            effect_id: None,
            inputs: schema.inputs(),
            properties,
            playback: schema.playback(),
            label: schema.label(),
        }
    }

    fn effect(item_id: ItemId, effect: &'a EffectInstance, properties: &'a PropertyValues) -> Self {
        let schema = effect.schema();
        Self {
            item_id,
            effect_id: Some(effect.id),
            inputs: schema.inputs(),
            properties,
            playback: schema.playback(),
            label: schema.label(),
        }
    }

    fn media_request(
        &self,
        input: &TextureInput,
        stream: MediaInputId,
        local_seconds: f64,
        metadata: &MediaMetadataCache,
        size: RasterSize,
    ) -> Option<MediaFrameRequest<MediaInputId>> {
        let asset = input.media_source()?.asset(self.properties, metadata)?;
        let playback = self
            .playback
            .media(input.id(), self.properties)
            .expect("validated media playback properties");
        MediaFrameRequest::new(stream, asset, playback, local_seconds, size)
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

/// Resolves only the property stages and images reached from an output request.
pub(super) struct SceneGraphBuilder<'a, T> {
    timeline: &'a dyn TimelineView,
    evaluation: RenderEvaluation<'a>,
    strict_calculations: bool,
    size: RenderSize,
    nodes: Vec<RenderNode<SourceFrame>>,
    properties: HashMap<(Vec<ItemId>, u64, RenderPropertyTarget), Arc<ResolvedRenderProperties>>,
    compositions: HashMap<CompositionKey, Arc<SceneCompositionPlan>>,
    node_cache: HashMap<ImageKey, ImageState>,
    demands: MediaDemandGraph,
    samples_remaining: usize,
    text_frame: T,
}

impl<T> SceneGraphBuilder<'_, T>
where
    T: FnMut(TextFrameRequest<'_>) -> Result<Arc<VisualFrame>, RenderError>,
{
    pub(super) fn build<E: From<RenderError>>(
        timeline: &dyn TimelineView,
        time: TimelineTime,
        size: RenderSize,
        strict_calculations: bool,
        media_frame: impl FnMut(MediaFrameRequest) -> Result<Option<Arc<VisualFrame>>, E>,
        text_frame: T,
    ) -> Result<RenderScene, E> {
        let mut builder = SceneGraphBuilder {
            timeline,
            evaluation: timeline.render_evaluation(),
            strict_calculations,
            size,
            nodes: Vec::new(),
            properties: HashMap::new(),
            compositions: HashMap::new(),
            node_cache: HashMap::new(),
            demands: MediaDemandGraph::default(),
            samples_remaining: MAX_TEMPORAL_SAMPLES,
            text_frame,
        };
        let context = RenderContext::default();
        let composition = builder
            .composition(&[], time, &context)?
            .expect("root composition exists");
        let mut demand = ImageDemand::default();
        let roots = builder.render_scope(&composition, time, 1, &context, &mut demand)?;
        Ok(RenderScene::from_graph(
            size,
            RenderSize::from(timeline.resolution()),
            builder.demands.bind(
                demand,
                builder.nodes,
                MAX_TEMPORAL_DEPTH,
                MAX_TEMPORAL_SAMPLES,
                media_frame,
            )?,
            roots,
        ))
    }

    fn check_diagnostics(
        &self,
        path: &[ItemId],
        time: TimelineTime,
        diagnostics: &[CalculationDiagnostic],
    ) -> Result<(), RenderError> {
        if self.strict_calculations
            && let Some(error) = diagnostics.first()
        {
            return Err(RenderError::Calculation {
                time,
                path: path.to_vec(),
                diagnostic: Box::new(error.clone()),
            });
        }
        Ok(())
    }

    fn properties(
        &mut self,
        path: &[ItemId],
        time: TimelineTime,
        target: RenderPropertyTarget,
    ) -> Result<Arc<ResolvedRenderProperties>, RenderError> {
        let key = (path.to_vec(), time.frames().to_bits(), target);
        let resolved = self
            .properties
            .entry(key)
            .or_insert_with(|| {
                Arc::new(
                    self.evaluation
                        .properties(path, time, target)
                        .expect("requested placement exists"),
                )
            })
            .clone();
        self.check_diagnostics(path, time, &resolved.diagnostics)?;
        Ok(resolved)
    }

    fn composition(
        &mut self,
        path: &[ItemId],
        time: TimelineTime,
        context: &RenderContext,
    ) -> Result<Option<Arc<SceneCompositionPlan>>, RenderError> {
        let key = CompositionKey {
            path: path.to_vec(),
            time_bits: time.frames().to_bits(),
            limits: context.limits.clone(),
        };
        if let Some(plan) = self.compositions.get(&key) {
            return Ok(Some(plan.clone()));
        }
        let Some(placements) = self.evaluation.composition(path, time) else {
            return Ok(None);
        };
        let mut captures = Vec::new();
        for placement in &placements {
            let before = context
                .limits
                .iter()
                .find(|l| l.path == placement.path)
                .map(|l| l.before);
            let source = &placement.source;
            let end = before.map_or(source.effects.len(), |id| {
                source
                    .effects
                    .iter()
                    .position(|effect| effect.id == id)
                    .expect("capture boundary exists")
            });
            let owners = source
                .schema()
                .map(|schema| (None, schema.inputs()))
                .into_iter()
                .chain(
                    source.effects[..end]
                        .iter()
                        .map(|effect| (Some(effect.id), effect.schema().inputs())),
                );
            for (effect, inputs) in owners {
                if !inputs
                    .iter()
                    .any(|input| matches!(input, TextureInput::RenderResult { .. }))
                {
                    continue;
                }
                let resolved = self.properties(
                    &placement.path,
                    time,
                    RenderPropertyTarget::CaptureTopology { effect },
                )?;
                captures.extend(inputs.iter().filter_map(|input| {
                    CaptureDefinition::resolve(
                        placement.runtime_id,
                        effect,
                        placement.local_layer,
                        input,
                        &resolved.values,
                    )
                }));
            }
        }
        let plan = Arc::new(SceneCompositionPlan::new(placements, captures));
        self.compositions.insert(key, plan.clone());
        Ok(Some(plan))
    }

    fn render_scope(
        &mut self,
        composition: &SceneCompositionPlan,
        time: TimelineTime,
        scale: u32,
        context: &RenderContext,
        demand: &mut ImageDemand,
    ) -> Result<Vec<SceneNodeId>, RenderError> {
        let mut children = Vec::new();
        for placement in composition.normal_nodes() {
            let request = ImageRequest {
                path: placement.path.clone(),
                stage: ImageStage::Output,
                time: placement.local_time(time),
                scale,
            };
            if let Some(id) = demand.input(self.image(request, context)?) {
                children.push(id);
            }
        }
        Ok(children)
    }

    fn image(
        &mut self,
        request: ImageRequest,
        context: &RenderContext,
    ) -> Result<PlannedImage, RenderError> {
        let key = ImageKey {
            path: request.path.clone(),
            stage: request.stage,
            time_bits: request.time.frames().to_bits(),
            scale: request.scale,
            limits: context.limits.clone(),
        };
        match self.node_cache.get(&key) {
            Some(ImageState::Planned(image)) => return Ok(*image),
            Some(ImageState::Resolving) => {
                return Err(RenderError::resource_limit("cyclic image dependency"));
            }
            None => {}
        }
        self.node_cache.insert(key.clone(), ImageState::Resolving);
        let mut demand = ImageDemand::default();
        let node = self.resolve_image(&request, context, &mut demand)?;
        let image = self.demands.image(node, demand);
        self.node_cache.insert(key, ImageState::Planned(image));
        Ok(image)
    }

    fn resolve_image(
        &mut self,
        request: &ImageRequest,
        context: &RenderContext,
        demand: &mut ImageDemand,
    ) -> Result<Option<SceneNodeId>, RenderError> {
        let Some(placement) = self.evaluation.placement(&request.path) else {
            return Ok(None);
        };
        let time = placement.global_time(request.time);
        if !placement.contains(time) {
            return Ok(None);
        }
        let effects = &placement.source.effects;
        let mut end = match request.stage {
            ImageStage::Output => effects.len(),
            ImageStage::BeforeEffect(id) => effects
                .iter()
                .position(|e| e.id == id)
                .expect("effect boundary exists"),
        };
        if let Some(limit) = context.limits.iter().find(|l| l.path == request.path) {
            end = end.min(
                effects
                    .iter()
                    .position(|e| e.id == limit.before)
                    .expect("inherited effect boundary exists"),
            );
        }
        if let Some(effect) = effects[..end].last() {
            let resolved =
                self.properties(&request.path, time, RenderPropertyTarget::Effect(effect.id))?;
            let schema = effect.schema();
            let scale = request.scale.max(schema.render().scale);
            let mut upstream = request.clone();
            upstream.stage = ImageStage::BeforeEffect(effect.id);
            upstream.scale = scale;
            let mut passes = Vec::new();
            let first = schema.render().passes.first();
            let input = match first {
                Some(EffectPassSchema::Sample { sampling }) => {
                    let sample_time = sampling
                        .sample_time(
                            &resolved.values,
                            request.time.frames(),
                            placement.source.duration,
                        )
                        .ok_or_else(|| RenderError::backend("invalid source sample time"))?;
                    let sampled_context = context.for_sampling(&request.path, effect.id)?;
                    let input = self.sample_image(
                        &upstream,
                        sample_time,
                        effect.id,
                        &sampled_context,
                        demand,
                    )?;
                    // A pure sample is an alias, with no logical effect node or shader ABI.
                    if schema.render().passes.len() == 1 {
                        return Ok(input);
                    }
                    input
                }
                Some(EffectPassSchema::Temporal { .. }) => None,
                _ => demand.input(self.image(upstream.clone(), context)?),
            };
            let properties = schema
                .property_layout()
                .pack("effect", schema.id(), &resolved.values)
                .expect("effect property contract");
            for (pass_index, pass) in schema.render().passes.iter().enumerate() {
                let kind = match pass {
                    EffectPassSchema::Sample { .. } => continue,
                    EffectPassSchema::Render { .. } => RenderEffectPassKind::Render,
                    EffectPassSchema::Compute { dispatch, .. } => {
                        RenderEffectPassKind::Compute(*dispatch)
                    }
                    EffectPassSchema::Temporal { sampling, .. } => {
                        let times = sampling
                            .sample_times(&resolved.values, request.time.frames())
                            .ok_or_else(|| RenderError::backend("invalid temporal source times"))?;
                        RenderEffectPassKind::Temporal(
                            self.sample_images(&upstream, effect.id, &times, context, demand)?,
                        )
                    }
                };
                passes.push(RenderEffectPass {
                    shader: EffectShaderId::plugin_pass(
                        &effect.plugin_id,
                        &effect.effect_id,
                        pass_index,
                    ),
                    properties: properties.clone(),
                    kind,
                    output_bounds: BoundsOperation::from_schema(
                        pass.bounds().expect("shader pass bounds"),
                        &resolved.values,
                        pass.overrides().expect("shader pass overrides"),
                    ),
                });
            }
            let inputs = self.render_inputs(
                InputSource::effect(placement.runtime_id, effect, &resolved.values),
                &placement,
                time,
                context,
                scale,
                demand,
            )?;
            let id = self.push_node(
                RenderNodeContent::Effect {
                    input,
                    effect: RenderEffect { passes, inputs },
                    render_scale: scale,
                },
                placement.source.blend_mode,
            );
            return Ok(Some(id));
        }
        let content = match &placement.source.kind {
            TimelineItemKind::Plugin {
                plugin_id,
                item_id,
                schema,
            } => {
                let Some(render) = schema.render() else {
                    return Ok(None);
                };
                let resolved = self.properties(&request.path, time, RenderPropertyTarget::Item)?;
                let inputs = self.render_inputs(
                    InputSource::item(&placement, &resolved.values),
                    &placement,
                    time,
                    context,
                    request.scale,
                    demand,
                )?;
                RenderNodeContent::Item(RenderItem {
                    shader: ItemShaderId::plugin_item(plugin_id, item_id),
                    inputs,
                    properties: render
                        .property_layout()
                        .pack("item", schema.id(), &resolved.values)
                        .expect("item property contract"),
                    render_scale: request.scale,
                    output_bounds: BoundsOperation::from_schema(
                        &render.bounds,
                        &resolved.values,
                        &Default::default(),
                    ),
                })
            }
            TimelineItemKind::Scene { .. } => {
                let composition = self
                    .composition(&request.path, time, context)?
                    .expect("scene composition exists");
                let children =
                    self.render_scope(&composition, time, request.scale, context, demand)?;
                RenderNodeContent::Scene {
                    children,
                    render_scale: request.scale,
                }
            }
        };
        Ok(Some(self.push_node(content, placement.source.blend_mode)))
    }

    fn sample_images(
        &mut self,
        upstream: &ImageRequest,
        effect: EffectInstanceId,
        times: &[f64],
        context: &RenderContext,
        demand: &mut ImageDemand,
    ) -> Result<Vec<RenderTemporalSample>, RenderError> {
        let sample_context = context.for_sampling(&upstream.path, effect)?;
        let mut samples = Vec::with_capacity(times.len());
        for &time in times {
            samples.push(RenderTemporalSample {
                frame_offset: (time - upstream.time.frames()) as f32,
                input: self.sample_image(upstream, time, effect, &sample_context, demand)?,
            });
        }
        Ok(samples)
    }

    fn sample_image(
        &mut self,
        upstream: &ImageRequest,
        time: f64,
        effect: EffectInstanceId,
        context: &RenderContext,
        demand: &mut ImageDemand,
    ) -> Result<Option<SceneNodeId>, RenderError> {
        self.samples_remaining = self.samples_remaining.checked_sub(1).ok_or_else(|| {
            RenderError::resource_limit(format!(
                "render graph exceeds {MAX_TEMPORAL_SAMPLES} temporal samples"
            ))
        })?;
        let mut request = upstream.clone();
        request.time = TimelineTime::from_frames(time);
        Ok(demand.sample(
            self.image(request, context)?,
            SamplingBranch {
                path: upstream.path.clone(),
                effect,
            },
        ))
    }

    fn scaled_size(&self, scale: u32) -> Result<RenderSize, RenderError> {
        self.size
            .checked_scale(scale)
            .ok_or_else(|| RenderError::resource_limit("render size overflows"))
    }

    fn content_scale(&self, content: &RenderNodeContent<SourceFrame>) -> u32 {
        let scale_for = |id: SceneNodeId| self.nodes[id].render_scale;
        match content {
            RenderNodeContent::Item(item) => item
                .inputs
                .iter()
                .map(|input| scale_for(input.node))
                .fold(item.render_scale, u32::max),
            RenderNodeContent::Scene {
                children,
                render_scale,
            } => children
                .iter()
                .copied()
                .map(scale_for)
                .fold(*render_scale, u32::max),
            RenderNodeContent::Effect {
                input,
                effect,
                render_scale,
            } => input
                .iter()
                .copied()
                .chain(effect.inputs.iter().map(|input| input.node))
                .chain(
                    effect
                        .passes
                        .iter()
                        .filter_map(|pass| match &pass.kind {
                            RenderEffectPassKind::Temporal(samples) => Some(samples),
                            _ => None,
                        })
                        .flatten()
                        .filter_map(|sample| sample.input),
                )
                .map(scale_for)
                .fold(*render_scale, u32::max),
            RenderNodeContent::Frame(_) => 1,
            RenderNodeContent::View { input, .. } => scale_for(*input).max(1),
        }
    }

    fn push_node(
        &mut self,
        content: RenderNodeContent<SourceFrame>,
        blend_mode: BlendMode,
    ) -> SceneNodeId {
        let render_scale = self.content_scale(&content);
        let id = self.nodes.len();
        self.nodes.push(RenderNode {
            content,
            render_scale,
            blend_mode,
        });
        id
    }

    fn render_inputs(
        &mut self,
        source: InputSource<'_>,
        placement: &RenderPlacement,
        time: TimelineTime,
        context: &RenderContext,
        scale: u32,
        demand: &mut ImageDemand,
    ) -> Result<Vec<RenderInput>, RenderError> {
        let target_size = self.scaled_size(scale)?;
        let mut inputs = Vec::with_capacity(source.inputs.len());
        for capability in source.inputs {
            let input = match capability {
                TextureInput::Media { .. } => {
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
                    let request = source.media_request(
                        capability,
                        input,
                        placement.local_time(time).frames()
                            / self.timeline.frame_rate().frames_per_second(),
                        self.timeline.media_cache(),
                        RasterSize {
                            max_width: size.width,
                            max_height: size.height,
                        },
                    );
                    let frame = match request {
                        Some(request) => self.demands.media(demand, request),
                        None => SourceFrame::Empty,
                    };
                    self.push_node(RenderNodeContent::Frame(frame), BlendMode::Normal)
                }
                TextureInput::Text { .. } | TextureInput::Number { .. } => {
                    let frame = (self.text_frame)(TextFrameRequest {
                        capability,
                        properties: source.properties,
                        label: source.label,
                        target_size,
                    })?;
                    self.frame_node(frame)
                }
                TextureInput::RenderResult { view, .. } => {
                    let scope = &placement.path[..placement.path.len() - 1];
                    let composition = self
                        .composition(scope, time, context)?
                        .expect("capture composition exists");
                    let capture =
                        composition.capture(source.item_id, source.effect_id, capability.id());
                    let mut children = Vec::new();
                    for placement in composition.captured_nodes(capture) {
                        let request = ImageRequest {
                            path: placement.path.clone(),
                            stage: ImageStage::Output,
                            time: placement.local_time(time),
                            scale,
                        };
                        if let Some(child) = demand.input(self.image(request, context)?) {
                            children.push(child);
                        }
                    }
                    let input = self.push_node(
                        RenderNodeContent::Scene {
                            children,
                            render_scale: scale,
                        },
                        BlendMode::Normal,
                    );
                    view.as_ref().map_or(input, |view| {
                        let view = RenderView::from_properties(view, source.properties);
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
        self.push_node(
            RenderNodeContent::Frame(SourceFrame::Ready(frame)),
            BlendMode::Normal,
        )
    }
}
