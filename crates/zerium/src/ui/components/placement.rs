use ::ui::{
    ActiveTheme as _, Kbd, Sizable as _,
    button::{Button, ButtonVariants as _},
};
use gpui::{App, Div, SharedString, Window, div, prelude::*, px};

use crate::app::actions::StampPlacement;

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
