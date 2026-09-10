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
        let title = selected.presentation.label.clone();
        let curve = selected.animation.curve.clone();
        let visible_progress_range = selected.visible_progress_range;
        let progress_is_visible = |progress: f32| {
            (visible_progress_range[0]..=visible_progress_range[1]).contains(&progress)
        };
        let background_curves = selected.background_curves;
        let background_anchors = background_curves
            .iter()
            .flat_map(|curve| curve.anchors().iter().copied())
            .filter(|position| progress_is_visible(position[0]))
            .collect::<Vec<_>>();
        let playhead_progress = selected.playhead_progress;
        let curve_editor = cx.entity();
        let focus_handle = self.focus_handle.clone();
        let graph_editor = curve_editor.clone();
        let anchors = curve.anchors().to_vec();
        let anchor_count = anchors.len();
        let custom_segments = (0..anchor_count.saturating_sub(1))
            .map(|segment| curve.is_custom(segment))
            .collect::<Vec<_>>();
        let selected_point = self.selected_point.filter(|point| match point {
            CurvePoint::Anchor(_) => true,
            CurvePoint::HandleIn(index) => index
                .checked_sub(1)
                .and_then(|segment| custom_segments.get(segment))
                .copied()
                .unwrap_or(false),
            CurvePoint::HandleOut(index) => custom_segments.get(*index).copied().unwrap_or(false),
        });
        let selected_segment = self
            .selected_segment
            .filter(|segment| segment.checked_add(1).is_some_and(|end| end < anchor_count));
        let playhead_position = [playhead_progress, curve.evaluate(playhead_progress)];
        let playhead_screen = self.viewport.screen_position(playhead_position);
        let anchor_editor = curve_editor.clone();
        let scroll_editor = curve_editor.clone();
        let axis_suffix = selected.axis_suffix.clone();
        let axis_viewport = self.viewport;
        let mut curve_grid = self.time_grid(
            selected.start_seconds,
            selected.duration_seconds,
            selected.frame_rate,
        );
        curve_grid.values = Self::value_grid(&selected.animation);
        let value_ticks = curve_grid.values.clone();
        let time_ticks = curve_grid.major.clone();
        let graph_viewport = self.viewport;
        let begin_scrub_editor = curve_editor.clone();
        let update_scrub_editor = curve_editor.clone();
        let finish_scrub_editor = curve_editor.clone();
        let value_drag_editor = curve_editor.clone();
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
            // becomes a scrub, a quiet release becomes a click (select /
            // deselect+seek / anchor add). Mousedown never selects.
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                begin_scrub_editor.update(cx, |editor, cx| {
                    editor.graph_press_started(event.position, cx);
                });
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
            .on_scroll_wheel(move |event, window, cx| {
                scroll_editor.update(cx, |editor, cx| editor.scroll_graph(event, window, cx));
            })
            .on_click({
                let curve_editor = curve_editor.clone();
                move |event, window, cx| {
                    if event.click_count() >= 2 {
                        let position = window.mouse_position();
                        let snap_disabled = window.modifiers().alt;
                        curve_editor.update(cx, |editor, cx| {
                            editor.graph_double_click(position, snap_disabled, window, cx)
                        });
                    } else {
                        // Exactly one hit test per click: a scrub drag is swallowed,
                        // otherwise the segment is selected or the selection cleared.
                        let position = window.mouse_position();
                        curve_editor.update(cx, |editor, cx| {
                            editor.resolve_graph_click(position, window, cx)
                        });
                    }
                }
            })
            .on_drag_move(move |event: &gpui::DragMoveEvent<CurvePointDrag>, _, cx| {
                let drag = event.drag(cx).clone();
                let position = event.event.position;
                graph_editor.update(cx, |editor, cx| {
                    editor.move_point(drag.point, position, event.event.modifiers.alt, cx)
                });
            })
            .on_drag_move(
                move |event: &gpui::DragMoveEvent<CurveValueDrag>, window, cx| {
                    let drag = event.drag(cx).clone();
                    cx.set_active_drag_cursor_style(CursorStyle::ResizeLeftRight, window);
                    value_drag_editor.update(cx, |editor, cx| {
                        editor.handle_point_value_drag(
                            &drag,
                            f32::from(event.event.position.x),
                            event.event.modifiers.shift,
                            window,
                            cx,
                        );
                    });
                },
            )
            .child(Self::graph_canvas(
                curve.clone(),
                background_curves,
                CurveTimeView {
                    playhead_progress,
                    visible_progress_range,
                },
                self.viewport,
                curve_grid,
                CurvePaintColors {
                    grid_major: colors.border.opacity(0.45),
                    grid_minor: colors.border.opacity(0.20),
                    handle: colors.muted_foreground.opacity(0.65),
                    playhead: colors.warning.opacity(0.75),
                    curve: colors.primary,
                    background_curve: colors.muted_foreground.opacity(0.32),
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
                let screen_x = axis_viewport.screen_position([x, 0.])[0];
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
            .when(Self::screen_position_is_visible(playhead_screen), |this| {
                this.child(
                    div()
                        .absolute()
                        .left(relative(playhead_screen[0]))
                        .bottom(relative(playhead_screen[1]))
                        .ml(px(Self::GRAPH_INSET_LEFT * (1. - playhead_screen[0])
                            - Self::GRAPH_INSET_RIGHT * playhead_screen[0]
                            - 4.))
                        .mb(px(Self::GRAPH_INSET_BOTTOM * (1. - playhead_screen[1])
                            - Self::GRAPH_INSET_TOP * playhead_screen[1]
                            - 4.))
                        .size(px(8.))
                        .rounded_full()
                        .border_1()
                        .border_color(colors.background)
                        .bg(colors.warning),
                )
            })
            .children(background_anchors.into_iter().filter_map(move |position| {
                let (screen, margin_left, margin_bottom) =
                    Self::point_layout(graph_viewport, position, 3.);
                Self::screen_position_is_visible(screen).then(|| {
                    div()
                        .absolute()
                        .left(relative(screen[0]))
                        .bottom(relative(screen[1]))
                        .ml(px(margin_left))
                        .mb(px(margin_bottom))
                        .size(px(6.))
                        .rounded_full()
                        .bg(colors.muted_foreground.opacity(0.38))
                })
            }))
            .children(anchors.iter().enumerate().flat_map({
                let curve_editor = curve_editor.clone();
                let curve = curve.clone();
                let custom_segments = custom_segments.clone();
                move |(index, anchor)| {
                    let mut handles = Vec::with_capacity(2);
                    if !(visible_progress_range[0]..=visible_progress_range[1]).contains(&anchor[0])
                    {
                        return handles;
                    }
                    if index > 0 && custom_segments[index - 1] {
                        let point = CurvePoint::HandleIn(index);
                        let (screen, margin_left, margin_bottom) = Self::point_layout(
                            graph_viewport,
                            curve.control(index, BezierHandle::In).unwrap_or(*anchor),
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
                                    .border_color(if selected_point == Some(point) {
                                        colors.primary
                                    } else {
                                        colors.muted_foreground
                                    })
                                    .bg(colors.background)
                                    .cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                        cx.stop_propagation();
                                        select_editor.update(cx, |editor, cx| {
                                            editor.select_point(point, window, cx);
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
                    if index + 1 < anchor_count && custom_segments[index] {
                        let point = CurvePoint::HandleOut(index);
                        let (screen, margin_left, margin_bottom) = Self::point_layout(
                            graph_viewport,
                            curve.control(index, BezierHandle::Out).unwrap_or(*anchor),
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
                                    .border_color(if selected_point == Some(point) {
                                        colors.primary
                                    } else {
                                        colors.muted_foreground
                                    })
                                    .bg(colors.background)
                                    .cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                        cx.stop_propagation();
                                        select_editor.update(cx, |editor, cx| {
                                            editor.select_point(point, window, cx);
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
            }))
            .children(
                anchors
                    .into_iter()
                    .enumerate()
                    .filter_map(move |(index, anchor)| {
                        if !(visible_progress_range[0]..=visible_progress_range[1])
                            .contains(&anchor[0])
                        {
                            return None;
                        }
                        let point = CurvePoint::Anchor(index);
                        let (screen, margin_left, margin_bottom) =
                            Self::point_layout(graph_viewport, anchor, 7.);
                        if !Self::screen_position_is_visible(screen) {
                            return None;
                        }
                        let drag = CurvePointDrag { point };
                        let remove_editor = anchor_editor.clone();
                        let select_editor = anchor_editor.clone();
                        Some(
                            div()
                                .id(("animation-anchor", index))
                                .absolute()
                                .left(relative(screen[0]))
                                .bottom(relative(screen[1]))
                                .ml(px(margin_left))
                                .mb(px(margin_bottom))
                                .size(px(14.))
                                .rounded_full()
                                .border_2()
                                .border_color(if selected_point == Some(point) {
                                    colors.foreground
                                } else {
                                    colors.background
                                })
                                .bg(colors.primary)
                                .cursor_pointer()
                                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                    cx.stop_propagation();
                                    select_editor.update(cx, |editor, cx| {
                                        editor.select_point(point, window, cx);
                                    });
                                })
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .on_drag(drag, |drag, _, _, cx| {
                                    cx.stop_propagation();
                                    cx.new(|_| drag.clone())
                                })
                                .context_menu(move |menu, _, _| {
                                    if index == 0 || index + 1 == anchor_count {
                                        return menu;
                                    }
                                    menu.item(PopupMenuItem::new("アンカーを削除").on_click({
                                        let remove_editor = remove_editor.clone();
                                        move |_, window, cx| {
                                            remove_editor.update(cx, |editor, cx| {
                                                editor.remove_anchor(index, window, cx);
                                            });
                                        }
                                    }))
                                }),
                        )
                    }),
            );

        let point_editor = selected_point.and_then(|point| {
            let position = Self::curve_point_position(&curve, point)?;
            if !(visible_progress_range[0]..=visible_progress_range[1]).contains(&position[0]) {
                return None;
            }
            let screen = self.viewport.screen_position(position);
            if !Self::screen_position_is_visible(screen) {
                return None;
            }
            let bounds = self.graph_bounds?;
            let origin = Self::point_editor_origin(
                [f32::from(bounds.size.width), f32::from(bounds.size.height)],
                screen,
            );
            let label = match point {
                CurvePoint::Anchor(_) => "アンカー",
                CurvePoint::HandleIn(_) => "Inハンドル",
                CurvePoint::HandleOut(_) => "Outハンドル",
            };
            let value_disabled =
                (selected.animation.to - selected.animation.from).abs() <= f64::EPSILON;
            let time = format!(
                "{}秒",
                Self::format_number(
                    selected.start_seconds + position[0] * selected.duration_seconds
                )
            );
            let value_input = self.point_value_input.clone();
            let suffix = selected.axis_suffix.clone();
            let value_drag = CurveValueDrag {
                editor_id: curve_editor.entity_id(),
            };
            let prepare_drag_editor = curve_editor.clone();
            let drag_input = value_input.clone();
            let drag_focus_handle = focus_handle.clone();
            Some(
                div()
                    .absolute()
                    .left(px(origin[0]))
                    .top(px(origin[1]))
                    .w(px(Self::POINT_EDITOR_WIDTH))
                    .h(px(Self::POINT_EDITOR_HEIGHT))
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
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(label)
                            .child(time),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().text_xs().child("値"))
                            .child(
                                div()
                                    .id("animation-curve-value-drag")
                                    .flex_1()
                                    .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                                        prepare_drag_editor.update(cx, |editor, cx| {
                                            editor.prepare_point_value_drag(event, cx);
                                        });
                                    })
                                    .on_drag(value_drag, move |drag, _, window, cx| {
                                        cx.stop_propagation();
                                        drag_input
                                            .update(cx, |input, cx| input.unselect(window, cx));
                                        drag_focus_handle.focus(window, cx);
                                        cx.new(|_| drag.clone())
                                    })
                                    .child(
                                        NumberInput::new(&value_input)
                                            .small()
                                            .w_full()
                                            .disabled(value_disabled)
                                            .suffix(div().text_xs().child(suffix)),
                                    ),
                            ),
                    )
                    .into_any_element(),
            )
        });
        let segment_editor = selected_segment.and_then(|segment| {
            let interpolation = curve.interpolation(segment)?;
            // Anchored to the visible part of the segment (see
            // `segment_panel_position`) so a valid selection always shows
            // its menu, even when zoomed or overshooting the value range.
            let position = Self::segment_panel_position(&curve, segment, self.viewport)?;
            let screen = self.viewport.screen_position(position);
            let screen = [screen[0].clamp(0., 1.), screen[1].clamp(0., 1.)];
            let bounds = self.graph_bounds?;
            let origin = Self::point_editor_origin(
                [f32::from(bounds.size.width), f32::from(bounds.size.height)],
                screen,
            );
            let interpolation_editor = curve_editor.clone();
            Some(
                div()
                    .absolute()
                    .left(px(origin[0]))
                    .top(px(origin[1]))
                    .w(px(Self::POINT_EDITOR_WIDTH))
                    .h(px(Self::POINT_EDITOR_HEIGHT))
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
                            .child(format!("セグメント {}", segment + 1)),
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
                                                        segment,
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
                                                        segment,
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
                                                    editor.set_custom(segment, cx);
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
                                                        editor
                                                            .set_interpolation(segment, option, cx);
                                                    });
                                                }),
                                        )
                                    })
                            }),
                    )
                    .into_any_element(),
            )
        });
        let graph = graph.children(point_editor).children(segment_editor);
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
                        editor.finish_history_drag(cx);
                    });
                }
            })
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|editor, _, _, cx| {
                    editor.end_graph_press(cx);
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
                    .child(graph),
            )
            .into_any_element()
    }
}
