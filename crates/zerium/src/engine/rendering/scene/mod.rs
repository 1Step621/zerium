mod composition;
mod evaluation;
mod geometry;
mod graph;
mod text;

use crate::engine::frame::VisualFrame;
pub(crate) use evaluation::SceneEvaluator;
pub(super) use geometry::{BoundsContext, BoundsOperation, RasterGrid, SurfaceRect};
use std::sync::Arc;
use thiserror::Error;
use zerium_core::plugin::ComputeDispatchDimension;
use zerium_core::timeline::{BlendMode, ProjectResolution};
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
    pub effects: Vec<RenderEffect>,
    pub render_scale: u32,
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

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RenderNodeContent {
    Item(RenderItem),
    Frame(Arc<VisualFrame>),
    Scene {
        children: Vec<SceneNodeId>,
        effects: Vec<RenderEffect>,
        render_scale: u32,
    },
    /// A finite image of a composition, sampled through a view.
    View {
        input: SceneNodeId,
        view: RenderView,
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
