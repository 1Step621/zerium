use super::*;

fn curve_sample_count(plot_width: f32, curve_span: f32, minimum: usize) -> usize {
    let visible_pixels = plot_width * curve_span.abs();
    ((visible_pixels * 1.5).ceil() as usize)
        .max(minimum)
        .min(4096)
}

impl AnimationCurveEditor {
    pub(super) fn graph_canvas(
        curve: GraphSegment,
        playhead_progress: f32,
        grid: CurveGrid,
        colors: CurvePaintColors,
        editor: Entity<Self>,
    ) -> impl IntoElement {
        canvas(
            move |bounds, _, cx| {
                editor.update(cx, |editor, cx| {
                    if editor.graph_bounds != Some(bounds) {
                        editor.graph_bounds = Some(bounds);
                        cx.notify();
                    }
                });
                CurvePaintState {
                    curve: curve.clone(),
                    playhead_progress,
                    grid: grid.clone(),
                    colors,
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
                let scale = window.scale_factor();
                let pixel = px(1. / scale);
                let to_grid_point = |position| {
                    let position = to_point(position);
                    point(
                        px((f32::from(position.x) * scale).floor() / scale) + pixel * 0.5,
                        px((f32::from(position.y) * scale).floor() / scale) + pixel * 0.5,
                    )
                };

                window.with_content_mask(
                    Some(gpui::ContentMask {
                        bounds: Bounds::new(
                            origin,
                            gpui::size(width.max(px(0.)), height.max(px(0.))),
                        ),
                    }),
                    |window| {
                        let mut minor_grid_path = PathBuilder::stroke(pixel);
                        for x in &state.grid.frame_ticks {
                            minor_grid_path.move_to(to_grid_point([*x, 0.]));
                            minor_grid_path.line_to(to_grid_point([*x, 1.]));
                        }
                        if let Ok(path) = minor_grid_path.build() {
                            window.paint_path(path, state.colors.grid_minor);
                        }

                        let mut major_grid_path = PathBuilder::stroke(pixel);
                        for x in &state.grid.beat_ticks {
                            major_grid_path.move_to(to_grid_point([*x, 0.]));
                            major_grid_path.line_to(to_grid_point([*x, 1.]));
                        }
                        for (_, normalized) in &state.grid.values {
                            major_grid_path.move_to(to_grid_point([0., *normalized]));
                            major_grid_path.line_to(to_grid_point([1., *normalized]));
                        }
                        if let Ok(path) = major_grid_path.build() {
                            window.paint_path(path, state.colors.grid_major);
                        }

                        let mut playhead_path = PathBuilder::stroke(px(1.));
                        playhead_path.move_to(to_point([state.playhead_progress, 0.]));
                        playhead_path.line_to(to_point([state.playhead_progress, 1.]));
                        if let Ok(path) = playhead_path.build() {
                            window.paint_path(path, state.colors.playhead);
                        }

                        let plot_width = f32::from(width).max(1.);
                        let [start, end] = state.curve.endpoints;
                        let mut handle_path = PathBuilder::stroke(px(1.));
                        for (stop, side) in [(start, BezierHandle::Out), (end, BezierHandle::In)] {
                            if let Some(handle) = state.curve.handle_position(side) {
                                handle_path.move_to(to_point(stop));
                                handle_path.line_to(to_point(handle));
                            }
                        }
                        if let Ok(path) = handle_path.build() {
                            window.paint_path(path, state.colors.handle);
                        }

                        let [start, end] = state.curve.endpoints.map(|point| point[0]);
                        let mut curve_path = PathBuilder::stroke(px(2.));
                        curve_path.move_to(to_point([start, state.curve.evaluate(start)]));
                        let samples = curve_sample_count(plot_width, end - start, 16);
                        for step in 1..=samples {
                            let progress = start + (end - start) * step as f32 / samples as f32;
                            curve_path
                                .line_to(to_point([progress, state.curve.evaluate(progress)]));
                        }
                        if let Ok(path) = curve_path.build() {
                            window.paint_path(path, state.colors.curve);
                        }
                    },
                );
            },
        )
        .absolute()
        .size_full()
    }
}
