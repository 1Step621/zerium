mod evaluation;
mod geometry;
mod graph;
mod text;

use crate::engine::frame::VisualFrame;
use crate::engine::media::MediaFrameRequest;
pub(crate) use evaluation::SceneEvaluator;
pub(super) use geometry::{BoundsOperation, SurfaceRect};
use graph::SceneGraphBuilder;
use std::sync::Arc;
use text::TextFrameRequest;
use thiserror::Error;
use zerium_core::plugin::{
    ComputeDispatchDimension, EffectInputSpace, EffectPassSchema, ItemSchema, TextureInput,
};
use zerium_core::timeline::{
    BlendMode, EffectInstance, EffectInstanceId, ProjectResolution, TimelineItem, TimelineTime,
    TimelineView,
};
use zerium_shader::{EffectShaderId, ItemShaderId};

pub(super) type SceneNodeId = usize;

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
    Texture(Vec<Arc<VisualFrame>>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderItem {
    pub shader: ItemShaderId,
    pub source: RenderItemSource,
    pub inputs: Vec<SceneNodeId>,
    pub properties: Vec<u8>,
    pub effects: Vec<RenderEffect>,
    pub target_size: RenderSize,
    pub render_scale: u32,
    pub output_bounds: SurfaceRect,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderEffect {
    pub passes: Vec<RenderEffectPass>,
    pub inputs: Vec<SceneNodeId>,
    pub output_bounds: BoundsOperation,
    pub input_space: EffectInputSpace,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderTemporalSample {
    pub frame_offset: f32,
    /// A missing node is an intentionally transparent sample at a clip boundary.
    pub input: Option<SceneNodeId>,
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

/// View of a composition: position is its focal point, zoom is a scale factor,
/// and angle is the camera's clockwise rotation in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RenderView {
    pub position: [f32; 2],
    pub zoom: f32,
    pub angle: f32,
}

impl Default for RenderView {
    fn default() -> Self {
        Self {
            position: [0.; 2],
            zoom: 1.,
            angle: 0.,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RenderNodeContent {
    Item(RenderItem),
    Scene {
        children: Vec<SceneNodeId>,
        view: RenderView,
        effects: Vec<RenderEffect>,
        render_scale: u32,
    },
}

/// A scene-linear compositing node. Scene-instance effects belong to the
/// `Scene` node and are therefore applied once after all children are blended.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderNode {
    pub content: RenderNodeContent,
    pub render_scale: u32,
    /// Used only when this completed node is composited into its parent scene.
    pub blend_mode: BlendMode,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderScene {
    pub size: RenderSize,
    pub composition_size: RenderSize,
    pub effect_size: RenderSize,
    pub background: [f64; 4],
    pub nodes: Vec<RenderNode>,
    pub roots: Vec<SceneNodeId>,
}

impl RenderScene {
    /// The media capability declares its placement; the source decides how many
    /// of the requested pixels it can produce.
    pub(super) fn media_raster_size_for_input(
        item: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
        input_id: &str,
        target_size: RenderSize,
        composition_size: RenderSize,
    ) -> RenderSize {
        let (capabilities, properties) = match effect_id {
            Some(effect_id) => {
                let Some(effect) = item.effects.iter().find(|effect| effect.id == effect_id) else {
                    return target_size;
                };
                (effect.schema().inputs(), &effect.properties)
            }
            None => {
                let Some(schema) = item.schema() else {
                    return target_size;
                };
                (schema.inputs(), &item.properties)
            }
        };
        let Some(size_property) = capabilities
            .iter()
            .find(|capability| capability.id() == input_id)
            .and_then(|capability| match capability {
                TextureInput::Media {
                    placement: Some(placement),
                    ..
                } => Some(placement.size.as_str()),
                _ => None,
            })
        else {
            return target_size;
        };
        let Some(size_value) = properties.property(size_property) else {
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

    pub(super) fn render_items(&self) -> Vec<&RenderItem> {
        self.nodes
            .iter()
            .filter_map(|node| match &node.content {
                RenderNodeContent::Item(item) => Some(item),
                RenderNodeContent::Scene { .. } => None,
            })
            .collect()
    }

    pub(super) fn render_effects(&self) -> Vec<&RenderEffect> {
        self.nodes
            .iter()
            .flat_map(|node| match &node.content {
                RenderNodeContent::Item(item) => &item.effects,
                RenderNodeContent::Scene { effects, .. } => effects,
            })
            .collect()
    }

    pub(super) fn from_graph(
        size: RenderSize,
        composition_size: RenderSize,
        nodes: Vec<RenderNode>,
        roots: Vec<SceneNodeId>,
    ) -> Result<Self, RenderError> {
        let effect_scale = roots
            .iter()
            .map(|id| nodes[*id].render_scale)
            .max()
            .unwrap_or(1);
        let effect_size = size.checked_scale(effect_scale).ok_or_else(|| {
            RenderError::resource_limit("hierarchical scene effect size overflows")
        })?;
        Ok(Self {
            size,
            composition_size,
            effect_size,
            background: [0.008, 0.006, 0.005, 1.],
            nodes,
            roots,
        })
    }

    fn from_timeline<E: From<RenderError>>(
        timeline: &dyn TimelineView,
        time: TimelineTime,
        size: RenderSize,
        media_frame: impl FnMut(MediaFrameRequest) -> Result<Option<Arc<VisualFrame>>, E>,
        text_frame: impl FnMut(TextFrameRequest<'_>) -> Result<Arc<VisualFrame>, RenderError>,
    ) -> Result<Self, E> {
        SceneGraphBuilder::build(timeline, time, size, media_frame, text_frame)
    }

    pub(crate) fn render_effect(
        effect: &EffectInstance,
        temporal_samples: Vec<Option<Vec<RenderTemporalSample>>>,
    ) -> RenderEffect {
        let schema = effect.schema();
        let properties = schema
            .property_layout()
            .pack("effect", schema.id(), &effect.properties)
            .expect("timeline effect properties come from the validated schema");
        let passes = schema
            .render()
            .passes
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
            output_bounds: BoundsOperation::from_schema(
                &schema.render().bounds,
                &effect.properties,
            ),
            input_space: schema.render().input_space,
        }
    }

    pub(super) fn pack_item_properties(item: &TimelineItem, schema: &ItemSchema) -> Vec<u8> {
        schema
            .property_layout()
            .pack("item", schema.id(), &item.properties)
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

impl From<zerium_shader::ShaderError> for RenderError {
    fn from(error: zerium_shader::ShaderError) -> Self {
        Self::backend(error.to_string())
    }
}
