use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{ClickEvent, Context, EntityId, MouseButton, div, prelude::*, px};

use zerium_core::timeline::{
    AnimationEditTarget, Frame, ResizeEdge, ResizeMode, TimelineItem, TimelineTime,
};

use crate::ui::TimelineEditorEntityExt as _;

use crate::ui::timeline::{
    Timeline,
    interaction::{
        item::{MoveTimelineItem, ResizeTimelineItem},
        keyframe::MoveAnimationStop,
    },
    render::{LayerRenderState, TimelineItemRenderData, navigation_hints},
};

use crate::ui::operation_hints::{Hint, OperationHintExt as _};

fn resize_hints() -> Vec<Hint> {
    vec![
        Hint::gesture("hints.drag", "hints.stretch", gpui::Modifiers::shift()),
        Hint::gesture("hints.drag", "hints.no_snap", gpui::Modifiers::alt()),
    ]
}

impl Timeline {
    pub(in crate::ui::timeline) fn timeline_item(
        render_item: TimelineItemRenderData,
        state: &LayerRenderState,
        timeline_id: EntityId,
        clip_top: f32,
        clip_height: f32,
        visible_range: (f64, f64),
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let TimelineItemRenderData {
            item,
            label: item_label,
        } = render_item;
        let item_start = state.frame_rate.frame_to_seconds(item.start);
        let item_end = state.frame_rate.frame_to_seconds(item.end_exclusive());
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
            .frame_to_seconds(Frame::new(item.duration.get()))
            * state.viewport.pixels_per_second()) as f32;
        let stop_left = move |time: TimelineTime, diameter: f32| {
            let x = state.viewport.x_at_seconds(time.seconds(state.frame_rate)) - item_left;
            px((x - diameter / 2.).clamp(0., (item_width - diameter).max(0.)))
        };
        let is_selected = state.selected_item_ids.contains(&item_id);
        let snap_frame = state.editor.read(cx).playhead();
        let visible_start = TimelineTime::from_frames(
            (visible_start * state.frame_rate.frames_per_second()).max(item.start.get() as f64),
        );
        let visible_end = TimelineTime::from_frames(
            (visible_end * state.frame_rate.frames_per_second())
                .min(item.end_exclusive().get().saturating_sub(1) as f64),
        );
        let pixels_per_frame =
            state.viewport.pixels_per_second() / state.frame_rate.frames_per_second();
        let mut animation_stops = Vec::new();
        for (_, track) in item.animations.tracks().chain(
            item.effects
                .iter()
                .flat_map(|effect| effect.animations.tracks()),
        ) {
            let clock = item.animation_clock(track);
            let stride = (1. / (clock.span_frames() * pixels_per_frame))
                .ceil()
                .max(1.) as usize;
            for stop in track.stops() {
                let canonical = clock.key_time_at(stop.frame());
                if canonical >= visible_start && canonical <= visible_end {
                    animation_stops.push((canonical, false));
                }
                animation_stops.extend(
                    clock
                        .occurrences(stop.frame(), visible_start, visible_end)
                        .step_by(stride)
                        .map(|time| (time, true)),
                );
            }
        }
        animation_stops.sort_by(|left, right| {
            left.0
                .frames()
                .total_cmp(&right.0.frames())
                .then(left.1.cmp(&right.1))
        });
        animation_stops.dedup_by(|left, right| {
            let same = (left.0.frames() - right.0.frames()).abs() * pixels_per_frame < 1.;
            if same {
                right.1 &= left.1;
            }
            same
        });
        let focused_target = state
            .animation_address
            .as_ref()
            .filter(|target| target.item_id == item_id)
            .cloned();
        let has_focused_target = focused_target.is_some();
        let focused_animation_stops = focused_target
            .as_ref()
            .and_then(|target| {
                let track = item.animation_track(
                    target.effect_id,
                    &target.property_id,
                    target.element_id,
                    target.scalar_index,
                )?;
                Some((target.clone(), track))
            })
            .map(|(target, track)| {
                let stop_count = track.stops().len();
                track
                    .stops()
                    .iter()
                    .enumerate()
                    .map(|(stop, animation_stop)| {
                        let progress = animation_stop.frame();
                        let time = item.animation_clock(track).key_time_at(progress);
                        let frame = time.nearest_frame();
                        let follow_focus = (snap_frame == frame)
                            .then_some(state.focused_animation_segment)
                            .flatten()
                            .filter(|segment| {
                                *segment == stop || segment.saturating_add(1) == stop
                            });
                        (
                            target.clone(),
                            stop,
                            time,
                            frame,
                            follow_focus,
                            stop > 0 && stop + 1 < stop_count,
                        )
                    })
                    .filter(|(_, _, time, _, _, _)| *time >= visible_start && *time <= visible_end)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

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
                                .pl_1()
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
                                .operation_hints(
                                    navigation_hints().into_iter().chain(resize_hints()),
                                ),
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
                                .operation_hints(
                                    navigation_hints().into_iter().chain(resize_hints()),
                                ),
                        ),
                )
                .children(animation_stops.into_iter().map(|(time, repeated)| {
                    div()
                        .absolute()
                        .top(px(2.))
                        .left(stop_left(time, 5.))
                        .size(px(5.))
                        .rounded_full()
                        .border_1()
                        .border_color(state.colors.background.opacity(0.7))
                        .bg(state.colors.primary.opacity(if repeated {
                            0.2
                        } else if has_focused_target {
                            0.35
                        } else if is_selected {
                            0.95
                        } else {
                            0.65
                        }))
                }))
                .children(focused_animation_stops.into_iter().map({
                    let timeline_editor = state.editor.clone();
                    let transport = state.transport.clone();
                    let colors = state.colors;
                    move |(target, stop, time, frame, follow_focus, movable)| {
                        let drag = MoveAnimationStop {
                            timeline_id,
                            address: target,
                            edit: Rc::new(RefCell::new(None)),
                            snap_playhead: Cell::new(snap_frame),
                            follow_focus,
                        };
                        let start_editor = timeline_editor.clone();
                        let seek_transport = transport.clone();
                        div()
                            .id(("focused-animation-stop", stop))
                            .absolute()
                            .top(px(1.))
                            .left(stop_left(time, 8.))
                            .size(px(8.))
                            .rounded_full()
                            .border_2()
                            .border_color(colors.background)
                            .bg(colors.primary)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                seek_transport.update(cx, |transport, cx| {
                                    transport.set_playhead(frame, cx);
                                });
                            })
                            .when(movable, |marker| {
                                marker
                                    .cursor_col_resize()
                                    .hover(move |style| style.bg(colors.primary_hover))
                                    .active(move |style| style.bg(colors.primary_active))
                                    .on_drag(drag, move |drag, _, window, cx| {
                                        cx.stop_propagation();
                                        // A press redraws the marker; initialize the retained
                                        // payload only when GPUI starts the drag.
                                        drag.snap_playhead.set(start_editor.read(cx).playhead());
                                        *drag.edit.borrow_mut() =
                                            start_editor.read(cx).begin_animation_edit(
                                                &drag.address,
                                                AnimationEditTarget::Stop(stop),
                                                !window.modifiers().alt,
                                            );
                                        cx.new(|_| drag.clone())
                                    })
                            })
                            .operation_hints(navigation_hints().into_iter().chain(movable.then(
                                || {
                                    Hint::gesture(
                                        "hints.drag",
                                        "hints.independent_stop",
                                        gpui::Modifiers::alt(),
                                    )
                                },
                            )))
                    }
                }))
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
}
