use crate::ui::{
    components::{pane::PANE_HEADER_HEIGHT, time_grid},
    panels::timeline::{LAYER_HEADER_WIDTH, Timeline, viewport::TimelineViewport},
    workspace::operation_hints::{Hint, OperationHintExt as _},
};
use rust_i18n::t;
use std::rc::Rc;

use ::ui::{
    Icon, IconName, Sizable as _, ThemeColor,
    button::{Button, ButtonVariants as _},
    menu::{PopupMenuItem, popup_menu::PopupMenuExt as _},
};
use gpui::{
    Bounds, Context, Div, Hsla, MouseButton, PathBuilder, Pixels, Window, canvas, div, point,
    prelude::*, px,
};

use zerium_core::timeline::{BeatGuide, FrameRate};

#[derive(Clone)]
pub(in crate::ui::panels::timeline) struct TimelineGrid {
    pub(in crate::ui::panels::timeline) ruler_ticks: Rc<Vec<(f64, f32)>>,
    pub(in crate::ui::panels::timeline) beat_ticks: Rc<Vec<f32>>,
    pub(in crate::ui::panels::timeline) frame_ticks: Rc<Vec<f32>>,
}

const SCENE_SWITCHER_LABEL_WIDTH: usize = 12;

impl Timeline {
    pub(in crate::ui::panels::timeline) fn ruler(
        &self,
        colors: ThemeColor,
        viewport_width: f32,
        ruler_ticks: Rc<Vec<(f64, f32)>>,
        cx: &mut Context<Self>,
    ) -> Div {
        let viewport = self.viewport;
        let (playhead_seconds, active_scene_name, scenes) = {
            let editor = self.editor.read(cx);
            (
                editor.playhead_seconds(),
                editor.active_scene().name.clone(),
                editor
                    .scenes()
                    .map(|scene| (scene.id, scene.name.clone()))
                    .collect::<Vec<_>>(),
            )
        };
        let playhead_x = viewport.x_at_seconds(playhead_seconds);
        let ruler_ticks = ruler_ticks.as_ref().clone();
        let mut label_width = 0;
        let active_scene_label = active_scene_name
            .char_indices()
            .find_map(|(index, character)| {
                label_width += if character.is_ascii() { 1 } else { 2 };
                (label_width > SCENE_SWITCHER_LABEL_WIDTH - 2)
                    .then(|| format!("{}…", &active_scene_name[..index]))
            })
            .unwrap_or_else(|| active_scene_name.clone());
        let playback_icon = if self.transport.read(cx).is_playing() {
            IconName::Pause
        } else {
            IconName::Play
        };
        let playback_tooltip = if self.transport.read(cx).is_playing() {
            t!("timeline.stop").to_string()
        } else {
            t!("timeline.play").to_string()
        };

        div()
            .h(px(PANE_HEADER_HEIGHT))
            .flex_none()
            .flex()
            .border_b_1()
            .border_color(colors.border)
            .bg(colors.background)
            .child(
                div()
                    .w(px(LAYER_HEADER_WIDTH))
                    .h_full()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .px_1()
                    .border_r_1()
                    .border_color(colors.border)
                    .bg(colors.title_bar)
                    .on_scroll_wheel(cx.listener(Self::on_label_scroll))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                transport_button(
                                    "timeline-previous",
                                    Icon::new(IconName::ChevronLeft),
                                    t!("timeline.previous_frame").to_string(),
                                )
                                .on_click(cx.listener(Self::previous_frame)),
                            )
                            .child(
                                transport_button(
                                    "timeline-play",
                                    Icon::new(playback_icon),
                                    playback_tooltip,
                                )
                                .on_click(cx.listener(Self::toggle_playback_button)),
                            )
                            .child(
                                transport_button(
                                    "timeline-next",
                                    Icon::new(IconName::ChevronRight),
                                    t!("timeline.next_frame").to_string(),
                                )
                                .on_click(cx.listener(Self::next_frame)),
                            ),
                    )
                    .child(
                        Button::new("timeline-scene-switcher")
                            .small()
                            .compact()
                            .ghost()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .label(active_scene_label)
                            .dropdown_caret(true)
                            .tooltip(active_scene_name)
                            .popup_menu({
                                let timeline = cx.entity();
                                move |menu, _, _| {
                                    scenes.iter().cloned().fold(menu, |menu, (id, name)| {
                                        let timeline = timeline.clone();
                                        menu.item(PopupMenuItem::new(name).on_click(
                                            move |_, _, cx| {
                                                timeline.update(cx, |timeline, cx| {
                                                    timeline.switch_scene(id, cx);
                                                });
                                            },
                                        ))
                                    })
                                }
                            }),
                    ),
            )
            .child(
                div()
                    .id("timeline-ruler-track")
                    .relative()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::begin_playhead_scrub))
                    .on_mouse_move(cx.listener(Self::update_playhead_scrub))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.finish_playhead_scrub(cx)),
                    )
                    .on_scroll_wheel(cx.listener(Self::on_track_scroll))
                    .child(
                        div()
                            .absolute()
                            .size_full()
                            .children(ruler_ticks.into_iter().map(|(seconds, tick_x)| {
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left(px(tick_x))
                                    .border_l_1()
                                    .border_color(colors.border)
                                    .flex()
                                    .items_center()
                                    .pl_1()
                                    .text_sm()
                                    .text_color(colors.muted_foreground)
                                    .child(ruler_tick_label(seconds))
                            }))
                            .when(
                                playhead_x >= -4. && playhead_x <= viewport_width + 4.,
                                |this| {
                                    this.child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .bottom_0()
                                            .left(px(playhead_x))
                                            .w(px(2.))
                                            .bg(colors.primary),
                                    )
                                },
                            )
                            .when(
                                playhead_x >= -4. && playhead_x <= viewport_width + 4.,
                                |this| {
                                    this.child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .left(px(playhead_x - 4.))
                                            .size(px(8.))
                                            .rounded_b_sm()
                                            .bg(colors.primary),
                                    )
                                },
                            ),
                    )
                    .when_some(
                        self.stretch_marker(colors, viewport_width, cx),
                        |this, marker| this.child(marker),
                    )
                    .operation_hints([
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
                    ]),
            )
    }
}

fn transport_button(id: &'static str, icon: Icon, tooltip: String) -> Button {
    Button::new(id)
        .icon(icon)
        .tooltip(tooltip)
        .small()
        .compact()
        .ghost()
}

fn ruler_tick_label(seconds: f64) -> String {
    time_grid::format_timestamp(seconds)
}

pub(in crate::ui::panels::timeline) fn timeline_grid(
    viewport: TimelineViewport,
    viewport_width: f32,
    frame_rate: FrameRate,
    beat_guide: BeatGuide,
) -> TimelineGrid {
    let (start, end) = viewport.visible_time_range(viewport_width);
    let pixels_per_second = viewport.pixels_per_second();
    let positions = |grid: time_grid::Grid| {
        grid.visible_times(start, end)
            .into_iter()
            .map(|seconds| viewport.x_at_seconds(seconds))
            .collect()
    };
    let ruler_ticks = time_grid::Grid::seconds(pixels_per_second)
        .visible_times(start, end)
        .into_iter()
        .map(|seconds| (seconds, viewport.x_at_seconds(seconds)))
        .collect();
    TimelineGrid {
        ruler_ticks: Rc::new(ruler_ticks),
        beat_ticks: Rc::new(positions(time_grid::Grid::beats(
            pixels_per_second,
            beat_guide,
            frame_rate,
        ))),
        frame_ticks: Rc::new(positions(time_grid::Grid::frames(
            pixels_per_second,
            frame_rate,
        ))),
    }
}

fn paint_vertical_lines(
    bounds: Bounds<Pixels>,
    positions: impl IntoIterator<Item = f32>,
    color: Hsla,
    window: &mut Window,
) {
    let scale = window.scale_factor();
    let pixel = px(1. / scale);
    let mut builder = PathBuilder::stroke(pixel);
    for tick_x in positions {
        if tick_x < -1. || tick_x > f32::from(bounds.size.width) + 1. {
            continue;
        }
        let x = px(((f32::from(bounds.origin.x) + tick_x) * scale).floor() / scale) + pixel * 0.5;
        builder.move_to(point(x, bounds.origin.y));
        builder.line_to(point(x, bounds.origin.y + bounds.size.height));
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

pub(in crate::ui::panels::timeline) fn grid_canvas(
    grid: TimelineGrid,
    color_major: Hsla,
    color_minor: Hsla,
) -> impl IntoElement {
    canvas(
        move |_, _, _| grid,
        move |bounds, grid, window, _| {
            paint_vertical_lines(
                bounds,
                grid.frame_ticks.iter().copied(),
                color_minor,
                window,
            );
            paint_vertical_lines(bounds, grid.beat_ticks.iter().copied(), color_major, window);
        },
    )
    .absolute()
    .size_full()
}
