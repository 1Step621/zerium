use ::ui::{
    ActiveTheme as _, Kbd, Sizable as _, ThemeColor,
    button::{Button, ButtonVariants as _},
};
use gpui::{App, Div, ElementId, SharedString, Window, div, prelude::*, px};
use rust_i18n::t;

use crate::app::actions::StampPlacement;

pub(crate) fn placement_row(selected: bool, active: bool, colors: ThemeColor) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .p_1()
        .border_l_2()
        .border_color(gpui::transparent_black())
        .when(selected, |view| view.bg(colors.primary.opacity(0.04)))
        .when(active, |view| {
            view.border_color(colors.primary)
                .bg(colors.primary.opacity(0.08))
        })
}

pub(crate) fn row_selector(id: impl Into<ElementId>, index: usize, selected: bool) -> Button {
    Button::new(id)
        .small()
        .ghost()
        .w(px(24.))
        .label((index + 1).to_string())
        .when(selected, |button| button.primary())
        .tooltip(t!("placement.select_row").to_string())
}

pub(crate) fn timing_done(colors: ThemeColor) -> Div {
    div()
        .px_2()
        .py_1()
        .text_xs()
        .text_color(colors.muted_foreground)
        .child(t!("placement.timing_done").to_string())
}

pub(crate) fn stamp_marker(label: SharedString, window: &Window, cx: &App) -> Div {
    let color = cx.theme().colors.primary;
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(div().w(px(32.)).h(px(1.)).bg(color))
        .child(
            Button::new("placement-stamp")
                .small()
                .compact()
                .ghost()
                .child(div().text_color(color).child(label))
                .when_some(
                    Kbd::binding_for_action(
                        &StampPlacement,
                        Some("ZeriumWorkspace PlacementTiming"),
                        window,
                    ),
                    |button, shortcut| button.child(shortcut.appearance(false)),
                )
                .on_click(|_, window, cx| window.dispatch_action(Box::new(StampPlacement), cx)),
        )
        .child(div().flex_1().h(px(1.)).bg(color))
}
