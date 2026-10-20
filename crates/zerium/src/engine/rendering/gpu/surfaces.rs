//! Image identity, resolved raster extents, and result reuse within and between frames.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::plan::{RenderNode, RenderNodeIdentity, RenderNodeKind};
use super::resources::NodeResources;
use super::scene::{RasterGrid, RenderError, RenderSize, SurfaceRect};
use crate::engine::frame::VisualFrame;

pub(super) struct RenderedSurface {
    pub bounds: SurfaceRect,
    pub view: wgpu::TextureView,
    pub rect: SurfaceRect,
}

impl RenderedSurface {
    pub(super) fn size(&self) -> RenderSize {
        let texture = self.view.texture();
        RenderSize {
            width: texture.width(),
            height: texture.height(),
        }
    }
}

/// The raster is resolved once, before lookup, allocation, or uniform creation.
#[derive(Clone, Copy)]
pub(super) struct SurfaceRaster {
    rect: SurfaceRect,
    size: RenderSize,
    grid: RasterGrid,
}

impl SurfaceRaster {
    pub(super) fn rect(self) -> SurfaceRect {
        self.rect
    }

    pub(super) fn size(self) -> RenderSize {
        self.size
    }

    pub(super) fn grid(self) -> RasterGrid {
        self.grid
    }

    fn new(bounds: SurfaceRect, grid: RasterGrid, limit: u32) -> Result<Self, RenderError> {
        let (rect, size) = bounds.pixel_aligned(grid).ok_or_else(|| {
            RenderError::resource_limit("render surface bounds cannot be rasterized")
        })?;
        if size.width > limit || size.height > limit {
            return Err(RenderError::resource_limit(format!(
                "render surface {}x{} exceeds GPU limit {limit}",
                size.width, size.height
            )));
        }
        Ok(Self { rect, size, grid })
    }
}

/// What a consumer needs, distinct from a node's complete logical image.
#[derive(Clone, Copy)]
pub(super) struct RenderRequest {
    pub bounds: SurfaceRect,
    pub grid: RasterGrid,
}

/// The only allocation decision accepted by surface encoding. Native frames
/// retain their source raster; all other nodes carry a validated raster extent.
#[derive(Clone, Copy)]
pub(super) enum SurfaceOutput<'a> {
    NativeFrame { frame: &'a Arc<VisualFrame> },
    Raster(SurfaceRaster),
}

impl<'a> SurfaceOutput<'a> {
    pub(super) fn resolve(
        node: &'a RenderNode,
        request: RenderRequest,
        limit: u32,
    ) -> Result<Option<Self>, RenderError> {
        if !request.bounds.is_valid() || !request.grid.is_valid() {
            return Err(RenderError::resource_limit("invalid image request"));
        }
        let Some(visible_bounds) = node.bounds.intersection(request.bounds) else {
            return Ok(None);
        };
        let bounds = match &node.kind {
            RenderNodeKind::Frame { frame } => {
                return Ok(Some(Self::NativeFrame { frame }));
            }
            // Shader nodes finish their complete logical image. Composition
            // and view nodes select coverage for this consumer.
            RenderNodeKind::Item { .. } | RenderNodeKind::Pass { .. } => node.bounds,
            _ => visible_bounds,
        };
        Ok(Some(Self::Raster(SurfaceRaster::new(
            bounds,
            request.grid,
            limit,
        )?)))
    }

    pub(super) fn key(self, node: RenderNodeIdentity) -> SurfaceKey {
        let raster = match self {
            Self::NativeFrame { .. } => None,
            Self::Raster(raster) => Some(RasterKey {
                rect: [
                    raster.rect.min[0].to_bits(),
                    raster.rect.min[1].to_bits(),
                    raster.rect.max[0].to_bits(),
                    raster.rect.max[1].to_bits(),
                ],
                size: [raster.size.width, raster.size.height],
                grid: raster.grid.key(),
            }),
        };
        SurfaceKey { node, raster }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct RasterKey {
    rect: [u64; 4],
    size: [u32; 2],
    grid: [u64; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct SurfaceKey {
    node: RenderNodeIdentity,
    raster: Option<RasterKey>,
}

/// Only results explicitly retained by the execution plan cross frame boundaries.
struct CachedSurface {
    surface: RenderedSurface,
    resources: NodeResources,
}

#[derive(Default)]
pub(super) struct SurfaceCache {
    retained: HashMap<SurfaceKey, CachedSurface>,
}

impl SurfaceCache {
    pub(super) fn size_bytes(&self, key: SurfaceKey) -> Option<usize> {
        self.retained
            .get(&key)
            .map(|entry| entry.resources.size_bytes())
    }

    /// Consume a previous-frame result once; all of this frame's consumers share
    /// the resulting SurfaceId rather than looking it up in another cache.
    pub(super) fn take(&mut self, key: SurfaceKey) -> Option<(RenderedSurface, NodeResources)> {
        self.retained
            .remove(&key)
            .map(|entry| (entry.surface, entry.resources))
    }

    pub(super) fn insert(
        &mut self,
        key: SurfaceKey,
        surface: RenderedSurface,
        resources: NodeResources,
    ) {
        self.retained
            .insert(key, CachedSurface { surface, resources });
    }

    pub(super) fn recycle_unrequested(
        &mut self,
        requested: impl Iterator<Item = SurfaceKey>,
    ) -> Vec<NodeResources> {
        let requested = requested.collect::<HashSet<_>>();
        self.retained
            .extract_if(|key, _| !requested.contains(key))
            .map(|(_, entry)| entry.resources)
            .collect()
    }
}
