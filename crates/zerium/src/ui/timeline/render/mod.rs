//! Timeline pane composition and per-frame render state.
use crate::ui::timeline::timeline_grid;
use crate::ui::timeline::track_viewport_width;
pub(in crate::ui::timeline) use item::timeline_item;
pub(in crate::ui::timeline) use layer::layer_row;
mod item;
mod layer;

use crate::ui::timeline::stretch::StretchPointDrag;
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
    Frame, FrameRate, ItemId, LayerId, PropertyAddress, TimelineEditor, TimelineItem,
};

use crate::ui::transport::TransportController;

use crate::ui::timeline::{
    Timeline,
    interaction::{
        drop::FileDropTarget,
        item::{MoveTimelineItem, ResizeTimelineItem},
        keyframe::MoveAnimationStop,
    },
    ruler::TimelineGrid,
    viewport::TimelineViewport,
};

use crate::ui::operation_hints::Hint;

#[derive(Clone, Copy)]
pub(in crate::ui::timeline) struct RenderResultHighlight {
    pub(in crate::ui::timeline) top_layer: LayerId,
    pub(in crate::ui::timeline) bottom_layer: LayerId,
    pub(in crate::ui::timeline) start: Frame,
    pub(in crate::ui::timeline) end: Frame,
}

#[derive(Clone)]
pub(in crate::ui::timeline) struct LayerRenderState {
    pub(in crate::ui::timeline) editor: Entity<TimelineEditor>,
    pub(in crate::ui::timeline) transport: Entity<TransportController>,
    pub(in crate::ui::timeline) animation_address: Option<PropertyAddress>,
    pub(in crate::ui::timeline) focused_animation_segment: Option<usize>,
    pub(in crate::ui::timeline) colors: ThemeColor,
    pub(in crate::ui::timeline) layer_height: f32,
    pub(in crate::ui::timeline) viewport: TimelineViewport,
    pub(in crate::ui::timeline) viewport_width: f32,
    pub(in crate::ui::timeline) frame_rate: FrameRate,
    pub(in crate::ui::timeline) playhead_seconds: f64,
    pub(in crate::ui::timeline) stretch_point: Option<Frame>,
    pub(in crate::ui::timeline) selected_item_ids: Rc<HashSet<ItemId>>,
    pub(in crate::ui::timeline) hidden_layers: Rc<HashSet<LayerId>>,
    pub(in crate::ui::timeline) hidden_items: Rc<HashSet<ItemId>>,
    pub(in crate::ui::timeline) render_result_highlights: Rc<Vec<RenderResultHighlight>>,
    pub(in crate::ui::timeline) file_drop_target: Option<FileDropTarget>,
    pub(in crate::ui::timeline) grid: TimelineGrid,
}

pub(in crate::ui::timeline) struct TimelineItemRenderData {
    pub(in crate::ui::timeline) item: TimelineItem,
    pub(in crate::ui::timeline) label: String,
}

pub(in crate::ui::timeline) fn navigation_hints() -> Vec<Hint> {
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
        let layer_count = self.dynamic_layer_count(window, cx);
        let colors = cx.theme().colors;
        let viewport_width = track_viewport_width(window);
        let layer_height = self.viewport.layer_height;
        let (
            frame_rate,
            playhead,
            playhead_seconds,
            selected_item_ids,
            render_result_highlights,
            hidden_layers,
            hidden_items,
            active_scene,
            stretch_point,
        ) = {
            let editor = self.editor.read(cx);
            let selected_items = editor.selected_items();
            let render_result_highlights = selected_items
                .iter()
                .flat_map(|item| {
                    let Some(source_layer) = editor.item_layer(item.id) else {
                        return Vec::new();
                    };
                    item.render_result_ranges()
                        .filter_map(|settings| {
                            let (top_layer, bottom_layer) = settings.layer_bounds(source_layer)?;
                            Some(RenderResultHighlight {
                                top_layer,
                                bottom_layer,
                                start: item.start,
                                end: item.end_exclusive(),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            (
                editor.frame_rate(),
                editor.playhead(),
                editor.playhead_seconds(),
                Rc::new(selected_items.iter().map(|item| item.id).collect()),
                Rc::new(render_result_highlights),
                Rc::new(editor.hidden_layer_ids().collect()),
                Rc::new(editor.hidden_item_ids().collect()),
                editor
                    .active_scene_id()
                    .and_then(|id| editor.scene(id))
                    .map(|scene| (scene.name.clone(), scene.is_empty())),
                editor
                    .active_scene_id()
                    .and_then(|id| editor.scene(id))
                    .map(|scene| scene.stretch_point()),
            )
        };
        self.viewport
            .follow_playhead(playhead, viewport_width, frame_rate);
        let active_scene_is_empty = active_scene.is_some_and(|(_, is_empty)| is_empty);
        let grid = timeline_grid(
            self.viewport,
            viewport_width,
            frame_rate,
            self.editor.read(cx).beat_guide(),
        );
        let ruler = self.ruler(colors, viewport_width, grid.ruler_ticks.clone(), cx);
        let animation_selection = self.animation_selection.read(cx);
        let row_state = LayerRenderState {
            editor: self.editor.clone(),
            transport: self.transport.clone(),
            animation_address: animation_selection.address().cloned(),
            focused_animation_segment: animation_selection.focused_segment(),
            colors,
            layer_height,
            viewport: self.viewport,
            viewport_width,
            frame_rate,
            playhead_seconds,
            stretch_point,
            selected_item_ids,
            render_result_highlights,
            hidden_layers,
            hidden_items,
            file_drop_target: self.file_drop_target,
            grid,
        };
        let timeline = cx.entity();
        let timeline_for_mouse_leave = timeline.clone();
        let file_drop_error = self.file_drop_error.clone();
        let focus_handle = self.focus_handle.clone();

        div()
            .id("timeline-root")
            .relative()
            .track_focus(&self.focus_handle)
            .capture_any_mouse_down(move |_, window, cx| {
                focus_handle.focus(window, cx);
            })
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_mouse_move(cx.listener(|this, event, window, cx| {
                this.update_playhead_scrub(event, window, cx);
                this.update_marquee_selection(event, window, cx);
            }))
            .on_mouse_leave(move |_, cx| {
                timeline_for_mouse_leave.update(cx, |timeline, cx| {
                    if timeline.marquee_selection.take().is_some() {
                        cx.notify();
                    }
                });
            })
            .on_click(cx.listener(Self::clear_selection_on_double_click))
            .capture_any_mouse_up(cx.listener(|this, event: &MouseUpEvent, window, cx| {
                if event.button == MouseButton::Left {
                    this.finish_playhead_scrub(cx);
                    this.finish_item_move(cx);
                    this.finish_marquee_selection(event, window, cx);
                } else if event.button == MouseButton::Right {
                    this.finish_marquee_selection(event, window, cx);
                }
            }))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<MoveTimelineItem>, window, cx| {
                    let drag = event.drag(cx).clone();
                    cx.set_active_drag_cursor_style(CursorStyle::ClosedHand, window);
                    this.move_item_from_pointer(
                        &drag,
                        f32::from(event.event.position.x),
                        f32::from(event.event.position.y),
                        event.event.modifiers.alt,
                        window,
                        cx,
                    );
                },
            ))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<ResizeTimelineItem>, window, cx| {
                    let drag = event.drag(cx).clone();
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
                    layer_count,
                    move |visible_range, _window, cx| {
                        timeline.update(cx, |timeline, cx| {
                            visible_range
                                .map(|layer_index| {
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
            .when_some(self.marquee_overlay(colors), |this, marquee| {
                this.child(marquee)
            })
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
