//! Consumer-local text preparation and timeline-to-scene evaluation.
use super::{RenderError, RenderScene, RenderSize, graph::SceneGraphBuilder, text::TextFrameCache};
use crate::engine::{
    input::{MediaFrameRequest, VisualFrame},
    processing::CalculationPolicy,
};
use std::sync::Arc;
use zerium_core::timeline::{TimelineTime, TimelineView};

pub(crate) struct SceneEvaluator {
    text: TextFrameCache,
    calculation_policy: CalculationPolicy,
}

impl SceneEvaluator {
    pub(crate) fn new(calculation_policy: CalculationPolicy) -> Self {
        Self {
            text: TextFrameCache::new(),
            calculation_policy,
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
            self.calculation_policy,
            media,
            |request| self.text.frame_for(request, composition_size),
        )
    }
}
