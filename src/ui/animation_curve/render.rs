use super::*;

const EASING_FAMILIES: &[(&str, EasingFamily)] = &[
    ("Bezier", EasingFamily::Bezier),
    ("Sine", EasingFamily::Sine),
    ("Quad", EasingFamily::Quad),
    ("Bounce", EasingFamily::Bounce),
    ("Elastic", EasingFamily::Elastic),
];
const EASING_DIRECTIONS: &[(&str, EasingDirection)] = &[
    ("In", EasingDirection::In),
    ("Out", EasingDirection::Out),
    ("In / Out", EasingDirection::InOut),
];

fn easing_options() -> Vec<(String, SegmentInterpolation)> {
    EASING_FAMILIES
        .iter()
        .flat_map(|(family_label, family)| {
            EASING_DIRECTIONS
                .iter()
                .map(move |(direction_label, direction)| {
                    (
                        format!("{family_label} {direction_label}"),
                        SegmentInterpolation::Ease {
                            family: *family,
                            direction: *direction,
                        },
                    )
                })
        })
        .collect()
}

fn interpolation_label(interpolation: SegmentInterpolation) -> String {
    match interpolation {
        SegmentInterpolation::Linear => "直線".to_owned(),
        SegmentInterpolation::Hold => "ホールド".to_owned(),
        SegmentInterpolation::Ease { family, direction } => {
            let family = EASING_FAMILIES
                .iter()
                .find_map(|(label, candidate)| (*candidate == family).then_some(*label))
                .expect("every easing family has a label");
            let direction = EASING_DIRECTIONS
                .iter()
                .find_map(|(label, candidate)| (*candidate == direction).then_some(*label))
                .expect("every easing direction has a label");
            format!("{family} {direction}")
        }
        SegmentInterpolation::Custom { .. } => "カスタム".to_owned(),
    }
}

impl Render for AnimationCurveEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let Some(selected) = self.selected_curve(cx) else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(colors.background)
                .text_sm()
                .text_color(colors.muted_foreground)
                .child("アニメーションするプロパティを選択")
                .into_any_element();
        };
        let mut title = format!(
            "{} · セグメント {} / {}",
            selected.presentation.label,
            selected.source_segment + 1,
            selected.source_stop_count.saturating_sub(1),
        );
        if let GraphInteraction::StopDrag { frame, .. } = self.graph_interaction {
            title.push_str(&format!(
                " · {}f / {}秒",
                frame.get(),
                Self::format_number(selected.frame_rate.frame_to_seconds(frame))
            ));
        }
        let curve = selected.animation.curve.clone();
        let playhead_progress = selected.playhead_progress;
        let curve_editor = cx.entity();
        let graph_editor = curve_editor.clone();
        let stops = curve.stops().to_vec();
        let stop_count = stops.len();
        let source_segment = selected.source_segment;
        let source_stop_count = selected.source_stop_count;
        let overview_playhead_progress = selected.source_playhead_progress;
        let overview_segments = selected
            .source_stop_positions
            .windows(2)
            .map(|stops| (stops[0], stops[1]))
            .collect::<Vec<_>>();
        let overview_stops = selected.source_stop_positions.clone();
        let custom_segments = (0..stop_count.saturating_sub(1))
            .map(|segment| curve.is_custom(segment))
            .collect::<Vec<_>>();
        let active_handle = match self.graph_interaction {
            GraphInteraction::HandleDrag { point } => Some(point),
            _ => None,
        };
        let selected_segment = self
            .selected_segment
            .filter(|segment| segment.checked_add(1).is_some_and(|end| end < stop_count));
        let playhead_position = [playhead_progress, curve.evaluate(playhead_progress)];
        let axis_suffix = selected.axis_suffix.clone();
        let mut curve_grid = self.time_grid(
            selected.start_seconds,
            selected.duration_seconds,
            selected.frame_rate,
        );
        curve_grid.values = Self::value_grid(&selected.animation);
        let value_ticks = curve_grid.values.clone();
        let time_ticks = curve_grid.major.clone();
        let begin_scrub_editor = curve_editor.clone();
        let update_scrub_editor = curve_editor.clone();
        let finish_scrub_editor = curve_editor.clone();
        let stop_nodes = stops
            .iter()
            .enumerate()
            .filter_map(|(index, stop)| {
                let (screen, margin_left, margin_bottom) = Self::point_layout(*stop, 7.);
                Self::screen_position_is_visible(screen).then(|| {
                    div()
                        .id(("animation-stop", index))
                        .absolute()
                        .left(relative(screen[0]))
                        .bottom(relative(screen[1]))
                        .ml(px(margin_left))
                        .mb(px(margin_bottom))
                        .size(px(14.))
                        .rounded_full()
                        .border_2()
                        .border_color(colors.background)
                        .bg(colors.primary)
                })
            })
            .collect::<Vec<_>>();
        let graph = div()
            .id("animation-curve-graph")
            .relative()
            .w_full()
            .flex_1()
            .min_h_0()
            .bg(colors.background)
            .overflow_hidden()
            // Single gesture: mousedown seeks at once on empty space (scrub
            // session only while playing, like the timeline ruler); other
            // presses stay click candidates. A drag past the threshold
            // becomes a scrub, and a quiet release becomes a click that
            // selects the curve or seeks. Mousedown never selects.
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                begin_scrub_editor.update(cx, |editor, cx| {
                    editor.graph_press_started(event.position, cx);
                });
                cx.stop_propagation();
            })
            .on_mouse_move(move |event, _, cx| {
                update_scrub_editor.update(cx, |editor, cx| {
                    editor.graph_press_moved(event, cx);
                });
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                finish_scrub_editor.update(cx, |editor, cx| {
                    editor.end_graph_press(cx);
                });
            })
            .on_click({
                let curve_editor = curve_editor.clone();
                move |_, window, cx| {
                    let position = window.mouse_position();
                    curve_editor.update(cx, |editor, cx| {
                        editor.resolve_graph_click(position, window, cx)
                    });
                    cx.stop_propagation();
                }
            })
            .on_drag_move(move |event: &gpui::DragMoveEvent<CurvePointDrag>, _, cx| {
                let drag = event.drag(cx).clone();
                let position = event.event.position;
                graph_editor.update(cx, |editor, cx| editor.move_point(drag.point, position, cx));
            })
            .child(Self::graph_canvas(
                curve.clone(),
                playhead_progress,
                curve_grid,
                CurvePaintColors {
                    grid_major: colors.border.opacity(0.45),
                    grid_minor: colors.border.opacity(0.20),
                    handle: colors.muted_foreground.opacity(0.65),
                    playhead: colors.warning.opacity(0.75),
                    curve: colors.primary,
                },
                curve_editor.clone(),
            ))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(px(Self::GRAPH_INSET_LEFT - 2.))
                    .bg(colors.background),
            )
            .children(value_ticks.into_iter().map(move |(value, screen_y)| {
                let label = format!("{}{}", Self::format_number(value), axis_suffix);
                div()
                    .absolute()
                    .left(px(4.))
                    .bottom(relative(screen_y))
                    .mb(px(Self::GRAPH_INSET_BOTTOM * (1. - screen_y)
                        - Self::GRAPH_INSET_TOP * screen_y
                        - 7.))
                    .w(px(Self::GRAPH_INSET_LEFT - 10.))
                    .text_right()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(label)
            }))
            .children(time_ticks.into_iter().map(move |(seconds, x)| {
                let screen_x = x;
                let label = time_grid::format_timestamp(seconds);
                div()
                    .absolute()
                    .left(relative(screen_x))
                    .bottom(px(1.))
                    .ml(px(Self::GRAPH_INSET_LEFT * (1. - screen_x)
                        - Self::GRAPH_INSET_RIGHT * screen_x
                        - 32.))
                    .w(px(64.))
                    .text_center()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(label)
            }))
            .children(stop_nodes)
            .when(
                Self::screen_position_is_visible(playhead_position),
                |this| {
                    this.child(
                        div()
                            .absolute()
                            .left(relative(playhead_position[0]))
                            .bottom(relative(playhead_position[1]))
                            .ml(px(Self::GRAPH_INSET_LEFT * (1. - playhead_position[0])
                                - Self::GRAPH_INSET_RIGHT * playhead_position[0]
                                - 4.))
                            .mb(px(Self::GRAPH_INSET_BOTTOM * (1. - playhead_position[1])
                                - Self::GRAPH_INSET_TOP * playhead_position[1]
                                - 4.))
                            .size(px(8.))
                            .rounded_full()
                            .border_1()
                            .border_color(colors.background)
                            .bg(colors.warning),
                    )
                },
            )
            .children(stops.iter().enumerate().flat_map({
                let curve_editor = curve_editor.clone();
                let curve = curve.clone();
                let custom_segments = custom_segments.clone();
                move |(index, stop)| {
                    let mut handles = Vec::with_capacity(2);
                    if index > 0 && custom_segments[index - 1] {
                        let point = CurvePoint::HandleIn(index);
                        let (screen, margin_left, margin_bottom) = Self::point_layout(
                            curve
                                .handle_position(index, BezierHandle::In)
                                .unwrap_or(*stop),
                            5.,
                        );
                        if Self::screen_position_is_visible(screen) {
                            let drag = CurvePointDrag { point };
                            let select_editor = curve_editor.clone();
                            handles.push(
                                div()
                                    .id(("animation-handle-in", index))
                                    .absolute()
                                    .left(relative(screen[0]))
                                    .bottom(relative(screen[1]))
                                    .ml(px(margin_left))
                                    .mb(px(margin_bottom))
                                    .size(px(10.))
                                    .rounded_full()
                                    .border_2()
                                    .border_color(if active_handle == Some(point) {
                                        colors.primary
                                    } else {
                                        colors.muted_foreground
                                    })
                                    .bg(colors.background)
                                    .cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                        cx.stop_propagation();
                                        select_editor.update(cx, |editor, cx| {
                                            editor.begin_handle_drag(point, window, cx);
                                        });
                                    })
                                    .on_click(|_, _, cx| cx.stop_propagation())
                                    .on_drag(drag, |drag, _, _, cx| {
                                        cx.stop_propagation();
                                        cx.new(|_| drag.clone())
                                    })
                                    .into_any_element(),
                            );
                        }
                    }
                    if index + 1 < stop_count && custom_segments[index] {
                        let point = CurvePoint::HandleOut(index);
                        let (screen, margin_left, margin_bottom) = Self::point_layout(
                            curve
                                .handle_position(index, BezierHandle::Out)
                                .unwrap_or(*stop),
                            5.,
                        );
                        if Self::screen_position_is_visible(screen) {
                            let drag = CurvePointDrag { point };
                            let select_editor = curve_editor.clone();
                            handles.push(
                                div()
                                    .id(("animation-handle-out", index))
                                    .absolute()
                                    .left(relative(screen[0]))
                                    .bottom(relative(screen[1]))
                                    .ml(px(margin_left))
                                    .mb(px(margin_bottom))
                                    .size(px(10.))
                                    .rounded_full()
                                    .border_2()
                                    .border_color(if active_handle == Some(point) {
                                        colors.primary
                                    } else {
                                        colors.muted_foreground
                                    })
                                    .bg(colors.background)
                                    .cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                        cx.stop_propagation();
                                        select_editor.update(cx, |editor, cx| {
                                            editor.begin_handle_drag(point, window, cx);
                                        });
                                    })
                                    .on_click(|_, _, cx| cx.stop_propagation())
                                    .on_drag(drag, |drag, _, _, cx| {
                                        cx.stop_propagation();
                                        cx.new(|_| drag.clone())
                                    })
                                    .into_any_element(),
                            );
                        }
                    }
                    handles
                }
            }));

        let segment_editor = selected_segment.and_then(|segment| {
            let interpolation = curve.interpolation(segment)?;
            // Clamp the menu position because some easing families overshoot
            // the value range.
            let screen = Self::segment_panel_position(&curve, segment)?;
            let screen = [screen[0].clamp(0., 1.), screen[1].clamp(0., 1.)];
            let bounds = self.graph_bounds?;
            let origin = Self::segment_editor_origin(
                [f32::from(bounds.size.width), f32::from(bounds.size.height)],
                screen,
            );
            let interpolation_editor = curve_editor.clone();
            Some(
                div()
                    .absolute()
                    .left(px(origin[0]))
                    .top(px(origin[1]))
                    .w(px(Self::SEGMENT_EDITOR_WIDTH))
                    .h(px(Self::SEGMENT_EDITOR_HEIGHT))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.background)
                    .shadow_md()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("セグメント {}", selected.source_segment + 1)),
                    )
                    .child(
                        Button::new("selected-segment-interpolation")
                            .small()
                            .compact()
                            .outline()
                            .w_full()
                            .label(interpolation_label(interpolation))
                            .dropdown_caret(true)
                            .dropdown_menu_with_anchor(Corner::TopLeft, move |menu, window, _| {
                                let linear_editor = interpolation_editor.clone();
                                let hold_editor = interpolation_editor.clone();
                                let custom_editor = interpolation_editor.clone();
                                let max_height =
                                    (window.viewport_size().height - px(16.)).min(px(320.));
                                let menu = menu
                                    .max_h(max_height)
                                    .scrollable()
                                    .item(
                                        PopupMenuItem::new("直線")
                                            .checked(interpolation == SegmentInterpolation::Linear)
                                            .on_click(move |_, _, cx| {
                                                linear_editor.update(cx, |editor, cx| {
                                                    editor.set_interpolation(
                                                        SegmentInterpolation::Linear,
                                                        cx,
                                                    );
                                                });
                                            }),
                                    )
                                    .item(
                                        PopupMenuItem::new("ホールド")
                                            .checked(interpolation == SegmentInterpolation::Hold)
                                            .on_click(move |_, _, cx| {
                                                hold_editor.update(cx, |editor, cx| {
                                                    editor.set_interpolation(
                                                        SegmentInterpolation::Hold,
                                                        cx,
                                                    );
                                                });
                                            }),
                                    )
                                    .item(
                                        PopupMenuItem::new("カスタム")
                                            .checked(interpolation.is_custom())
                                            .on_click(move |_, _, cx| {
                                                custom_editor.update(cx, |editor, cx| {
                                                    editor.set_interpolation(
                                                        SegmentInterpolation::custom_default(),
                                                        cx,
                                                    );
                                                });
                                            }),
                                    )
                                    .separator();
                                easing_options()
                                    .into_iter()
                                    .fold(menu, |menu, (label, option)| {
                                        let editor = interpolation_editor.clone();
                                        menu.item(
                                            PopupMenuItem::new(label)
                                                .checked(interpolation == option)
                                                .on_click(move |_, _, cx| {
                                                    editor.update(cx, |editor, cx| {
                                                        editor.set_interpolation(option, cx);
                                                    });
                                                }),
                                        )
                                    })
                            }),
                    )
                    .into_any_element(),
            )
        });
        let context_menu_editor = curve_editor.clone();
        let graph = graph
            .children(segment_editor)
            .context_menu(move |menu, window, cx| {
                let position = window.mouse_position();
                let Some(index) = context_menu_editor
                    .read(cx)
                    .graph_stop_at_position(position, cx)
                else {
                    return menu;
                };
                let source_stop = source_segment + index;
                if source_stop == 0 || source_stop + 1 == source_stop_count {
                    return menu.item(PopupMenuItem::Label("端のstopは削除できません".into()));
                }
                let remove_editor = context_menu_editor.clone();
                menu.item(PopupMenuItem::new("stopを削除").on_click(move |_, _, cx| {
                    remove_editor.update(cx, |editor, cx| {
                        editor.remove_source_stop(source_stop, cx);
                    });
                }))
            });
        let overview_drag_editor = curve_editor.clone();
        let overview_context_menu_editor = curve_editor.clone();
        let dragging_stop = match self.graph_interaction {
            GraphInteraction::StopDrag { stop, .. } => Some(stop),
            _ => None,
        };
        let segment_overview = div()
            .w_full()
            .h(px(8.))
            .flex_none()
            .pl(px(Self::GRAPH_INSET_LEFT))
            .pr(px(Self::GRAPH_INSET_RIGHT))
            .child(
                div()
                    .id("animation-segment-overview-bar")
                    .relative()
                    .size_full()
                    .overflow_hidden()
                    .rounded_sm()
                    .bg(colors.border.opacity(0.45))
                    .on_drag_move(
                        move |event: &gpui::DragMoveEvent<StopPositionDrag>, _, cx| {
                            let drag = event.drag(cx).clone();
                            overview_drag_editor.update(cx, |editor, cx| {
                                editor.move_stop_from_overview(
                                    &drag,
                                    f32::from(event.event.position.x),
                                    cx,
                                );
                            });
                        },
                    )
                    .children({
                        let overview_editor = curve_editor.clone();
                        overview_segments.into_iter().enumerate().map(
                            move |(segment, (start, end))| {
                                let start = start.clamp(0., 1.);
                                let width = (end - start).clamp(0., 1.);
                                let segment_editor = overview_editor.clone();
                                let fill = if segment == source_segment {
                                    colors.warning
                                } else {
                                    colors.secondary
                                };
                                div()
                                    .id(("animation-segment-overview", segment))
                                    .absolute()
                                    .left(relative(start))
                                    .top_0()
                                    .bottom_0()
                                    .w(relative(width))
                                    .border_r_1()
                                    .border_color(colors.background)
                                    .bg(fill)
                                    .hover(move |style| style.bg(fill.lighten(0.12)))
                                    .active(move |style| style.bg(fill.darken(0.12)))
                                    .cursor_pointer()
                                    .on_click(move |_, window, cx| {
                                        cx.stop_propagation();
                                        segment_editor.update(cx, |editor, cx| {
                                            editor.focus_source_segment(segment, window, cx);
                                        });
                                    })
                            },
                        )
                    })
                    .children(overview_stops.into_iter().enumerate().filter_map({
                        let overview_editor = curve_editor.clone();
                        move |(stop, position)| {
                            (stop > 0 && stop + 1 < source_stop_count).then(|| {
                                let drag = StopPositionDrag { stop };
                                let start_editor = overview_editor.clone();
                                div()
                                    .id(("animation-stop-overview", stop))
                                    .absolute()
                                    .left(relative(position.clamp(0., 1.)))
                                    .top_0()
                                    .bottom_0()
                                    .ml(px(-Self::OVERVIEW_STOP_HANDLE_WIDTH * 0.5))
                                    .w(px(Self::OVERVIEW_STOP_HANDLE_WIDTH))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_col_resize()
                                    .when(dragging_stop == Some(stop), |style| {
                                        style.bg(colors.foreground.opacity(0.12))
                                    })
                                    .hover(move |style| style.bg(colors.foreground.opacity(0.18)))
                                    .active(move |style| style.bg(colors.foreground.opacity(0.28)))
                                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        cx.stop_propagation();
                                        start_editor.update(cx, |editor, cx| {
                                            editor.begin_stop_drag(stop, cx);
                                        });
                                    })
                                    .on_click(|_, _, cx| cx.stop_propagation())
                                    .on_drag(drag, |drag, _, _, cx| {
                                        cx.stop_propagation();
                                        cx.new(|_| drag.clone())
                                    })
                                    .child(
                                        div()
                                            .w(px(if dragging_stop == Some(stop) {
                                                3.
                                            } else {
                                                1.
                                            }))
                                            .h_full()
                                            .bg(if dragging_stop == Some(stop) {
                                                colors.foreground
                                            } else {
                                                colors.muted_foreground
                                            }),
                                    )
                            })
                        }
                    }))
                    .child(
                        div()
                            .absolute()
                            .left(relative(overview_playhead_progress.clamp(0., 1.)))
                            .top_0()
                            .bottom_0()
                            .ml(px(-1.))
                            .w(px(2.))
                            .bg(colors.foreground),
                    )
                    .context_menu(move |menu, window, cx| {
                        let position = window.mouse_position();
                        let (source_stop, frame) = {
                            let editor = overview_context_menu_editor.read(cx);
                            (
                                editor.overview_stop_at_position(position, cx),
                                editor.frame_at_overview_position(position, cx),
                            )
                        };
                        if let Some(source_stop) = source_stop {
                            if source_stop == 0 || source_stop + 1 == source_stop_count {
                                return menu
                                    .item(PopupMenuItem::Label("端のstopは削除できません".into()));
                            }
                            let remove_editor = overview_context_menu_editor.clone();
                            return menu.item(PopupMenuItem::new("stopを削除").on_click(
                                move |_, _, cx| {
                                    remove_editor.update(cx, |editor, cx| {
                                        editor.remove_source_stop(source_stop, cx);
                                    });
                                },
                            ));
                        }
                        let Some(frame) = frame else {
                            return menu;
                        };
                        if !overview_context_menu_editor
                            .read(cx)
                            .can_add_stop_at_frame(frame, cx)
                        {
                            return menu.item(PopupMenuItem::Label(
                                "このフレームにはstopを追加できません".into(),
                            ));
                        }
                        let add_stop_editor = overview_context_menu_editor.clone();
                        menu.item(PopupMenuItem::new("この位置にstopを追加").on_click(
                            move |_, _, cx| {
                                add_stop_editor.update(cx, |editor, cx| {
                                    editor.add_stop_at_frame(frame, cx);
                                });
                            },
                        ))
                    }),
            );
        let capture_scrub_editor = curve_editor.clone();

        div()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .capture_any_mouse_up(move |event: &MouseUpEvent, _, cx| {
                if event.button == MouseButton::Left {
                    capture_scrub_editor.update(cx, |editor, cx| {
                        // Capture releases over graph children and the pane header.
                        editor.end_graph_press(cx);
                        editor.end_pointer_drag();
                        editor.finish_history_drag(cx);
                    });
                }
            })
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|editor, _, _, cx| {
                    editor.end_graph_press(cx);
                    editor.end_pointer_drag();
                    editor.finish_history_drag(cx);
                }),
            )
            .child(
                pane_header(colors).child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(title),
                ),
            )
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .p_3()
                    .gap_2()
                    .child(segment_overview)
                    .child(graph),
            )
            .into_any_element()
    }
}
