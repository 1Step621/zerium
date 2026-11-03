use super::Timeline;
use crate::ui::{
    components::pane::PANE_HEADER_HEIGHT,
    panels::timeline::{LAYER_HEADER_WIDTH, layer_scroll_base},
};
use ::ui::ThemeColor;
use gpui::{Bounds, ContentMask, Context, canvas, point, prelude::*, px, quad};
use zerium_core::timeline::LayerId;

impl Timeline {
    pub(super) fn capture_ranges(
        &self,
        colors: ThemeColor,
        cx: &Context<Self>,
    ) -> Option<impl IntoElement> {
        let editor = self.editor.read(cx);
        let frame_rate = editor.frame_rate();
        let mut ranges = Vec::new();
        for id in editor.selected_item_ids() {
            let Some(item) = editor.item(id) else {
                continue;
            };
            let Some(source_layer) = editor.item_layer(id) else {
                continue;
            };
            for settings in editor.resolved_item(item).render_result_ranges() {
                let Some((top, bottom)) = settings.layer_bounds(source_layer) else {
                    continue;
                };
                let top_row = self.rows.layer_row(top);
                let end_row = self
                    .rows
                    .layer_row(LayerId::new(bottom.get().saturating_add(1)));
                let range = Bounds::from_corners(
                    point(
                        px(self
                            .viewport
                            .x_at_seconds(frame_rate.frame_to_seconds(item.start()))),
                        px(top_row as f32 * self.viewport.layer_height),
                    ),
                    point(
                        px(self.viewport.x_at_seconds(
                            frame_rate.frame_to_seconds(item.end_exclusive(frame_rate)),
                        )),
                        px(end_row as f32 * self.viewport.layer_height),
                    ),
                );
                if !ranges.contains(&range) {
                    ranges.push(range);
                }
            }
        }
        if ranges.is_empty() {
            return None;
        }
        let scroll = layer_scroll_base(&self.layer_scroll);
        Some(
            canvas(
                move |_, _, _| ranges,
                move |bounds, ranges, window, _| {
                    let origin = bounds.origin + point(px(0.), scroll.offset().y);
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        for range in ranges {
                            let range = Bounds::new(origin + range.origin, range.size);
                            if range.intersects(&bounds) {
                                window.paint_quad(quad(
                                    range,
                                    px(0.),
                                    colors.primary.opacity(0.04),
                                    px(1.),
                                    colors.primary.opacity(0.65),
                                    Default::default(),
                                ));
                            }
                        }
                    });
                },
            )
            .absolute()
            .top(px(PANE_HEADER_HEIGHT))
            .left(px(LAYER_HEADER_WIDTH))
            .right_0()
            .bottom_0(),
        )
    }
}
