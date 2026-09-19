use super::*;

fn curve_sample_count(plot_width: f32, curve_span: f32, minimum: usize) -> usize {
    let visible_pixels = plot_width * curve_span.abs();
    ((visible_pixels * 1.5).ceil() as usize)
        .max(minimum)
        .min(4096)
}

impl AnimationCurveEditor {
    pub(super) fn graph_canvas(
        curve: GraphCurve,
        playhead_progress: f32,
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
                    playhead_progress,
                    selected_segment,
                    grid: grid.clone(),
                    grid_major: colors.grid_major,
                    grid_minor: colors.grid_minor,
                    handle_color: colors.handle,
                    playhead_color: colors.playhead,
                    curve_color: colors.curve,
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
                    point(
                        origin.x + width * position[0],
                        origin.y + height * (1. - position[1]),
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
                    major_grid_path.move_to(to_point([0., *normalized]));
                    major_grid_path.line_to(to_point([1., *normalized]));
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

                let plot_width = f32::from(width).max(1.);
                let stops = &state.curve.stops;
                let mut handle_path = PathBuilder::stroke(px(1.));
                for (index, stop) in stops.iter().enumerate() {
                    if index > 0
                        && let Some(handle) = state
                            .curve
                            .handle_position(index, crate::domain::animation::BezierHandle::In)
                    {
                        handle_path.move_to(to_point(*stop));
                        handle_path.line_to(to_point(handle));
                    }
                    if index + 1 < stops.len()
                        && let Some(handle) = state
                            .curve
                            .handle_position(index, crate::domain::animation::BezierHandle::Out)
                    {
                        handle_path.move_to(to_point(*stop));
                        handle_path.line_to(to_point(handle));
                    }
                }
                if let Ok(path) = handle_path.build() {
                    window.paint_path(path, state.handle_color);
                }

                let mut curve_path = PathBuilder::stroke(px(2.));
                let mut started = false;
                for stops in state.curve.stops.windows(2) {
                    let start = stops[0][0];
                    let end = stops[1][0];
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
                    let samples = curve_sample_count(plot_width, end - start, 16);
                    for step in 1..=samples {
                        let progress = start + (end - start) * step as f32 / samples as f32;
                        curve_path.line_to(to_point([progress, state.curve.evaluate(progress)]));
                    }
                }
                if started && let Ok(path) = curve_path.build() {
                    window.paint_path(path, state.curve_color);
                }

                if let Some(segment) = state.selected_segment
                    && let Some(end) = segment.checked_add(1)
                    && let Some(stops) = state.curve.stops.get(segment..=end)
                {
                    let start = stops[0][0];
                    let end = stops[1][0];
                    if start <= end {
                        let mut selected_path = PathBuilder::stroke(px(4.));
                        selected_path.move_to(to_point([start, state.curve.evaluate(start)]));
                        let samples = curve_sample_count(plot_width, end - start, 32);
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
