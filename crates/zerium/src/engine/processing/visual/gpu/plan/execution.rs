//! Lower the logical DAG to self-contained image tasks and an ordered execution
//! plan. The encoder needs only this plan, never the logical graph.

use std::{collections::HashMap, sync::Arc};

use super::{
    ImageInput, PassKind, RenderGraph, RenderNodeId, RenderNodeKind,
    surfaces::{RenderRequest, SurfaceKey, SurfaceOutput, SurfaceRaster},
};
use crate::engine::{
    input::VisualFrame,
    processing::visual::{
        gpu::{
            PROPERTY_WORD_SIZE,
            device::interface::{GpuItem, GpuPass, GpuSurface},
            session::resources::surface_size_bytes,
        },
        scene::{RasterGrid, RenderError, RenderScene, RenderSize, RenderView, SurfaceRect},
    },
};
use zerium_core::timeline::BlendMode;
use zerium_shader::{EffectShaderId, ImageBinding, ItemShaderId};

const RETAINED_SURFACE_BUDGET: usize = 128 * 1024 * 1024;

/// Only data needed by physical tasks that survive cache lookup is packed here.
#[derive(Default)]
pub(in crate::engine::processing::visual::gpu) struct RenderData {
    pub background: [f64; 4],
    pub items: Vec<GpuItem>,
    pub properties: Vec<u8>,
    /// Unique by Arc identity; upload preparation relies on graph interning.
    pub frames: Vec<Arc<VisualFrame>>,
}

impl RenderData {
    fn push_properties(&mut self, properties: &[u8]) -> Result<GpuItem, RenderError> {
        let property_offset = u32::try_from(self.properties.len() / PROPERTY_WORD_SIZE)
            .map_err(|_| RenderError::resource_limit("property offset exceeds u32"))?;
        let property_size = u32::try_from(properties.len())
            .map_err(|_| RenderError::resource_limit("property size exceeds u32"))?;
        self.properties.extend_from_slice(properties);
        self.properties.resize(
            self.properties.len().next_multiple_of(PROPERTY_WORD_SIZE),
            0,
        );
        Ok(GpuItem {
            property_offset,
            property_size,
        })
    }
}

pub(in crate::engine::processing::visual::gpu) type SurfaceId = usize;

#[derive(Debug, PartialEq, Eq)]
pub(in crate::engine::processing::visual::gpu) enum ExecutionCommand {
    Render(SurfaceId),
    Composite {
        source: SurfaceId,
        blend_mode: BlendMode,
    },
    /// The last consumer has been encoded; this allocation may now be overwritten.
    Release(SurfaceId),
    /// Move a completed, allocated raster into the next frame's cache.
    Retain {
        surface: SurfaceId,
        key: SurfaceKey,
    },
}

/// Inputs and shader state belong to the operation that consumes them. There is
/// no separate input-kind tag to reconcile with a logical node during encoding.
pub(in crate::engine::processing::visual::gpu) enum RasterOperation {
    Clear,
    Item {
        shader: ItemShaderId,
        instance: u32,
        images: Vec<(ImageBinding, SurfaceId)>,
        uniform: GpuSurface,
    },
    Pass {
        shader: EffectShaderId,
        kind: PassKind,
        images: Vec<(ImageBinding, SurfaceId)>,
        uniform: GpuPass,
    },
    Composite {
        children: Vec<(BlendMode, SurfaceId)>,
    },
    View {
        input: SurfaceId,
        view: RenderView,
    },
}

impl RasterOperation {
    pub(in crate::engine::processing::visual::gpu) fn uniform_bytes(&self) -> &[u8] {
        match self {
            Self::Item { uniform, .. } => bytemuck::bytes_of(uniform),
            Self::Pass { uniform, .. } => bytemuck::bytes_of(uniform),
            _ => &[],
        }
    }

    fn dependencies(&self) -> Vec<SurfaceId> {
        let mut ids = match self {
            Self::Item { images, .. } | Self::Pass { images, .. } => {
                images.iter().map(|(_, id)| *id).collect()
            }
            Self::Composite { children } => children.iter().map(|(_, id)| *id).collect(),
            Self::View { input, .. } => vec![*input],
            Self::Clear => Vec::new(),
        };
        // Binding the same image twice still constitutes one consumer operation.
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}

pub(in crate::engine::processing::visual::gpu) enum SurfaceTask {
    Transparent {
        bounds: SurfaceRect,
    },
    Cached {
        key: SurfaceKey,
        bytes: usize,
    },
    NativeFrame {
        index: usize,
        bounds: SurfaceRect,
    },
    Raster {
        key: SurfaceKey,
        bounds: SurfaceRect,
        raster: SurfaceRaster,
        // Keep cache-hit tasks compact; shader state is needed only for raster work.
        operation: Box<RasterOperation>,
    },
}

pub(in crate::engine::processing::visual::gpu) struct PlannedSurface {
    pub unchanged: bool,
    pub task: SurfaceTask,
}

impl PlannedSurface {
    pub(in crate::engine::processing::visual::gpu) fn cached_key(&self) -> Option<SurfaceKey> {
        match self.task {
            SurfaceTask::Cached { key, .. } => Some(key),
            _ => None,
        }
    }

    fn dependencies(&self) -> Vec<SurfaceId> {
        match &self.task {
            SurfaceTask::Raster { operation, .. } => operation.dependencies(),
            _ => Vec::new(),
        }
    }

    fn retention(&self) -> Option<(SurfaceKey, usize)> {
        match &self.task {
            SurfaceTask::Cached { key, bytes } => Some((*key, *bytes)),
            SurfaceTask::Raster {
                key,
                raster,
                operation,
                ..
            } => Some((
                *key,
                surface_size_bytes(raster.size()).saturating_add(operation.uniform_bytes().len()),
            )),
            _ => None,
        }
    }
}

#[derive(Default)]
pub(in crate::engine::processing::visual::gpu) struct ExecutionPlan {
    pub data: RenderData,
    pub commands: Vec<ExecutionCommand>,
    pub surfaces: Vec<PlannedSurface>,
}

struct ExecutionPlanner<'a> {
    graph: &'a RenderGraph,
    composition_size: RenderSize,
    limit: u32,
    cached_size: &'a dyn Fn(SurfaceKey) -> Option<usize>,
    by_key: HashMap<SurfaceKey, SurfaceId>,
    shader_data: Vec<Option<PreparedShader>>,
    output: ExecutionPlan,
}

/// Different rasters of one logical shader share their constant GPU data.
#[derive(Clone, Copy)]
struct PreparedShader {
    properties: GpuItem,
    item_instance: Option<u32>,
}

#[derive(Clone, Default)]
struct SurfaceUsage {
    count: usize,
    root: bool,
    stable_boundary: bool,
    release_command: usize,
}

impl ExecutionPlanner<'_> {
    fn properties(&mut self, node: RenderNodeId, bytes: &[u8]) -> Result<GpuItem, RenderError> {
        if let Some(prepared) = self.shader_data[node] {
            return Ok(prepared.properties);
        }
        let properties = self.output.data.push_properties(bytes)?;
        self.shader_data[node] = Some(PreparedShader {
            properties,
            item_instance: None,
        });
        Ok(properties)
    }

    fn item_instance(&mut self, node: RenderNodeId, bytes: &[u8]) -> Result<u32, RenderError> {
        let properties = self.properties(node, bytes)?;
        let prepared = self.shader_data[node]
            .as_mut()
            .expect("shader properties were prepared");
        if let Some(instance) = prepared.item_instance {
            return Ok(instance);
        }
        let instance = u32::try_from(self.output.data.items.len())
            .map_err(|_| RenderError::resource_limit("too many visible items in one frame"))?;
        self.output.data.items.push(properties);
        prepared.item_instance = Some(instance);
        Ok(instance)
    }

    fn images(
        &mut self,
        images: &[ImageInput],
        grid: RasterGrid,
    ) -> Result<Vec<(ImageBinding, SurfaceId)>, RenderError> {
        images
            .iter()
            .map(|image| {
                let input = self.request(
                    image.node,
                    RenderRequest {
                        bounds: self.graph.nodes[image.node].bounds,
                        grid,
                    },
                )?;
                Ok((image.binding, input))
            })
            .collect()
    }

    fn operation(
        &mut self,
        node: RenderNodeId,
        raster: SurfaceRaster,
    ) -> Result<RasterOperation, RenderError> {
        let description = &self.graph.nodes[node];
        let (rect, size, grid) = (raster.rect(), raster.size(), raster.grid());
        Ok(match &description.kind {
            RenderNodeKind::Transparent => RasterOperation::Clear,
            RenderNodeKind::Item {
                shader,
                properties,
                images,
            } => {
                let images = self.images(images, grid)?;
                let instance = self.item_instance(node, properties)?;
                RasterOperation::Item {
                    shader: shader.clone(),
                    instance,
                    images,
                    uniform: GpuSurface {
                        output_size: [size.width as f32, size.height as f32],
                        composition_size: [
                            self.composition_size.width as f32,
                            self.composition_size.height as f32,
                        ],
                        surface_min: rect.min.map(|value| value as f32),
                        surface_size: [
                            (rect.max[0] - rect.min[0]) as f32,
                            (rect.max[1] - rect.min[1]) as f32,
                        ],
                    },
                }
            }
            RenderNodeKind::Pass { images, pass } => {
                let images = self.images(images, grid)?;
                let properties = self.properties(node, &pass.properties)?;
                let uniform = GpuPass {
                    property_offset: properties.property_offset,
                    property_size: properties.property_size,
                    sample_index: pass.temporal[0],
                    sample_count: pass.temporal[1],
                    frame_offset: f32::from_bits(pass.temporal[2]),
                    sample_progress: f32::from_bits(pass.temporal[3]),
                    composition_size: [
                        self.composition_size.width as f32,
                        self.composition_size.height as f32,
                    ],
                    surface_min: rect.min.map(|value| value as f32),
                    surface_size: [
                        (rect.max[0] - rect.min[0]) as f32,
                        (rect.max[1] - rect.min[1]) as f32,
                    ],
                    output_size: [size.width, size.height],
                    bounds_min: description.bounds.min.map(|value| value as f32),
                    bounds_size: [
                        (description.bounds.max[0] - description.bounds.min[0]) as f32,
                        (description.bounds.max[1] - description.bounds.min[1]) as f32,
                    ],
                };
                RasterOperation::Pass {
                    shader: pass.shader.clone(),
                    kind: pass.kind,
                    images,
                    uniform,
                }
            }
            RenderNodeKind::Composite { children } => {
                let request = RenderRequest {
                    bounds: grid.sampling_bounds(rect),
                    grid,
                };
                let mut inputs = Vec::new();
                for child in children {
                    if self.graph.nodes[child.node]
                        .bounds
                        .intersection(request.bounds)
                        .is_some()
                    {
                        inputs.push((child.blend_mode, self.request(child.node, request)?));
                    }
                }
                RasterOperation::Composite { children: inputs }
            }
            RenderNodeKind::View { input, view } => {
                let grid = grid.through_view(*view);
                RasterOperation::View {
                    input: self.request(
                        *input,
                        RenderRequest {
                            bounds: grid.sampling_bounds(view.source_bounds(rect)),
                            grid,
                        },
                    )?,
                    view: *view,
                }
            }
            RenderNodeKind::Frame { .. } => {
                unreachable!("native frames are resolved before raster lowering")
            }
        })
    }

    fn request(
        &mut self,
        node: RenderNodeId,
        request: RenderRequest,
    ) -> Result<SurfaceId, RenderError> {
        let description = &self.graph.nodes[node];
        let output = SurfaceOutput::resolve(description, request, self.limit)?;
        let key = output.map(|output| output.key(description.identity));
        if let Some(id) = key.and_then(|key| self.by_key.get(&key)) {
            return Ok(*id);
        }
        let task = match output {
            None => SurfaceTask::Transparent {
                bounds: description.bounds,
            },
            Some(SurfaceOutput::NativeFrame { frame }) => {
                let index = self.output.data.frames.len();
                self.output.data.frames.push(frame.clone());
                SurfaceTask::NativeFrame {
                    index,
                    bounds: description.bounds,
                }
            }
            Some(SurfaceOutput::Raster(raster)) => {
                let key = SurfaceOutput::Raster(raster).key(description.identity);
                if let Some(bytes) = (self.cached_size)(key) {
                    // Retained results cut off the dependency subtree entirely.
                    SurfaceTask::Cached { key, bytes }
                } else {
                    SurfaceTask::Raster {
                        key,
                        bounds: description.bounds,
                        raster,
                        operation: Box::new(self.operation(node, raster)?),
                    }
                }
            }
        };
        let id = self.output.surfaces.len();
        self.output.surfaces.push(PlannedSurface {
            unchanged: description.unchanged,
            task,
        });
        if let Some(key) = key {
            self.by_key.insert(key, id);
        }
        self.output.commands.push(ExecutionCommand::Render(id));
        Ok(id)
    }
}

impl ExecutionPlan {
    pub(in crate::engine::processing::visual::gpu) fn build(
        graph: RenderGraph,
        scene: &RenderScene,
        limit: u32,
        cached_size: impl Fn(SurfaceKey) -> Option<usize>,
    ) -> Result<Self, RenderError> {
        let mut output = {
            let mut planner = ExecutionPlanner {
                graph: &graph,
                composition_size: scene.composition_size,
                limit,
                cached_size: &cached_size,
                by_key: HashMap::new(),
                shader_data: vec![None; graph.nodes.len()],
                output: Self {
                    data: RenderData {
                        background: graph.background,
                        ..RenderData::default()
                    },
                    ..Self::default()
                },
            };
            let viewport = SurfaceRect::viewport(scene.composition_size);
            for root in &graph.roots {
                let source = planner.request(
                    root.layer.node,
                    RenderRequest {
                        bounds: viewport,
                        grid: RasterGrid::new(
                            scene.size,
                            scene.composition_size,
                            root.render_scale,
                        ),
                    },
                )?;
                planner.output.commands.push(ExecutionCommand::Composite {
                    source,
                    blend_mode: root.layer.blend_mode,
                });
            }
            planner.output
        };
        output.schedule_releases();
        Ok(output)
    }

    fn reads(&self, command: &ExecutionCommand) -> Vec<SurfaceId> {
        match command {
            ExecutionCommand::Render(id) => self.surfaces[*id].dependencies(),
            ExecutionCommand::Composite { source, .. } => vec![*source],
            ExecutionCommand::Release(_) | ExecutionCommand::Retain { .. } => Vec::new(),
        }
    }

    fn schedule_releases(&mut self) {
        let mut usage = vec![SurfaceUsage::default(); self.surfaces.len()];
        let commands = std::mem::take(&mut self.commands);
        // The first consumer seen backwards is the final use. Record its release
        // now; the retention decision below can replace it without rereading inputs.
        for command in commands.into_iter().rev() {
            for id in self.reads(&command).into_iter().rev() {
                let entry = &mut usage[id];
                if entry.count == 0 {
                    entry.release_command = self.commands.len();
                    self.commands.push(ExecutionCommand::Release(id));
                }
                entry.count += 1;
                if let ExecutionCommand::Render(consumer) = &command
                    && !self.surfaces[*consumer].unchanged
                    && self.surfaces[id].unchanged
                {
                    entry.stable_boundary = true;
                }
            }
            if let ExecutionCommand::Composite { source, .. } = &command {
                usage[*source].root = true;
            }
            self.commands.push(command);
        }
        let mut candidates = (0..self.surfaces.len())
            .filter(|id| {
                usage[*id].root
                    || usage[*id].count > 1
                    || usage[*id].stable_boundary
                    || self.surfaces[*id].cached_key().is_some()
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|id| {
            (
                std::cmp::Reverse(usage[*id].root),
                std::cmp::Reverse(usage[*id].stable_boundary),
                std::cmp::Reverse(usage[*id].count),
                *id,
            )
        });
        let mut budget = RETAINED_SURFACE_BUDGET;
        for id in candidates {
            if let Some((key, bytes)) = self.surfaces[id].retention()
                && bytes <= budget
            {
                budget -= bytes;
                self.commands[usage[id].release_command] =
                    ExecutionCommand::Retain { surface: id, key };
            }
        }
        self.commands.reverse();
    }
}
