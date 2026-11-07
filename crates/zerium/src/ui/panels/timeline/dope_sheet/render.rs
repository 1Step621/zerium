use crate::ui::components::animation::animation_stop_marker;
use crate::ui::panels::timeline::LAYER_HEIGHT;
use crate::ui::panels::timeline::Timeline;
use crate::ui::panels::timeline::{
    LAYER_HEADER_WIDTH,
    interaction::keyframe::MoveAnimationStop,
    render::{LayerRenderState, navigation_hints, timeline_track},
};
use crate::ui::workspace::operation_hints::OperationHintExt as _;
use gpui::{
    Bounds, ClickEvent, DispatchPhase, HitboxBehavior, MouseButton, MouseDownEvent, MouseMoveEvent,
    SharedString, canvas, div, fill, point, prelude::*, px, size,
};
use zerium_core::timeline::{PropertyAddress, TimelineTime};

pub(in crate::ui::panels::timeline) fn animation_row(
    row: &crate::ui::panels::timeline::rows::AnimationRow,
    last_in_layer: bool,
    state: &LayerRenderState,
    cx: &mut Context<Timeline>,
) -> gpui::AnyElement {
    let crate::ui::panels::timeline::rows::AnimationRow {
        layer,
        address,
        label,
        ..
    } = row;
    let layer_index = layer.get() as usize;
    let address = address.clone();
    let editor = state.editor.read(cx);
    let Some(item) = editor.item(address.item_id) else {
        return div().into_any_element();
    };
    let Some(track) = editor.animation_track(&address) else {
        return div().into_any_element();
    };
    let clock = item.animation_clock(track);
    let (start, end) = state.viewport.visible_time_range(state.viewport_width);
    let (visible_start, visible_end) = (
        TimelineTime::from_frames(start * state.frame_rate.frames_per_second()),
        TimelineTime::from_frames(end * state.frame_rate.frames_per_second()),
    );
    let pixels_per_frame =
        state.viewport.pixels_per_second() / state.frame_rate.frames_per_second();
    let stride = (8. / (clock.span_frames() * pixels_per_frame))
        .ceil()
        .max(1.) as usize;
    let mut ghosts = Vec::new();
    let mut keys = Vec::new();
    let timeline = cx.entity();
    let focused_segment = state
        .focused_animation
        .as_ref()
        .filter(|(target, _)| target == &address)
        .map(|(_, segment)| *segment);
    let mut segments = Vec::new();
    for (index, stops) in track.stops().windows(2).enumerate() {
        let [left, right] = [&stops[0], &stops[1]].map(|stop| {
            state
                .viewport
                .x_at_seconds(clock.key_time_at(stop.frame()).seconds(state.frame_rate))
        });
        let (left, right) = (left.max(0.), right.min(state.viewport_width));
        if right <= left {
            continue;
        }
        let selected = focused_segment == Some(index);
        let color = state.colors.primary;
        let segment_address = address.clone();
        segments.push(
            div()
                .id(("dope-segment", index))
                .absolute()
                .left(px(left))
                .w(px(right - left))
                .top(px(5.))
                .bottom(px(5.))
                .rounded_sm()
                .bg(color.opacity(if selected { 0.25 } else { 0.08 }))
                .hover(move |style| style.bg(color.opacity(if selected { 0.35 } else { 0.16 })))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        if event.modifiers.control {
                            return;
                        }
                        this.begin_track_interaction(layer_index, event, window, cx);
                        this.open_animation_curve(segment_address.clone(), Some(index), cx);
                        cx.stop_propagation();
                    }),
                ),
        );
    }
    for (index, stop) in track.stops().iter().enumerate() {
        for time in clock
            .occurrences(stop.frame(), visible_start, visible_end)
            .step_by(stride)
        {
            let x = state.viewport.x_at_seconds(time.seconds(state.frame_rate));
            ghosts.push(
                div()
                    .absolute()
                    .left(px(x - 2.))
                    .top(px(LAYER_HEIGHT * 0.5 - 2.))
                    .size(px(4.))
                    .bg(state.colors.muted_foreground.opacity(0.3)),
            );
        }
        let time = clock.key_time_at(stop.frame());
        let x = state.viewport.x_at_seconds(time.seconds(state.frame_rate));
        if x < -8. || x > state.viewport_width + 8. {
            continue;
        }
        let fixed = index == 0 || index + 1 == track.stops().len();
        let selected = state.selected_stops.contains(&(address.clone(), index));
        let key_address = address.clone();
        let click_address = address.clone();
        let menu_address = address.clone();
        let drag_timeline = timeline.clone();
        let drag = MoveAnimationStop {
            timeline_id: cx.entity_id(),
            address: address.clone(),
            stop: index,
        };
        let key = div()
            .id(("dope-stop", index))
            .absolute()
            .left(px(x - 7.))
            .top(px(LAYER_HEIGHT * 0.5 - 7.))
            .size(px(14.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .when(!fixed, |key| key.cursor(gpui::CursorStyle::ResizeLeftRight))
            .child(animation_stop_marker(fixed, selected, state.colors))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    if !fixed {
                        this.begin_stop_interaction(key_address.clone(), index, event, cx);
                    } else {
                        this.edit_stops(cx).keys.clear();
                        this.transport.update(cx, |transport, cx| {
                            transport.set_playhead(time.nearest_frame(), cx)
                        });
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if !fixed {
                    this.select_stop(click_address.clone(), index, event.modifiers().shift, cx);
                }
                cx.stop_propagation();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.begin_stop_context_menu(menu_address.clone(), index, event, cx);
                }),
            )
            .when(!fixed, |key| {
                key.on_drag(drag, move |drag, _, _, cx| {
                    cx.stop_propagation();
                    let source = drag.clone();
                    drag_timeline.update(cx, |this, cx| {
                        this.begin_stop_drag(source, cx);
                    });
                    cx.new(|_| drag.clone())
                })
            });
        keys.push(key);
    }
    let start_x = state
        .viewport
        .x_at_seconds(state.frame_rate.frame_to_seconds(item.start()))
        .max(0.);
    let end_x = state
        .viewport
        .x_at_seconds(
            state
                .frame_rate
                .frame_to_seconds(item.end_exclusive(state.frame_rate)),
        )
        .min(state.viewport_width);
    let label_address = address.clone();
    div()
        .id(SharedString::from(format!("dope-row-{address:?}")))
        .relative()
        .h(px(LAYER_HEIGHT))
        .w_full()
        .flex()
        .bg(state.layer_background(layer.get() as usize))
        .child(
            div()
                .id("dope-label")
                .w(px(LAYER_HEADER_WIDTH))
                .h_full()
                .flex_none()
                .flex()
                .items_center()
                .pl_10()
                .pr_2()
                .border_r_1()
                .border_color(state.colors.border)
                .text_sm()
                .when(focused_segment.is_some(), |label| {
                    label.text_color(state.colors.accent_foreground)
                })
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .cursor_pointer()
                .child(label.clone())
                .on_scroll_wheel(cx.listener(Timeline::on_label_scroll))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_animation_curve(label_address.clone(), None, cx);
                    cx.stop_propagation();
                })),
        )
        .child(
            timeline_track(
                SharedString::from(format!("dope-track-{address:?}")),
                layer.get() as usize,
                state,
                cx,
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(start_x))
                    .w(px((end_x - start_x).max(0.)))
                    .bg(state.colors.primary.opacity(0.04)),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .child(crate::ui::panels::timeline::grid_canvas(
                        state.grid.clone(),
                        state.colors.border.opacity(0.4),
                        state.colors.border.opacity(0.14),
                    )),
            )
            .children(segments)
            .child(stop_insertion_guide(&address, state, cx))
            .children(ghosts)
            .children(keys)
            .operation_hints(navigation_hints()),
        )
        .when(last_in_layer, |row| {
            row.child(
                div()
                    .absolute()
                    .bottom_0()
                    .w_full()
                    .h(px(1.))
                    .bg(state.colors.table_row_border),
            )
        })
        .into_any_element()
}

fn stop_insertion_guide(
    address: &PropertyAddress,
    state: &LayerRenderState,
    cx: &Context<Timeline>,
) -> gpui::AnyElement {
    let timeline = cx.entity_id();
    let viewport = state.viewport;
    let frame_rate = state.frame_rate;
    let color = state.colors.muted_foreground.opacity(0.65);
    let follow_pointer = !state.context_menu_open;
    let menu_frame = state
        .stop_insertion
        .as_ref()
        .filter(|(target, _)| target == address)
        .map(|(_, frame)| *frame);
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, cx| {
            let hovered = hitbox.is_hovered(window);
            let frame = menu_frame.or_else(|| {
                (follow_pointer
                    && hovered
                    && window.pressed_mouse_button().is_none()
                    && !cx.has_active_drag())
                .then(|| {
                    viewport.frame_at_x(
                        f32::from(window.mouse_position().x - bounds.origin.x),
                        0.,
                        f32::from(bounds.size.width),
                        frame_rate,
                    )
                })
            });
            if let Some(frame) = frame {
                let x = viewport.x_at_seconds(frame_rate.frame_to_seconds(frame));
                let bottom = f32::from(bounds.size.height) - 5.;
                for y in (5..bottom.max(5.) as usize).step_by(6) {
                    window.paint_quad(fill(
                        Bounds::new(
                            bounds.origin + point(px(x - 0.5), px(y as f32)),
                            size(px(1.), px(3_f32.min(bottom - y as f32))),
                        ),
                        color,
                    ));
                }
            }
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if follow_pointer
                    && phase == DispatchPhase::Capture
                    && (hovered || bounds.contains(&event.position))
                {
                    cx.notify(timeline);
                }
            });
        },
    )
    .absolute()
    .inset_0()
    .into_any_element()
}
