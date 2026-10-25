use std::{cell::Cell, rc::Rc};

use gpui::{ClickEvent, Context, EntityId, MouseButton, div, prelude::*, px};

use zerium_core::timeline::{Frame, ResizeEdge, ResizeMode, TimelineItem};

use crate::ui::{
    TimelineEditorEntityExt as _,
    panels::timeline::{
        Timeline,
        interaction::item::{MoveTimelineItem, ResizeTimelineItem},
    },
    workspace::operation_hints::{Hint, OperationHintExt as _},
};

use super::{LayerRenderState, TimelineItemRenderData, navigation_hints};

fn resize_hints() -> Vec<Hint> {
    vec![
        Hint::gesture("hints.drag", "hints.stretch", gpui::Modifiers::shift()),
        Hint::gesture("hints.drag", "hints.no_snap", gpui::Modifiers::alt()),
    ]
}

pub(in crate::ui::panels::timeline) fn timeline_item(
    render_item: TimelineItemRenderData,
    state: &LayerRenderState,
    timeline_id: EntityId,
    clip_top: f32,
    clip_height: f32,
    visible_range: (f64, f64),
    cx: &mut Context<Timeline>,
) -> Option<gpui::AnyElement> {
    let TimelineItemRenderData {
        item,
        label: item_label,
    } = render_item;
    let item_start = state.frame_rate.frame_to_seconds(item.start());
    let item_end = state
        .frame_rate
        .frame_to_seconds(item.end_exclusive(state.frame_rate));
    let (visible_start, visible_end) = visible_range;

    if item_end <= visible_start || item_start >= visible_end {
        return None;
    }

    let item_id = item.id;
    let scene_id = item.scene_id();
    let item_hidden = state.hidden_items.contains(&item_id);
    let item_left = state.viewport.x_at_seconds(item_start);
    let item_width = (state
        .frame_rate
        .frame_to_seconds(Frame::new(item.duration(state.frame_rate).get()))
        * state.viewport.pixels_per_second()) as f32;
    let is_selected = state.selected_item_ids.contains(&item_id);
    let animation_markers = crate::ui::panels::timeline::render::animation::key_markers(
        &item,
        state,
        item_left,
        item_width,
        visible_range,
        cx,
    );

    let move_drag = MoveTimelineItem {
        timeline_id,
        item_id,
    };
    let resize_origins =
        Rc::<[TimelineItem]>::from(if state.selected_item_ids.contains(&item_id) {
            state.editor.read(cx).selected_items()
        } else {
            vec![item.clone()]
        });
    let left_drag = ResizeTimelineItem {
        timeline_id,
        origins: resize_origins.clone(),
        anchor_id: item_id,
        edge: ResizeEdge::Left,
        mode: Cell::new(ResizeMode::Trim),
    };
    let right_drag = ResizeTimelineItem {
        timeline_id,
        origins: resize_origins,
        anchor_id: item_id,
        edge: ResizeEdge::Right,
        mode: Cell::new(ResizeMode::Trim),
    };
    Some(
        div()
            .id(("timeline-item", item_id.get()))
            .absolute()
            .top(px(clip_top))
            .left(px(item_left))
            .h(px(clip_height))
            .w(px(item_width))
            .min_w_0()
            .max_w(px(item_width))
            .overflow_hidden()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event, _, cx| {
                    this.begin_item_interaction(item_id, event, cx);
                }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if !event.modifiers().shift {
                    this.editor
                        .update_if_changed(cx, |editor| editor.select(item_id));
                }
                let seek_frame = {
                    let editor = this.editor.read(cx);
                    editor
                        .item(item_id)
                        .filter(|item| {
                            editor.is_item_selected(item_id)
                                && !item.contains(editor.playhead(), editor.frame_rate())
                        })
                        .map(|item| item.start())
                };
                if let Some(frame) = seek_frame {
                    this.transport
                        .update(cx, |transport, cx| transport.set_playhead(frame, cx));
                }
                this.open_scene_item(item_id, event, cx);
                cx.stop_propagation();
            }))
            .on_drag(move_drag, |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_item_move(cx)),
            )
            .child(
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .overflow_hidden()
                    .rounded_sm()
                    .border_1()
                    .border_color(if is_selected {
                        state.colors.primary
                    } else {
                        state.colors.border
                    })
                    .bg(if is_selected {
                        state.colors.primary.opacity(0.24)
                    } else if item_hidden {
                        state.colors.accent.opacity(0.35)
                    } else {
                        state.colors.accent
                    })
                    .when(is_selected, |item| item.border_2())
                    .text_sm()
                    .text_color(if item_hidden {
                        state.colors.muted_foreground
                    } else {
                        state.colors.accent_foreground
                    })
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap_1()
                            .ml(px((-item_left).max(0.)))
                            .pl_1()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .when(scene_id.is_none(), |this| {
                                this.child(
                                    div()
                                        .text_color(state.colors.accent_foreground)
                                        .child(item.symbol().to_owned()),
                                )
                            })
                            .child(item_label),
                    )
                    .child(
                        div()
                            .id(("timeline-item-left-handle", item_id.get()))
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left_0()
                            .w(px(6.))
                            .cursor_col_resize()
                            .bg(state.colors.primary.opacity(0.35))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.begin_item_resize(item_id, cx);
                                }),
                            )
                            .on_drag(left_drag, |drag, _, window, cx| {
                                cx.stop_propagation();
                                drag.mode.set(if window.modifiers().shift {
                                    ResizeMode::Stretch
                                } else {
                                    ResizeMode::Trim
                                });
                                cx.new(|_| drag.clone())
                            })
                            .operation_hints(navigation_hints().into_iter().chain(resize_hints())),
                    )
                    .child(
                        div()
                            .id(("timeline-item-right-handle", item_id.get()))
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right_0()
                            .w(px(6.))
                            .cursor_col_resize()
                            .bg(state.colors.primary.opacity(0.35))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.begin_item_resize(item_id, cx);
                                }),
                            )
                            .on_drag(right_drag, |drag, _, window, cx| {
                                cx.stop_propagation();
                                drag.mode.set(if window.modifiers().shift {
                                    ResizeMode::Stretch
                                } else {
                                    ResizeMode::Trim
                                });
                                cx.new(|_| drag.clone())
                            })
                            .operation_hints(navigation_hints().into_iter().chain(resize_hints())),
                    ),
            )
            .children(animation_markers)
            .operation_hints(navigation_hints().into_iter().chain([
                Hint::gesture(
                    "hints.click",
                    "hints.toggle_selection",
                    gpui::Modifiers::shift(),
                ),
                Hint::gesture("hints.drag", "hints.no_snap", gpui::Modifiers::alt()),
            ]))
            .into_any_element(),
    )
}
