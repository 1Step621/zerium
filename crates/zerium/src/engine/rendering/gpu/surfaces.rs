//! Image identity, resolved raster extents, and result reuse within and between frames.

use std::collections::HashMap;

use super::plan::RenderNodeIdentity;
use super::resources::NodeResources;
use super::scene::{RasterGrid, RenderError, RenderSize, SurfaceRect};

const RETAINED_SURFACE_BUDGET: usize = 128 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct RenderedSurface {
    pub bounds: SurfaceRect,
    pub view: wgpu::TextureView,
    pub rect: SurfaceRect,
    pub size: RenderSize,
}

/// The raster is resolved once, before lookup, allocation, or uniform creation.
#[derive(Clone, Copy)]
pub(super) struct SurfaceRaster {
    pub rect: SurfaceRect,
    pub size: RenderSize,
    pub grid: RasterGrid,
}

impl SurfaceRaster {
    pub(super) fn new(
        bounds: SurfaceRect,
        grid: RasterGrid,
        limit: u32,
    ) -> Result<Self, RenderError> {
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct RasterKey {
    rect: [u64; 4],
    size: [u32; 2],
    grid: [u64; 4],
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct SurfaceKey {
    node: RenderNodeIdentity,
    raster: Option<RasterKey>,
}

impl SurfaceKey {
    pub(super) fn new(node: RenderNodeIdentity, raster: Option<SurfaceRaster>) -> Self {
        Self {
            node,
            // Native conversion does not depend on the consumer's raster grid.
            raster: raster.map(|raster| RasterKey {
                rect: [
                    raster.rect.min[0].to_bits(),
                    raster.rect.min[1].to_bits(),
                    raster.rect.max[0].to_bits(),
                    raster.rect.max[1].to_bits(),
                ],
                size: [raster.size.width, raster.size.height],
                grid: raster.grid.key(),
            }),
        }
    }
}

struct CachedSurface {
    surface: RenderedSurface,
    // Native conversion targets belong to FrameImage and are shared for this frame only.
    resources: Option<NodeResources>,
    uses: usize,
}

#[derive(Default)]
pub(super) struct SurfaceCache {
    retained: HashMap<SurfaceKey, CachedSurface>,
}

pub(super) struct FrameSurfaces {
    previous: HashMap<SurfaceKey, CachedSurface>,
    current: HashMap<SurfaceKey, CachedSurface>,
}

impl SurfaceCache {
    pub(super) fn begin_frame(&mut self) -> FrameSurfaces {
        let mut previous = std::mem::take(&mut self.retained);
        for entry in previous.values_mut() {
            entry.uses = 0;
        }
        FrameSurfaces {
            previous,
            current: HashMap::new(),
        }
    }
}

impl FrameSurfaces {
    pub(super) fn get(&mut self, key: SurfaceKey) -> Option<RenderedSurface> {
        if let Some(entry) = self.previous.remove(&key) {
            self.current.insert(key, entry);
        }
        let entry = self.current.get_mut(&key)?;
        entry.uses += 1;
        Some(entry.surface.clone())
    }

    pub(super) fn insert(
        &mut self,
        key: SurfaceKey,
        surface: RenderedSurface,
        resources: Option<NodeResources>,
    ) {
        self.current.insert(
            key,
            CachedSurface {
                surface,
                resources,
                uses: 1,
            },
        );
    }

    /// Publish retention only after submission. Unused and over-budget targets return to the pool.
    pub(super) fn finish(self, cache: &mut SurfaceCache) -> Vec<NodeResources> {
        let mut reusable = self
            .previous
            .into_values()
            .filter_map(|entry| entry.resources)
            .collect::<Vec<_>>();
        let mut candidates = self.current.into_iter().collect::<Vec<_>>();
        candidates.sort_by_key(|(key, entry)| (std::cmp::Reverse(entry.uses), *key));
        let mut remaining = RETAINED_SURFACE_BUDGET;
        for (key, mut entry) in candidates {
            let Some(resources) = &entry.resources else {
                continue;
            };
            let bytes = resources.size_bytes();
            if bytes <= remaining {
                remaining -= bytes;
                cache.retained.insert(key, entry);
            } else if let Some(resources) = entry.resources.take() {
                reusable.push(resources);
            }
        }
        reusable
    }
}
