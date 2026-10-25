mod composition;
mod demand;
mod evaluation;
mod geometry;
mod graph;
mod text;

pub(crate) use self::evaluation::SceneEvaluator;
pub(super) use self::geometry::{BoundsContext, BoundsOperation, RasterGrid, SurfaceRect};
use crate::engine::input::VisualFrame;
use std::sync::Arc;
use thiserror::Error;
use zerium_core::{
    plugin::{ComputeDispatchDimension, RenderResultView},
    property::PropertyValues,
    timeline::{BlendMode, ProjectResolution},
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
pub(crate) struct RenderItem {
    pub shader: ItemShaderId,
    pub inputs: Vec<RenderInput>,
    pub properties: Vec<u8>,
    pub output_bounds: Box<BoundsOperation>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderEffect {
    pub passes: Vec<RenderEffectPass>,
    pub inputs: Vec<RenderInput>,
}

/// A resolved texture input keeps its shader name and scene node together.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderInput {
    pub id: String,
    pub node: SceneNodeId,
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
    pub output_bounds: Box<BoundsOperation>,
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

impl RenderView {
    fn from_properties(view: &RenderResultView, properties: &PropertyValues) -> Self {
        let value = |id: &str, index| {
            properties
                .property(id)
                .and_then(|value| value.scalar_at(index))
                .and_then(|value| value.as_f32())
                .expect("validated capture view property")
        };
        Self {
            position: [
                value(&view.position, Some(0)),
                value(&view.position, Some(1)),
            ],
            zoom: value(&view.zoom, None) / 100.,
            angle: value(&view.angle, None),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RenderNodeContent<F = Arc<VisualFrame>> {
    Item(RenderItem),
    Frame(F),
    /// A shader stage depends on its requested upstream image. Temporal
    /// reductions have sampled inputs instead; pure samples alias an existing node.
    Effect {
        input: Option<SceneNodeId>,
        effect: RenderEffect,
    },
    Scene {
        children: Vec<SceneNodeId>,
    },
    /// A finite image of a composition, sampled through a view.
    View {
        input: SceneNodeId,
        view: RenderView,
    },
}

/// A scene-linear compositing node. Scene-instance effects depend on the
/// completed `Scene` image and run once after all children are blended.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderNode<F = Arc<VisualFrame>> {
    pub content: RenderNodeContent<F>,
    /// Resolved once from this node and all of its input requirements.
    pub render_scale: u32,
    /// Used only when this completed node is composited into its parent scene.
    pub blend_mode: BlendMode,
}

impl<F> RenderNode<F> {
    fn map_frame<G>(self, map: impl FnOnce(F) -> G) -> RenderNode<G> {
        let content = match self.content {
            RenderNodeContent::Frame(frame) => RenderNodeContent::Frame(map(frame)),
            RenderNodeContent::Item(item) => RenderNodeContent::Item(item),
            RenderNodeContent::Effect { input, effect } => {
                RenderNodeContent::Effect { input, effect }
            }
            RenderNodeContent::Scene { children } => RenderNodeContent::Scene { children },
            RenderNodeContent::View { input, view } => RenderNodeContent::View { input, view },
        };
        RenderNode {
            content,
            render_scale: self.render_scale,
            blend_mode: self.blend_mode,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenderScene {
    pub size: RenderSize,
    pub composition_size: RenderSize,
    pub background: [f64; 4],
    pub nodes: Vec<RenderNode>,
    pub roots: Vec<SceneNodeId>,
}

impl RenderScene {
    pub(super) fn from_graph(
        size: RenderSize,
        composition_size: RenderSize,
        nodes: Vec<RenderNode>,
        roots: Vec<SceneNodeId>,
    ) -> Self {
        Self {
            size,
            composition_size,
            background: [0.008, 0.006, 0.005, 1.],
            nodes,
            roots,
        }
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub(crate) enum RenderError {
    #[error("{0}")]
    ResourceLimit(String),
    #[error("{0}")]
    Backend(String),
    #[error("At frame {frame} (path {path:?}): {diagnostic}", frame = time.frames())]
    Calculation {
        time: zerium_core::timeline::TimelineTime,
        path: Vec<zerium_core::timeline::ItemId>,
        #[source]
        diagnostic: Box<zerium_core::timeline::CalculationDiagnostic>,
    },
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
