//! Completed results and idle allocations share one retention budget per session.

use super::{FrameResources, NodeResources};
use crate::engine::processing::visual::{
    gpu::plan::{
        execution::{ExecutionPlan, PlannedSurface},
        surfaces::{SurfaceCache, SurfaceKey},
    },
    scene::{RenderScene, RenderSize},
};

pub(in crate::engine::processing::visual::gpu) const RESOURCE_CACHE_BYTES: usize =
    256 * 1024 * 1024;

#[derive(Default)]
pub(in crate::engine::processing::visual::gpu) struct ResourceCache {
    surfaces: SurfaceCache,
    available: Vec<NodeResources>,
}

impl ResourceCache {
    pub(in crate::engine::processing::visual::gpu) fn prepare_scene(
        &mut self,
        previous_frame: Option<&FrameResources>,
        scene: &RenderScene,
    ) {
        if previous_frame.is_none_or(|frame| !frame.matches_scene(scene)) {
            self.available
                .extend(self.surfaces.recycle_unrequested(std::iter::empty()));
        }
    }

    pub(in crate::engine::processing::visual::gpu) fn cached_size(
        &self,
        key: SurfaceKey,
    ) -> Option<usize> {
        self.surfaces.size_bytes(key)
    }

    pub(in crate::engine::processing::visual::gpu) fn begin_frame(
        &mut self,
        execution: &ExecutionPlan,
    ) -> SurfaceCache {
        let mut previous = std::mem::take(&mut self.surfaces);
        self.available.extend(
            previous.recycle_unrequested(
                execution
                    .surfaces
                    .iter()
                    .filter_map(PlannedSurface::cached_key),
            ),
        );
        previous
    }

    // Publish completed results only after their commands have been submitted.
    pub(in crate::engine::processing::visual::gpu) fn finish_frame(
        &mut self,
        surfaces: SurfaceCache,
    ) {
        self.surfaces = surfaces;
        self.trim();
    }

    pub(in crate::engine::processing::visual::gpu) fn take(
        &mut self,
        size: RenderSize,
        uniform_size: u64,
    ) -> Option<NodeResources> {
        let index = self
            .available
            .iter()
            .rposition(|resource| resource.satisfies(size, uniform_size))?;
        Some(self.available.remove(index))
    }

    pub(in crate::engine::processing::visual::gpu) fn recycle(&mut self, resource: NodeResources) {
        self.available.push(resource);
    }

    fn trim(&mut self) {
        // The planner reserves results first. Only the remaining budget can
        // retain scratch allocations, including frame conversion and blending.
        // Live inputs and targets required to render a frame are not cache space.
        let mut remaining = RESOURCE_CACHE_BYTES.saturating_sub(self.surfaces.retained_bytes());
        // Prefer allocations used most recently, preserving that order next frame.
        self.available.reverse();
        self.available.retain(|resource| {
            let bytes = resource.size_bytes();
            if bytes > remaining {
                return false;
            }
            remaining -= bytes;
            true
        });
        self.available.reverse();
    }
}
