//! Consumer-local text preparation and timeline-to-scene evaluation.
use super::{RenderError, RenderScene, RenderSize, graph::SceneGraphBuilder, text::TextFrameCache};
use crate::engine::{frame::VisualFrame, media::MediaFrameRequest};
use std::sync::Arc;
use zerium_core::timeline::{TimelineTime, TimelineView};

pub(crate) struct SceneEvaluator {
    text: TextFrameCache,
    strict_calculations: bool,
}

impl SceneEvaluator {
    pub(crate) fn new() -> Self {
        Self {
            text: TextFrameCache::new(),
            strict_calculations: false,
        }
    }

    pub(crate) fn for_export() -> Self {
        Self {
            strict_calculations: true,
            ..Self::new()
        }
    }

    pub(crate) fn evaluate<E: From<RenderError>>(
        &mut self,
        timeline: &dyn TimelineView,
        time: TimelineTime,
        size: RenderSize,
        media: impl FnMut(MediaFrameRequest) -> Result<Option<Arc<VisualFrame>>, E>,
    ) -> Result<RenderScene, E> {
        let composition_size = RenderSize::from(timeline.resolution());
        SceneGraphBuilder::build(
            timeline,
            time,
            size,
            self.strict_calculations,
            media,
            |request| self.text.frame_for(request, composition_size),
        )
    }
}
