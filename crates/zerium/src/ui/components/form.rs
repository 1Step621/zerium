//! Shared layout for settings forms and read-only confirmation dialogs.
use ::ui::{Size, StyledExt as _, modal::Modal};
use gpui::{Div, SharedString, Window, div, prelude::*, px};

pub(crate) fn form_dialog(modal: Modal, title: impl Into<SharedString>, window: &Window) -> Modal {
    modal
        .font_family(crate::ui::theme::FONT_FAMILY)
        .text_sm()
        .title(div().text_lg().font_semibold().child(title.into()))
        .width(px(480.))
        .max_h(window.viewport_size().height * 0.8)
        .confirm()
}

pub(crate) fn form_body() -> Div {
    div().w_full().flex().flex_col().gap_5()
}

pub(crate) fn form_fields() -> Div {
    div().w_full().flex().flex_col().gap_2()
}

pub(crate) fn form_section(title: impl Into<SharedString>) -> Div {
    form_fields().child(div().text_base().font_semibold().child(title.into()))
}

pub(crate) fn form_row(label: impl Into<SharedString>, value: impl IntoElement) -> Div {
    div()
        .w_full()
        .min_h(Size::Small.control_height())
        .flex()
        .items_center()
        .gap_3()
        .child(div().w(px(160.)).flex_none().child(label.into()))
        .child(div().min_w_0().flex_1().child(value))
}
