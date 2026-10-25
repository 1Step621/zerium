//! Timeline dots show all keys; only the displayed scalar property's interior keys are movable.
use gpui::{Context, MouseButton, div, prelude::*, px};
use zerium_core::timeline::{AnimationEditTarget, PropertyAddress, TimelineItem, TimelineTime};

use super::{LayerRenderState, navigation_hints};
use crate::ui::{
    animation::AnimationSynchronization,
    operation_hints::{Hint, OperationHintExt as _},
    timeline::{
        Timeline,
        interaction::keyframe::{AnimationStopDrag, MoveAnimationStop},
    },
};

const KEY_MARKER_HIT_SIZE: f32 = 12.;

#[derive(Clone, Copy)]
struct KeyMarker {
    time: TimelineTime,
    repeated: bool,
    // A stop index is meaningful only for the inspector's displayed property.
    stop: Option<usize>,
}

fn collect_markers(
    item: &TimelineItem,
    focused: Option<&PropertyAddress>,
    visible_start: TimelineTime,
    visible_end: TimelineTime,
    pixels_per_frame: f64,
) -> Vec<KeyMarker> {
    let mut markers = Vec::new();
    for (address, track) in item.animation_tracks() {
        let clock = item.animation_clock(track);
        let stride = (1. / (clock.span_frames() * pixels_per_frame))
            .ceil()
            .max(1.) as usize;
        for (stop, key) in track.stops().iter().enumerate() {
            let time = clock.key_time_at(key.frame());
            if time >= visible_start && time <= visible_end {
                markers.push(KeyMarker {
                    time,
                    repeated: false,
                    stop: (focused == Some(&address)).then_some(stop),
                });
            }
            markers.extend(
                clock
                    .occurrences(key.frame(), visible_start, visible_end)
                    .step_by(stride)
                    .map(|time| KeyMarker {
                        time,
                        repeated: true,
                        stop: None,
                    }),
            );
        }
    }
    markers.sort_by(|left, right| left.time.frames().total_cmp(&right.time.frames()));
    markers.dedup_by(|later, earlier| {
        let same_pixel =
            (later.time.frames() - earlier.time.frames()).abs() * pixels_per_frame < 1.;
        if same_pixel
            && ((earlier.stop.is_none() && later.stop.is_some())
                || (earlier.repeated && !later.repeated))
        {
            *earlier = *later;
        }
        same_pixel
    });
    // Keep the displayed property's hit regions above the other dots.
    markers.sort_by_key(|marker| marker.stop.is_some());
    markers
}

pub(super) fn key_markers(
    item: &TimelineItem,
    state: &LayerRenderState,
    item_left: f32,
    item_width: f32,
    visible_range: (f64, f64),
    cx: &Context<Timeline>,
) -> Vec<gpui::AnyElement> {
    let focused = state
        .animation_address
        .as_ref()
        .filter(|address| address.item_id == item.id);
    let visible_start = TimelineTime::from_frames(
        (visible_range.0 * state.frame_rate.frames_per_second()).max(item.start().get() as f64),
    );
    let visible_end = TimelineTime::from_frames(
        (visible_range.1 * state.frame_rate.frames_per_second())
            .min(item.end_exclusive(state.frame_rate).get().saturating_sub(1) as f64),
    );
    let pixels_per_frame =
        state.viewport.pixels_per_second() / state.frame_rate.frames_per_second();
    let selected = state.selected_item_ids.contains(&item.id);
    let editor = state.editor.read(cx);
    let timeline = cx.entity();
    collect_markers(item, focused, visible_start, visible_end, pixels_per_frame)
        .into_iter()
        .map(|marker| {
            let x = state
                .viewport
                .x_at_seconds(marker.time.seconds(state.frame_rate))
                - item_left;
            let source = focused
                .zip(marker.stop)
                .map(|(address, stop)| MoveAnimationStop {
                    timeline_id: cx.entity_id(),
                    address: address.clone(),
                    stop,
                });
            let synchronization = source.as_ref().and_then(|source| {
                match state
                    .animation_drag_synchronization
                    .as_ref()
                    .filter(|(active, _)| active == source)
                {
                    Some((_, synchronization)) => Some(synchronization.clone()),
                    None => editor
                        .begin_animation_edit(
                            &source.address,
                            AnimationEditTarget::Stop(source.stop),
                            state.synchronize_animation_keys,
                        )
                        .map(|edit| AnimationSynchronization::from_edit(&state.editor, &edit)),
                }
            });
            let linked = synchronization
                .as_ref()
                .is_some_and(|scope| scope.is_linked());
            let drag = source.filter(|_| synchronization.is_some());
            let color = state.colors.primary;
            let is_focused = marker.stop.is_some();
            let opacity = if is_focused {
                1.
            } else if selected {
                0.65
            } else {
                0.45
            };
            let dot = div()
                .absolute()
                .top_0()
                .left(px((x - KEY_MARKER_HIT_SIZE / 2.)
                    .clamp(0., (item_width - KEY_MARKER_HIT_SIZE).max(0.))))
                .size(px(KEY_MARKER_HIT_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .child(
                    div()
                        .size(px(if is_focused { 7. } else { 5. }))
                        .rounded_full()
                        .border_1()
                        .border_color(if is_focused {
                            color
                        } else {
                            state.colors.background.opacity(0.7)
                        })
                        .bg(if is_focused && !linked {
                            state.colors.background
                        } else {
                            color.opacity(opacity)
                        }),
                );
            if marker.repeated {
                return dot.into_any_element();
            }
            let frame = marker.time.nearest_frame();
            let id: gpui::ElementId = marker.stop.map_or_else(
                || ("animation-time", frame.get()).into(),
                |stop| ("animation-stop", stop).into(),
            );
            let movable = drag.is_some();
            let press_source = drag.clone();
            let press_timeline = timeline.clone();
            let transport = state.transport.clone();
            dot.id(id)
                .when_some(synchronization, |dot, scope| {
                    dot.tooltip(move |window, cx| scope.tooltip(window, cx))
                })
                .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                    cx.stop_propagation();
                    press_timeline.update(cx, |timeline, cx| {
                        timeline.animation_drag = press_source.clone().and_then(|source| {
                            AnimationStopDrag::new(
                                source,
                                timeline.editor.read(cx),
                                !event.modifiers.alt,
                            )
                        });
                    });
                })
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    transport.update(cx, |transport, cx| transport.set_playhead(frame, cx));
                })
                .when_some(drag, |dot, source| {
                    dot.cursor_col_resize()
                        .hover(move |style| style.bg(color.opacity(0.12)))
                        .active(move |style| style.bg(color.opacity(0.20)))
                        .on_drag(source, |source, _, _, cx| {
                            cx.stop_propagation();
                            cx.new(|_| source.clone())
                        })
                })
                .operation_hints(navigation_hints().into_iter().chain(movable.then(|| {
                    Hint::gesture(
                        "hints.drag",
                        "hints.independent_stop",
                        gpui::Modifiers::alt(),
                    )
                })))
                .into_any_element()
        })
        .collect()
}
