//! Timeline pane composition and per-frame render state.
pub(super) mod ruler;
pub(super) use self::{
    item::timeline_item,
    layer::{layer_row, timeline_track},
};
use super::{
    Timeline,
    interaction::{
        drop::FileDropTarget,
        item::{MoveTimelineItem, ResizeTimelineItem},
        keyframe::MoveAnimationStop,
        stretch::StretchPointDrag,
    },
    timeline_grid,
    viewport::TimelineViewport,
};
use crate::ui::workspace::operation_hints::Hint;
mod capture;
mod item;
mod layer;
mod playhead;

use rust_i18n::t;
use std::{collections::HashSet, rc::Rc};

use ::ui::{
    ActiveTheme as _, IconName, Sizable as _, ThemeColor,
    button::{Button, ButtonVariants as _},
};
use gpui::{
    Context, CursorStyle, DragMoveEvent, Entity, MouseButton, MouseUpEvent, Render, Window, div,
    prelude::*, px, uniform_list,
};

use zerium_core::timeline::{
    FrameRate, ItemId, LayerId, PropertyAddress, TimelineEditor, TimelineItem,
};

use self::ruler::TimelineGrid;

#[derive(Clone)]
pub(super) struct LayerRenderState {
    pub(super) editor: Entity<TimelineEditor>,
    pub(super) expanded_layers: Rc<HashSet<LayerId>>,
    pub(super) selected_stops: Rc<super::dope_sheet::StopKeys>,
    pub(super) focused_animation: Option<PropertyAddress>,
    pub(super) colors: ThemeColor,
    pub(super) layer_height: f32,
    pub(super) viewport: TimelineViewport,
    pub(super) viewport_width: f32,
    pub(super) frame_rate: FrameRate,
    pub(super) selected_item_ids: Rc<HashSet<ItemId>>,
    pub(super) hidden_layers: Rc<HashSet<LayerId>>,
    pub(super) file_drop_target: Option<FileDropTarget>,
    pub(super) grid: TimelineGrid,
}

impl LayerRenderState {
    pub(super) fn layer_background(&self, layer: usize) -> gpui::Hsla {
        if layer.is_multiple_of(2) {
            self.colors.background
        } else {
            self.colors.table_even
        }
    }
}

pub(super) struct TimelineItemRenderData {
    pub(super) item: TimelineItem,
    pub(super) label: String,
}

pub(super) fn navigation_hints() -> Vec<Hint> {
    vec![
        Hint::gesture(
            "hints.right_drag",
            "hints.marquee_selection",
            gpui::Modifiers::default(),
        ),
        Hint::gesture(
            "hints.scroll",
            "hints.zoom_time",
            gpui::Modifiers::control(),
        ),
        Hint::gesture(
            "hints.scroll",
            "hints.zoom_layers",
            gpui::Modifiers::control_shift(),
        ),
        Hint::gesture(
            "hints.scroll",
            "hints.scroll_layers",
            gpui::Modifiers::alt(),
        ),
    ]
}

impl Render for Timeline {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !cx.has_active_drag() {
            self.file_drop_target = None;
        }
        self.rebuild_rows(cx);
        let row_count = self.row_count(window, cx);
        let rows = self.rows.clone();
        let colors = cx.theme().colors;
        let viewport_width = self.track_viewport_width(window);
        let layer_height = self.viewport.layer_height;
        let (
            frame_rate,
            playhead,
            playhead_seconds,
            selected_item_ids,
            hidden_layers,
            active_scene_is_empty,
        ) = {
            let editor = self.editor.read(cx);
            (
                editor.frame_rate(),
                editor.playhead(),
                editor.playhead_seconds(),
                Rc::new(editor.selected_item_ids().collect()),
                Rc::new(editor.hidden_layer_ids().collect()),
                editor.active_scene_id() != editor.main_scene_id()
                    && editor.active_scene().is_empty(),
            )
        };
        self.viewport
            .follow_playhead(playhead, viewport_width, frame_rate);
        let grid = timeline_grid(
            self.viewport,
            viewport_width,
            frame_rate,
            self.editor.read(cx).beat_guide(),
        );
        let ruler = self.ruler(colors, viewport_width, grid.ruler_ticks.clone(), cx);
        let row_state = LayerRenderState {
            editor: self.editor.clone(),
            expanded_layers: Rc::new(self.expanded_layers.clone()),
            selected_stops: Rc::new(
                self.stop_selection
                    .as_ref()
                    .map(|selection| selection.keys.clone())
                    .unwrap_or_default(),
            ),
            focused_animation: self.animation_selection.read(cx).address().cloned(),
            colors,
            layer_height,
            viewport: self.viewport,
            viewport_width,
            frame_rate,
            selected_item_ids,
            hidden_layers,
            file_drop_target: self.file_drop_target,
            grid,
        };
        let capture_ranges = self.capture_ranges(colors, cx);
        let stretch_overlay = self.stretch_overlay(colors, viewport_width, cx);
        let playhead_overlay = playhead::playhead_overlay(
            self.viewport.x_at_seconds(playhead_seconds),
            viewport_width,
            colors.primary,
        );
        let timeline = cx.entity();
        let file_drop_error = self.file_drop_error.clone();
        let focus_handle = self.focus_handle.clone();

        div()
            .id("timeline-root")
            .relative()
            .track_focus(&self.focus_handle)
            .on_modifiers_changed(cx.listener(|this, _, window, cx| {
                this.refresh_item_move(window, cx);
                cx.notify();
            }))
            .capture_any_mouse_down(move |_, window, cx| {
                focus_handle.focus(window, cx);
            })
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_mouse_move(
                cx.listener(|this, event: &gpui::MouseMoveEvent, window, cx| {
                    if event.pressed_button != Some(MouseButton::Left)
                        && this.animation_drag.take().is_some()
                    {
                        cx.notify();
                    }
                    this.update_playhead_scrub(event, window, cx);
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::app::actions::DeleteSelectedItem, _, cx| {
                    if this.stop_selection.is_none() {
                        cx.propagate();
                    } else {
                        this.delete_selected_stops(cx);
                    }
                }),
            )
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape"
                    && let Some(selection) = &mut this.stop_selection
                    && !selection.keys.is_empty()
                {
                    selection.keys.clear();
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_click(cx.listener(Self::clear_selection_on_double_click))
            .capture_any_mouse_up(cx.listener(|this, event: &MouseUpEvent, _, cx| {
                if event.button == MouseButton::Left {
                    if this.animation_drag.take().is_some() {
                        cx.notify();
                    }
                    this.finish_playhead_scrub(cx);
                    this.finish_item_move(cx);
                }
            }))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<MoveTimelineItem>, window, cx| {
                    let drag = event.drag(cx).clone();
                    cx.notify();
                    cx.set_active_drag_cursor_style(CursorStyle::ClosedHand, window);
                    this.move_item_from_pointer(
                        &drag,
                        f32::from(event.event.position.x),
                        f32::from(event.event.position.y),
                        event.event.modifiers,
                        cx,
                    );
                },
            ))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<ResizeTimelineItem>, window, cx| {
                    let drag = event.drag(cx).clone();
                    cx.notify();
                    this.resize_item_from_pointer(
                        &drag,
                        f32::from(event.event.position.x),
                        event.event.modifiers.alt,
                        window,
                        cx,
                    );
                },
            ))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<MoveAnimationStop>, window, cx| {
                    let drag = event.drag(cx).clone();
                    cx.set_active_drag_cursor_style(CursorStyle::ResizeLeftRight, window);
                    this.move_animation_stop_from_pointer(
                        &drag,
                        f32::from(event.event.position.x),
                        event.event.modifiers.alt,
                        window,
                        cx,
                    );
                },
            ))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<StretchPointDrag>, window, cx| {
                    let drag = event.drag(cx).clone();
                    cx.set_active_drag_cursor_style(CursorStyle::ResizeLeftRight, window);
                    this.move_stretch_point(&drag, &event.event, window, cx);
                },
            ))
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(ruler)
            .child(
                uniform_list(
                    "timeline-layers",
                    row_count,
                    move |visible_range, _window, cx| {
                        timeline.update(cx, |timeline, cx| {
                            visible_range
                                .map(|row_index| {
                                    let layer_index = match rows.row(row_index) {
                                        super::TimelineRow::Property(row) => {
                                            let last_in_layer =
                                                rows.row(row_index + 1).layer() != row.layer;
                                            return super::dope_sheet::animation_row(
                                                row,
                                                last_in_layer,
                                                &row_state,
                                                cx,
                                            );
                                        }
                                        super::TimelineRow::Layer(layer) => layer.get() as usize,
                                    };
                                    let items = {
                                        let editor = timeline.editor.read(cx);
                                        editor
                                            .items_on_layer(LayerId::new(layer_index as u64))
                                            .into_iter()
                                            .map(|item| {
                                                let label =
                                                    editor.item_label(item.id).unwrap_or_else(
                                                        || t!("timeline.unknown_item").to_string(),
                                                    );
                                                TimelineItemRenderData { item, label }
                                            })
                                            .collect()
                                    };
                                    layer_row(layer_index, row_state.clone(), items, cx)
                                        .into_any_element()
                                })
                                .collect::<Vec<_>>()
                        })
                    },
                )
                .track_scroll(&self.layer_scroll)
                .flex_1()
                .min_h_0()
                .w_full(),
            )
            .when_some(capture_ranges, |this, ranges| this.child(ranges))
            .when_some(stretch_overlay, |this, marker| this.child(marker))
            .child(self.marquee_overlay(colors, cx))
            .when_some(playhead_overlay, |this, playhead| this.child(playhead))
            .when_some(
                self.context_menu_overlay(colors, cx.entity()),
                |this, menu| this.child(menu),
            )
            .when(active_scene_is_empty, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap_3()
                                .px_5()
                                .py_4()
                                .rounded_md()
                                .border_1()
                                .border_color(colors.border)
                                .bg(colors.background.opacity(0.96))
                                .shadow_md()
                                .text_sm()
                                .child(t!("timeline.empty_scene").to_string())
                                .child(
                                    Button::new("timeline-delete-empty-scene")
                                        .small()
                                        .danger()
                                        .icon(IconName::Delete)
                                        .label(t!("timeline.delete_scene").to_string())
                                        .tooltip(t!("timeline.delete_scene_tooltip").to_string())
                                        .on_click(cx.listener(Self::delete_empty_scene)),
                                ),
                        ),
                )
            })
            .when_some(file_drop_error, |this, error| {
                this.child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .bottom(px(8.))
                        .max_w(px(520.))
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(colors.danger)
                        .bg(colors.background.opacity(0.96))
                        .text_sm()
                        .text_color(colors.danger)
                        .child(error),
                )
            })
    }
}
