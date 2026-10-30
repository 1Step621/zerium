use ::ui::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Input,
    menu::{PopupMenuItem, popup_menu::PopupMenuExt as _},
};
use gpui::{Render, div};

use crate::ui::components::placement::{placement_row, row_selector, stamp_marker, timing_done};

use super::*;

impl Render for TextPlacement {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows(cx);
        self.sync_inputs(&rows, window, cx);
        let colors = cx.theme().colors;
        let stamp = self.current_stamp(&rows, cx);
        div()
            .id("text-placement")
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
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(
                            Button::new("load-text")
                                .icon(IconName::Plus)
                                .label(t!("text_placement.load").to_string())
                                .disabled(
                                    self.layer.is_none()
                                        || self
                                            .target
                                            .as_ref()
                                            .and_then(|target| target.schema(self.editor.read(cx)))
                                            .is_none(),
                                )
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.prompt(window, cx)),
                                ),
                        )
                        .child(self.target_selector(cx)),
                )
            })
            .when(!rows.is_empty(), |view| {
                view.child(
                    div()
                        .id("text-placement-rows")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .children(rows.iter().enumerate().map(|(index, row)| {
                            let id = row.address.item_id;
                            let selected = self.editor.read(cx).is_item_selected(id);
                            let starting = stamp == Some(StampTarget::Start(id));
                            let ending = stamp == Some(StampTarget::End(id));
                            div()
                                .id(("text-placement-row", id.get()))
                                .flex()
                                .flex_col()
                                .when(starting, |view| {
                                    view.child(self.stamp_marker(false, window, cx))
                                })
                                .child(
                                    placement_row(selected, starting || ending, colors)
                                        .child(
                                            row_selector(
                                                ("text-placement-select", id.get()),
                                                index,
                                                selected,
                                            )
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    this.select_row(id, cx)
                                                }),
                                            ),
                                        )
                                        .child(
                                            div().flex_1().min_w_0().child(
                                                Input::new(
                                                    &self
                                                        .inputs
                                                        .text(&row.address)
                                                        .expect("row input"),
                                                )
                                                .small()
                                                .disabled(!row.editable),
                                            ),
                                        ),
                                )
                                .when(ending, |view| {
                                    view.child(self.stamp_marker(true, window, cx))
                                })
                        }))
                        .when(matches!(stamp, Some(StampTarget::Done(_))), |view| {
                            view.child(timing_done(colors))
                        }),
                )
                .child(
                    div().flex_none().flex().justify_end().child(
                        Button::new("text-placement-clear")
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

impl TextPlacement {
    fn target_selector(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let options = Target::available(self.editor.read(cx));
        let selected = self.target.clone();
        let label = options
            .iter()
            .find(|(target, _)| Some(target) == selected.as_ref())
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| t!("text_placement.choose_target").to_string());
        let panel = cx.entity();
        Button::new("text-placement-target")
            .small()
            .compact()
            .ghost()
            .max_w_full()
            .min_w_0()
            .overflow_hidden()
            .dropdown_caret(true)
            .child(div().min_w_0().truncate().child(label.clone()))
            .tooltip(label)
            .disabled(options.is_empty())
            .popup_menu(move |menu, _, _| {
                options.iter().fold(menu, |menu, (target, label)| {
                    let target = target.clone();
                    let checked = Some(&target) == selected.as_ref();
                    let panel = panel.clone();
                    menu.item(PopupMenuItem::new(label.clone()).checked(checked).on_click(
                        move |_, _, cx| {
                            panel.update(cx, |this, cx| this.set_target(target.clone(), cx));
                        },
                    ))
                })
            })
            .into_any_element()
    }

    fn stamp_marker(&self, end: bool, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        stamp_marker(
            if end {
                t!("text_placement.stamp_end")
            } else {
                t!("text_placement.stamp_start")
            }
            .to_string()
            .into(),
            window,
            cx,
        )
    }
}
