//! Timeline pane composition and per-frame render state.
pub(super) mod ruler;
pub(super) use self::{item::timeline_item, layer::layer_row};
use super::{
    Timeline,
    interaction::{
        drop::FileDropTarget,
        item::{MoveTimelineItem, ResizeTimelineItem},
        keyframe::MoveAnimationStop,
        stretch::StretchPointDrag,
    },
    timeline_grid, track_viewport_width,
    viewport::TimelineViewport,
};
use crate::{
    app::transport::TransportController,
    ui::{components::animation::AnimationSynchronization, workspace::operation_hints::Hint},
};
mod animation;
mod item;
mod layer;

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

use self::ruler::TimelineGrid;

#[derive(Clone, Copy)]
pub(super) struct RenderResultHighlight {
    pub(super) top_layer: LayerId,
    pub(super) bottom_layer: LayerId,
    pub(super) start: Frame,
    pub(super) end: Frame,
}

#[derive(Clone)]
pub(super) struct LayerRenderState {
    pub(super) editor: Entity<TimelineEditor>,
    pub(super) transport: Entity<TransportController>,
    pub(super) animation_address: Option<PropertyAddress>,
    pub(super) synchronize_animation_keys: bool,
    pub(super) animation_drag_synchronization:
        Option<(MoveAnimationStop, AnimationSynchronization)>,
    pub(super) colors: ThemeColor,
    pub(super) layer_height: f32,
    pub(super) viewport: TimelineViewport,
    pub(super) viewport_width: f32,
    pub(super) frame_rate: FrameRate,
    pub(super) playhead_seconds: f64,
    pub(super) stretch_point: Option<Frame>,
    pub(super) selected_item_ids: Rc<HashSet<ItemId>>,
    pub(super) hidden_layers: Rc<HashSet<LayerId>>,
    pub(super) render_result_highlights: Rc<Vec<RenderResultHighlight>>,
    pub(super) file_drop_target: Option<FileDropTarget>,
    pub(super) grid: TimelineGrid,
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
            active_scene_is_empty,
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
                    editor
                        .resolved_item(item)
                        .render_result_ranges()
                        .filter_map(|settings| {
                            let (top_layer, bottom_layer) = settings.layer_bounds(source_layer)?;
                            Some(RenderResultHighlight {
                                top_layer,
                                bottom_layer,
                                start: item.start(),
                                end: item.end_exclusive(editor.frame_rate()),
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
                editor.active_scene_id() != editor.main_scene_id()
                    && editor.active_scene().is_empty(),
                (editor.active_scene_id() != editor.main_scene_id())
                    .then(|| editor.active_scene().stretch_point()),
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
        let animation_selection = self.animation_selection.read(cx);
        let row_state = LayerRenderState {
            editor: self.editor.clone(),
            transport: self.transport.clone(),
            animation_address: animation_selection.address().cloned(),
            synchronize_animation_keys: !window.modifiers().alt,
            animation_drag_synchronization: self
                .animation_drag
                .as_ref()
                .and_then(|drag| drag.current_edit(self.editor.read(cx)))
                .map(|(source, edit)| {
                    (
                        source.clone(),
                        AnimationSynchronization::from_edit(&self.editor, edit),
                    )
                }),
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
                    this.update_marquee_selection(event, window, cx);
                }),
            )
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
                    if this.animation_drag.take().is_some() {
                        cx.notify();
                    }
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
                        event.event.modifiers,
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
