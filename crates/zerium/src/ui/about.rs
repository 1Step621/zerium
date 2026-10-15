use std::sync::Arc;

use ::ui::{
    ActiveTheme as _, ContextModal as _, IconName, StyledExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    modal::{Modal, ModalButtonProps},
};
use gpui::{
    App, Image, ImageFormat, Window, div, gradient_color_stop, img, linear_gradient, prelude::*,
    px, relative,
};
use rust_i18n::t;

pub(super) fn open(window: &mut Window, cx: &mut App) {
    let logo = Arc::new(Image::from_bytes(
        ImageFormat::Svg,
        include_bytes!("../../../../assets/zerium.svg").to_vec(),
    ));
    window.open_modal(cx, move |modal: Modal, _, cx| {
        let colors = cx.theme().colors;
        modal
            .width(px(520.))
            .p_0()
            .rounded(px(16.))
            .overflow_hidden()
            .show_close(false)
            .overlay_closable(true)
            .button_props(
                ModalButtonProps::default()
                    .ok_text(t!("common.close").to_string())
                    .ok_variant(ButtonVariant::Ghost),
            )
            .footer(|ok, _, window, cx| {
                vec![
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .border_t_1()
                        .border_color(cx.theme().border.opacity(0.5))
                        .px_6()
                        .py_3()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    t!("about.license", license = env!("CARGO_PKG_LICENSE"))
                                        .to_string(),
                                ),
                        )
                        .child(ok(window, cx)),
                ]
            })
            .child(
                div()
                    .font_family(super::theme::FONT_FAMILY)
                    .flex()
                    .items_center()
                    .gap_6()
                    .p_6()
                    .rounded_t(px(16.))
                    .bg(linear_gradient(
                        90.,
                        gradient_color_stop(colors.accent, 0.),
                        gradient_color_stop(colors.background, 1.),
                    ))
                    .child(img(logo.clone()).size(px(128.)).flex_none())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_5()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .flex_wrap()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_size(px(32.))
                                            .line_height(relative(1.2))
                                            .font_bold()
                                            .child("Zerium"),
                                    )
                                    .child(
                                        div()
                                            .px_3()
                                            .py_1()
                                            .rounded_full()
                                            .bg(colors.primary.opacity(0.1))
                                            .text_sm()
                                            .text_color(colors.accent_foreground)
                                            .child(
                                                t!(
                                                    "about.version",
                                                    version = env!("CARGO_PKG_VERSION")
                                                )
                                                .to_string(),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex()
                                            .gap_3()
                                            .child(
                                                Button::new("about-website")
                                                    .flex_1()
                                                    .icon(IconName::Globe)
                                                    .label(t!("about.website").to_string())
                                                    .on_click(|_, _, cx| {
                                                        cx.open_url("https://zerium.pages.dev");
                                                    }),
                                            )
                                            .child(
                                                Button::new("about-github")
                                                    .flex_1()
                                                    .icon(IconName::Github)
                                                    .label("GitHub")
                                                    .on_click(|_, _, cx| {
                                                        cx.open_url(
                                                            "https://github.com/1Step621/zerium",
                                                        );
                                                    }),
                                            ),
                                    )
                                    .child(
                                        Button::new("about-ko-fi")
                                            .primary()
                                            .w_full()
                                            .label(t!("support.open_ko_fi").to_string())
                                            .on_click(|_, _, cx| {
                                                cx.open_url(super::support::KO_FI_URL);
                                            }),
                                    ),
                            ),
                    ),
            )
    });
}
