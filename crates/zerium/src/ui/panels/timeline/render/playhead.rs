use crate::ui::panels::timeline::LAYER_HEADER_WIDTH;
use gpui::{Bounds, ContentMask, Hsla, canvas, fill, point, prelude::*, px, size};

pub(super) fn playhead_overlay(
    x: f32,
    viewport_width: f32,
    color: Hsla,
) -> Option<impl IntoElement> {
    if x < -4. || x > viewport_width + 4. {
        return None;
    }
    Some(
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let origin = bounds.origin + point(px(x), px(0.));
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    window.paint_quad(fill(
                        Bounds::new(origin, size(px(2.), bounds.size.height)),
                        color,
                    ));
                    let mut cap = fill(
                        Bounds::new(origin - point(px(4.), px(0.)), size(px(8.), px(8.))),
                        color,
                    );
                    cap.corner_radii.bottom_left = px(2.);
                    cap.corner_radii.bottom_right = px(2.);
                    window.paint_quad(cap);
                });
            },
        )
        .absolute()
        .top_0()
        .left(px(LAYER_HEADER_WIDTH))
        .right_0()
        .bottom_0(),
    )
}
