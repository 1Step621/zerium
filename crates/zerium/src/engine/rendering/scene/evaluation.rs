//! Consumer-local text preparation and timeline-to-scene evaluation.
use super::{RenderError, RenderScene, RenderSize, graph::SceneGraphBuilder, text::TextFrameCache};
use crate::engine::{frame::VisualFrame, media::MediaFrameRequest};
use std::sync::Arc;
use zerium_core::timeline::{TimelineTime, TimelineView};

pub(crate) struct SceneEvaluator {
    text: TextFrameCache,
}

impl SceneEvaluator {
    pub(crate) fn new() -> Self {
        Self {
            text: TextFrameCache::new(),
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
        let active_items = timeline.active_items_at_time(time);
        self.text
            .retain_active(active_items.iter().map(|(_, item)| item));
        SceneGraphBuilder::build(timeline, time, size, media, |request| {
            self.text.frame_for(request, composition_size)
        })
    }
}
