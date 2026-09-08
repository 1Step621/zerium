use super::*;

fn curve_sample_count(
    plot_width: f32,
    viewport_span: f32,
    curve_span: f32,
    minimum: usize,
) -> usize {
    let visible_pixels = plot_width * curve_span.abs() / viewport_span.max(f32::EPSILON);
    ((visible_pixels * 1.5).ceil() as usize)
        .max(minimum)
        .min(4096)
}

impl AnimationCurveEditor {
    pub(super) fn graph_canvas(
        curve: AnimationCurve,
        background_curves: Vec<AnimationCurve>,
        time: CurveTimeView,
        viewport: GraphViewport,
        grid: CurveGrid,
        colors: CurvePaintColors,
        editor: Entity<Self>,
    ) -> impl IntoElement {
        canvas(
            move |bounds, _, cx| {
                let selected_segment = editor.update(cx, |editor, cx| {
                    if editor.graph_bounds != Some(bounds) {
                        editor.graph_bounds = Some(bounds);
                        cx.notify();
                    }
                    editor.selected_segment
                });
                CurvePaintState {
                    curve: curve.clone(),
                    background_curves: background_curves.clone(),
                    playhead_progress: time.playhead_progress,
                    visible_progress_range: time.visible_progress_range,
                    selected_segment,
                    viewport,
                    grid: grid.clone(),
                    grid_major: colors.grid_major,
                    grid_minor: colors.grid_minor,
                    handle_color: colors.handle,
                    playhead_color: colors.playhead,
                    curve_color: colors.curve,
                    background_curve_color: colors.background_curve,
                }
            },
            move |bounds, state, window, _| {
                let origin = point(
                    bounds.origin.x + px(Self::GRAPH_INSET_LEFT),
                    bounds.origin.y + px(Self::GRAPH_INSET_TOP),
                );
                let width =
                    bounds.size.width - px(Self::GRAPH_INSET_LEFT + Self::GRAPH_INSET_RIGHT);
                let height =
                    bounds.size.height - px(Self::GRAPH_INSET_TOP + Self::GRAPH_INSET_BOTTOM);
                let to_point = |position: [f32; 2]| {
                    let screen = state.viewport.screen_position(position);
                    point(
                        origin.x + width * screen[0],
                        origin.y + height * (1. - screen[1]),
                    )
                };

                let mut minor_grid_path = PathBuilder::stroke(px(1.));
                for x in &state.grid.minor {
                    minor_grid_path.move_to(to_point([*x, 0.]));
                    minor_grid_path.line_to(to_point([*x, 1.]));
                }
                if let Ok(path) = minor_grid_path.build() {
                    window.paint_path(path, state.grid_minor);
                }

                let mut major_grid_path = PathBuilder::stroke(px(1.));
                for (_, x) in &state.grid.major {
                    major_grid_path.move_to(to_point([*x, 0.]));
                    major_grid_path.line_to(to_point([*x, 1.]));
                }
                for (_, normalized) in &state.grid.values {
                    major_grid_path.move_to(to_point([state.viewport.x_min, *normalized]));
                    major_grid_path.line_to(to_point([state.viewport.x_max, *normalized]));
                }
                if let Ok(path) = major_grid_path.build() {
                    window.paint_path(path, state.grid_major);
                }

                let mut playhead_path = PathBuilder::stroke(px(1.));
                playhead_path.move_to(to_point([state.playhead_progress, 0.]));
                playhead_path.line_to(to_point([state.playhead_progress, 1.]));
                if let Ok(path) = playhead_path.build() {
                    window.paint_path(path, state.playhead_color);
                }

                let visible_start = state.visible_progress_range[0].clamp(0., 1.);
                let visible_end = state.visible_progress_range[1].clamp(0., 1.);
                let draw_start = visible_start.max(state.viewport.x_min);
                let draw_end = visible_end.min(state.viewport.x_max);
                let plot_width = f32::from(width).max(1.);
                if draw_start <= draw_end {
                    for curve in &state.background_curves {
                        let mut curve_path = PathBuilder::stroke(px(1.));
                        let mut started = false;
                        for anchors in curve.anchors().windows(2) {
                            let start = anchors[0][0].max(draw_start);
                            let end = anchors[1][0].min(draw_end);
                            if start > end {
                                continue;
                            }
                            let start_point = to_point([start, curve.evaluate(start)]);
                            if started {
                                curve_path.line_to(start_point);
                            } else {
                                curve_path.move_to(start_point);
                                started = true;
                            }
                            let samples = curve_sample_count(
                                plot_width,
                                state.viewport.x_span(),
                                end - start,
                                16,
                            );
                            for step in 1..=samples {
                                let progress = start + (end - start) * step as f32 / samples as f32;
                                curve_path.line_to(to_point([progress, curve.evaluate(progress)]));
                            }
                        }
                        if started && let Ok(path) = curve_path.build() {
                            window.paint_path(path, state.background_curve_color);
                        }
                    }
                }

                let anchors = state.curve.anchors();
                let mut handle_path = PathBuilder::stroke(px(1.));
                for (index, anchor) in anchors.iter().enumerate() {
                    if !(visible_start..=visible_end).contains(&(*anchor)[0]) {
                        continue;
                    }
                    if index > 0
                        && state.curve.is_custom(index - 1)
                        && let Some(control) = state
                            .curve
                            .control(index, crate::domain::animation::BezierHandle::In)
                    {
                        handle_path.move_to(to_point(*anchor));
                        handle_path.line_to(to_point(control));
                    }
                    if index + 1 < anchors.len()
                        && state.curve.is_custom(index)
                        && let Some(control) = state
                            .curve
                            .control(index, crate::domain::animation::BezierHandle::Out)
                    {
                        handle_path.move_to(to_point(*anchor));
                        handle_path.line_to(to_point(control));
                    }
                }
                if let Ok(path) = handle_path.build() {
                    window.paint_path(path, state.handle_color);
                }

                if draw_start <= draw_end {
                    let mut curve_path = PathBuilder::stroke(px(2.));
                    let mut started = false;
                    for anchors in state.curve.anchors().windows(2) {
                        let start = anchors[0][0].max(draw_start);
                        let end = anchors[1][0].min(draw_end);
                        if start > end {
                            continue;
                        }
                        let start_point = to_point([start, state.curve.evaluate(start)]);
                        if started {
                            curve_path.line_to(start_point);
                        } else {
                            curve_path.move_to(start_point);
                            started = true;
                        }
                        let samples = curve_sample_count(
                            plot_width,
                            state.viewport.x_span(),
                            end - start,
                            16,
                        );
                        for step in 1..=samples {
                            let progress = start + (end - start) * step as f32 / samples as f32;
                            curve_path
                                .line_to(to_point([progress, state.curve.evaluate(progress)]));
                        }
                    }
                    if started && let Ok(path) = curve_path.build() {
                        window.paint_path(path, state.curve_color);
                    }
                }

                if let Some(segment) = state.selected_segment
                    && let Some(end) = segment.checked_add(1)
                    && let Some(anchors) = state.curve.anchors().get(segment..=end)
                {
                    let start = anchors[0][0].max(draw_start);
                    let end = anchors[1][0].min(draw_end);
                    if start <= end {
                        let mut selected_path = PathBuilder::stroke(px(4.));
                        selected_path.move_to(to_point([start, state.curve.evaluate(start)]));
                        let samples = curve_sample_count(
                            plot_width,
                            state.viewport.x_span(),
                            end - start,
                            32,
                        );
                        for step in 1..=samples {
                            let progress = start + (end - start) * step as f32 / samples as f32;
                            selected_path
                                .line_to(to_point([progress, state.curve.evaluate(progress)]));
                        }
                        if let Ok(path) = selected_path.build() {
                            window.paint_path(path, state.curve_color);
                        }
                    }
                }
            },
        )
        .absolute()
        .size_full()
    }
}
