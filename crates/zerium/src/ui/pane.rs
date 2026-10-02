use ::ui::ThemeColor;
use gpui::{Div, div, prelude::*, px};

pub(crate) const PANE_HEADER_HEIGHT: f32 = 34.;

pub(crate) fn pane_header(colors: ThemeColor) -> Div {
    div()
        .w_full()
        .h(px(PANE_HEADER_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .px_3()
        .border_b_1()
        .border_color(colors.border)
        .bg(colors.title_bar)
        .text_sm()
        .text_color(colors.primary)
}
