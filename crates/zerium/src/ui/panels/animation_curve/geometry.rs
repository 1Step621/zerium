//! One plot rectangle for painting, hit testing, and pointer conversion.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct GraphGeometry {
    pub(super) plot: Bounds<Pixels>,
}

impl GraphGeometry {
    pub(super) fn new(bounds: Bounds<Pixels>) -> Self {
        Self {
            plot: Bounds::new(
                bounds.origin
                    + point(
                        px(AnimationCurveEditor::GRAPH_INSET_LEFT),
                        px(AnimationCurveEditor::GRAPH_INSET_TOP),
                    ),
                gpui::size(
                    (bounds.size.width
                        - px(AnimationCurveEditor::GRAPH_INSET_LEFT
                            + AnimationCurveEditor::GRAPH_INSET_RIGHT))
                    .max(px(0.)),
                    (bounds.size.height
                        - px(AnimationCurveEditor::GRAPH_INSET_TOP
                            + AnimationCurveEditor::GRAPH_INSET_BOTTOM))
                    .max(px(0.)),
                ),
            ),
        }
    }

    pub(super) fn position(self, normalized: [f32; 2]) -> gpui::Point<Pixels> {
        self.plot.origin
            + point(
                self.plot.size.width * normalized[0],
                self.plot.size.height * (1. - normalized[1]),
            )
    }

    pub(super) fn normalized(self, position: gpui::Point<Pixels>) -> [f32; 2] {
        [
            self.progress_at_x(f32::from(position.x)),
            1. - f32::from(position.y - self.plot.origin.y)
                / f32::from(self.plot.size.height).max(1.),
        ]
    }

    pub(super) fn progress_at_x(self, x: f32) -> f32 {
        (x - f32::from(self.plot.origin.x)) / f32::from(self.plot.size.width).max(1.)
    }
}

impl AnimationCurveEditor {
    pub(super) fn graph_geometry(&self) -> Option<GraphGeometry> {
        self.graph_bounds.map(GraphGeometry::new)
    }
}
