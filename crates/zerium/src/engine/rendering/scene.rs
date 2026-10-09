use super::scene_builder::SceneBuilder;
use super::surface::{BoundsOperation, SurfaceRect};
use super::text::TextFrameRequest;
use crate::engine::frame::RgbaFrame;
use std::sync::Arc;
use thiserror::Error;
use zerium_core::plugin::{
    ComputeDispatchDimension, EffectInputSpace, EffectPassSchema, ItemSchema, TextureInput,
};
use zerium_core::timeline::{
    EffectInstance, EffectInstanceId, ItemId, LayerId, ProjectResolution, TimelineItem,
    TimelineTime, TimelineView,
};
use zerium_shader::{EffectShaderId, ItemShaderId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RenderQuality {
    #[default]
    Full,
    Realtime {
        max_temporal_samples: usize,
    },
}

impl RenderQuality {
    pub(super) fn temporal_offsets(self, offsets: Vec<f64>) -> Vec<f64> {
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
    Texture(Vec<Arc<RgbaFrame>>),
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
    pub time: TimelineTime,
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
        children: Vec<SceneNodeId>,
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
    pub render_scale: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MediaFrameRequest<'a> {
    pub item_id: ItemId,
    pub effect_id: Option<EffectInstanceId>,
    pub input_id: &'a str,
    pub time: TimelineTime,
    pub target_size: RenderSize,
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
    pub(crate) fn media_raster_size_for_input(
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

    pub(crate) fn render_size_for_item(
        item: &TimelineItem,
        size: RenderSize,
    ) -> Result<RenderSize, RenderError> {
        let render_scale = item
            .effects
            .iter()
            .map(|effect| effect.schema().render().scale)
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

    pub(crate) fn from_timeline<E: From<RenderError>>(
        timeline: &dyn TimelineView,
        time: TimelineTime,
        size: RenderSize,
        quality: RenderQuality,
        media_frame: impl FnMut(MediaFrameRequest<'_>) -> Result<Option<Arc<RgbaFrame>>, E>,
        text_frame: impl FnMut(TextFrameRequest<'_>) -> Result<Arc<RgbaFrame>, RenderError>,
    ) -> Result<Self, E> {
        SceneBuilder::build(timeline, time, size, quality, media_frame, text_frame)
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
