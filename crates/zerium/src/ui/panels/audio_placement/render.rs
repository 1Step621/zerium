use ::ui::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
};
use gpui::{Render, div, px};
use rust_i18n::t;

use crate::ui::components::placement::stamp_marker;

use super::*;

impl Render for AudioPlacement {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows(cx);
        let stamp = self.current_stamp(&rows, cx);
        let colors = cx.theme().colors;
        div()
            .id("audio-placement")
            .track_focus(&self.focus)
            .size_full()
            .min_h_0()
            .text_sm()
            .flex()
            .flex_col()
            .p_2()
            .gap_2()
            .when(rows.is_empty(), |view| {
                view.child(
                    div().flex_1().flex().items_center().justify_center().child(
                        Button::new("load-audio-folder")
                            .icon(IconName::Folder)
                            .label(
                                if self.import_task.is_some() {
                                    t!("audio_placement.loading")
                                } else {
                                    t!("audio_placement.load")
                                }
                                .to_string(),
                            )
                            .disabled(self.import_task.is_some() || self.layer.is_none())
                            .on_click(cx.listener(|this, _, window, cx| this.prompt(window, cx))),
                    ),
                )
            })
            .when(!rows.is_empty(), |view| {
                view.child(
                    div()
                        .id("audio-placement-rows")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .children(rows.iter().enumerate().map(|(index, row)| {
                            let id = row.id;
                            let selected = self.editor.read(cx).is_item_selected(id);
                            let starting = stamp == Some(StampTarget::Start(id));
                            let name = row
                                .path
                                .file_name()
                                .unwrap_or(row.path.as_os_str())
                                .to_string_lossy()
                                .into_owned();
                            div()
                                .id(("audio-placement-row", id.get()))
                                .flex()
                                .flex_col()
                                .when(starting, |view| {
                                    view.child(stamp_marker(
                                        t!("audio_placement.stamp_start").to_string().into(),
                                        window,
                                        cx,
                                    ))
                                })
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .p_1()
                                        .border_l_2()
                                        .border_color(gpui::transparent_black())
                                        .when(selected, |view| {
                                            view.bg(colors.primary.opacity(0.04))
                                        })
                                        .when(starting, |view| {
                                            view.border_color(colors.primary)
                                                .bg(colors.primary.opacity(0.08))
                                        })
                                        .child(
                                            Button::new(("audio-placement-select", id.get()))
                                                .small()
                                                .ghost()
                                                .w(px(24.))
                                                .label((index + 1).to_string())
                                                .when(selected, |button| button.primary())
                                                .tooltip(t!("placement.select_row").to_string())
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.select_row(id, cx)
                                                })),
                                        )
                                        .child(
                                            div()
                                                .id(("audio-placement-name", id.get()))
                                                .min_w_0()
                                                .flex_1()
                                                .truncate()
                                                .cursor_pointer()
                                                .child(name)
                                                .tooltip({
                                                    let path = row.path.display().to_string();
                                                    move |window, cx| {
                                                        ::ui::tooltip::Tooltip::new(path.clone())
                                                            .build(window, cx)
                                                    }
                                                })
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.select_row(id, cx)
                                                })),
                                        ),
                                )
                        }))
                        .when(matches!(stamp, Some(StampTarget::Done(_))), |view| {
                            view.child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child(t!("placement.timing_done").to_string()),
                            )
                        }),
                )
                .child(
                    div().flex_none().flex().justify_end().child(
                        Button::new("audio-placement-clear")
                            .small()
                            .ghost()
                            .label(t!("placement.clear").to_string())
                            .tooltip(t!("placement.clear_hint").to_string())
                            .on_click(cx.listener(|this, _, window, cx| this.clear(window, cx))),
                    ),
                )
            })
            .when_some(self.error.clone(), |view, error| {
                view.child(div().text_color(colors.danger).child(error))
            })
    }
}
