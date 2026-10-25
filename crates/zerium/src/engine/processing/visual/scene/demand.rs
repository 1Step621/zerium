//! Decoder affinity follows consumer edges, independently of shared image nodes.
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock},
};

use super::{RenderError, RenderNode, SceneNodeId};
use crate::engine::input::{
    DecodeStreamKey, MediaFrameRequest, MediaInputId, SamplingBranch, VisualFrame,
};

pub(super) enum SourceFrame {
    Ready(Arc<VisualFrame>),
    Media(MediaFrameId),
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct DemandId(usize);

#[derive(Clone, Copy)]
pub(super) struct MediaFrameId(usize);

/// Reusing an image always reuses its consumer dependencies, including aliases
/// that have no render node of their own.
#[derive(Clone, Copy)]
pub(super) struct PlannedImage {
    node: Option<SceneNodeId>,
    demand: DemandId,
}

struct DemandEdge {
    image: DemandId,
    sampling: Option<SamplingBranch>,
}

#[derive(Default)]
pub(super) struct ImageDemand {
    inputs: Vec<DemandEdge>,
    media: Vec<MediaFrameId>,
}

impl ImageDemand {
    pub(super) fn input(&mut self, image: PlannedImage) -> Option<SceneNodeId> {
        self.dependency(image, None)
    }

    pub(super) fn sample(
        &mut self,
        image: PlannedImage,
        sampling: SamplingBranch,
    ) -> Option<SceneNodeId> {
        self.dependency(image, Some(sampling))
    }

    fn dependency(
        &mut self,
        image: PlannedImage,
        sampling: Option<SamplingBranch>,
    ) -> Option<SceneNodeId> {
        self.inputs.push(DemandEdge {
            image: image.demand,
            sampling,
        });
        image.node
    }
}

#[derive(Default)]
pub(super) struct MediaDemandGraph {
    images: Vec<ImageDemand>,
    sources: Vec<MediaFrameRequest<MediaInputId>>,
}

struct FrameBinding {
    request: MediaFrameRequest,
    targets: Vec<MediaFrameId>,
}

impl MediaDemandGraph {
    pub(super) fn image(&mut self, node: Option<SceneNodeId>, inputs: ImageDemand) -> PlannedImage {
        PlannedImage {
            node,
            demand: self.push_demand(inputs),
        }
    }

    fn push_demand(&mut self, inputs: ImageDemand) -> DemandId {
        let demand = DemandId(self.images.len());
        self.images.push(inputs);
        demand
    }

    pub(super) fn media(
        &mut self,
        owner: &mut ImageDemand,
        request: MediaFrameRequest<MediaInputId>,
    ) -> SourceFrame {
        let id = MediaFrameId(self.sources.len());
        self.sources.push(request);
        owner.media.push(id);
        SourceFrame::Media(id)
    }

    fn bindings(
        &self,
        root: DemandId,
        max_depth: usize,
        max_samples: usize,
    ) -> Result<Vec<FrameBinding>, RenderError> {
        let mut pending = vec![(root, Vec::new())];
        let mut visited = HashSet::new();
        let mut requests = Vec::<FrameBinding>::new();
        let mut request_ids = HashMap::new();
        let mut samples = 0;
        while let Some((id, branch)) = pending.pop() {
            if !visited.insert((id, branch.clone())) {
                continue;
            }
            let image = &self.images[id.0];
            for &frame in &image.media {
                let source = &self.sources[frame.0];
                let request = MediaFrameRequest {
                    stream: DecodeStreamKey {
                        input: source.stream.clone(),
                        branch: branch.clone(),
                    },
                    asset: source.asset.clone(),
                    presentation_time: source.presentation_time,
                    size: source.size,
                };
                let index = *request_ids.entry(request.clone()).or_insert_with(|| {
                    let index = requests.len();
                    requests.push(FrameBinding {
                        request,
                        targets: Vec::new(),
                    });
                    index
                });
                requests[index].targets.push(frame);
            }
            for edge in image.inputs.iter().rev() {
                let mut branch = branch.clone();
                if let Some(sampling) = &edge.sampling {
                    branch.push(sampling.clone());
                    samples += 1;
                    if branch.len() > max_depth || samples > max_samples {
                        return Err(RenderError::resource_limit(
                            "media demand exceeds temporal limits",
                        ));
                    }
                }
                pending.push((edge.image, branch));
            }
        }
        // Decoder state follows source time, not graph traversal order. Stable
        // sorting preserves owner priority when shared requests coincide.
        requests.sort_by_key(|binding| binding.request.presentation_time);
        Ok(requests)
    }

    /// Plan and validate every consumer path before invoking any decoder.
    /// Media slots have their own identity; they do not index render nodes.
    pub(super) fn bind<E: From<RenderError>>(
        mut self,
        root: ImageDemand,
        nodes: Vec<RenderNode<SourceFrame>>,
        max_depth: usize,
        max_samples: usize,
        mut frame_for: impl FnMut(MediaFrameRequest) -> Result<Option<Arc<VisualFrame>>, E>,
    ) -> Result<Vec<RenderNode>, E> {
        let root = self.push_demand(root);
        let bindings = self.bindings(root, max_depth, max_samples)?;
        let mut frames = vec![None; self.sources.len()];
        for FrameBinding { request, targets } in bindings {
            if let Some(frame) = frame_for(request)? {
                for target in targets {
                    // All owners request the same source pixels. A ready owner
                    // can fill the shared node while the others retain demand.
                    frames[target.0].get_or_insert_with(|| frame.clone());
                }
            }
        }
        // Missing sources have stable pixel identity across evaluations too.
        static TRANSPARENT: OnceLock<Arc<VisualFrame>> = OnceLock::new();
        let transparent = TRANSPARENT.get_or_init(|| Arc::new(VisualFrame::rgba(1, 1, [0u8; 4])));
        Ok(nodes
            .into_iter()
            .map(|node| {
                node.map_frame(|source| match source {
                    SourceFrame::Ready(frame) => frame,
                    SourceFrame::Media(id) => {
                        frames[id.0].clone().unwrap_or_else(|| transparent.clone())
                    }
                    SourceFrame::Empty => transparent.clone(),
                })
            })
            .collect())
    }
}
