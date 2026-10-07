use super::*;

impl GraphCurve {
    pub(super) fn has_handles(&self) -> bool {
        self.interpolations
            .iter()
            .any(|interpolation| matches!(interpolation, SegmentInterpolation::Custom(_)))
    }

    fn segment_for_handle(index: usize, handle: BezierHandle) -> Option<usize> {
        match handle {
            BezierHandle::In => index.checked_sub(1),
            BezierHandle::Out => Some(index),
        }
    }

    pub(super) fn handle_position(&self, index: usize, handle: BezierHandle) -> Option<[f32; 2]> {
        let segment = Self::segment_for_handle(index, handle)?;
        let SegmentInterpolation::Custom(curve) = self.interpolations.get(segment)? else {
            return None;
        };
        let local = curve.handle(handle);
        let start = self.stops.get(segment)?;
        let end = self.stops.get(segment + 1)?;
        Some([
            start[0] + (end[0] - start[0]) * local[0],
            start[1] + (end[1] - start[1]) * local[1],
        ])
    }

    /// Off-screen controls are marked where their guide reaches the plot edge.
    pub(super) fn handle_display_position(
        &self,
        index: usize,
        handle: BezierHandle,
    ) -> Option<[f32; 2]> {
        let position = self.handle_position(index, handle)?;
        let edge = position[1].clamp(0., 1.);
        if edge == position[1] {
            return Some(position);
        }
        let stop = self.stops.get(index)?;
        let span = position[1] - stop[1];
        let progress = if span.abs() <= f32::EPSILON {
            1.
        } else {
            ((edge - stop[1]) / span).clamp(0., 1.)
        };
        Some([stop[0] + (position[0] - stop[0]) * progress, edge])
    }

    pub(super) fn local_handle_position(
        &self,
        index: usize,
        handle: BezierHandle,
        point: [f32; 2],
    ) -> Option<(usize, [f32; 2])> {
        let segment = Self::segment_for_handle(index, handle)?;
        let start = self.stops.get(segment)?;
        let end = self.stops.get(segment + 1)?;
        let dx = end[0] - start[0];
        if dx <= f32::EPSILON {
            return None;
        }
        let dy = end[1] - start[1];
        Some((
            segment,
            [
                ((point[0] - start[0]) / dx).clamp(0., 1.),
                if dy.abs() <= f32::EPSILON {
                    point[1]
                } else {
                    (point[1] - start[1]) / dy
                },
            ],
        ))
    }

    pub(super) fn evaluate(&self, progress: f32) -> f32 {
        let progress = progress.clamp(0., 1.);
        let right = self
            .stops
            .partition_point(|stop| stop[0] < progress)
            .min(self.stops.len().saturating_sub(1));
        if right == 0 || self.stops[right][0] == progress {
            return self.stops.get(right).map_or(0., |stop| stop[1]);
        }
        let left = right - 1;
        let start = self.stops[left];
        let end = self.stops[right];
        let span = end[0] - start[0];
        if span <= 0. {
            return start[1];
        }
        let local = (progress - start[0]) / span;
        let eased = self
            .interpolations
            .get(left)
            .map_or(local, |interpolation| interpolation.evaluate(local));
        start[1] + (end[1] - start[1]) * eased
    }

    fn value_range(&self, include_handles: bool) -> [f32; 2] {
        let curve = self;
        let samples = (0..=64).map(|index| curve.evaluate(index as f32 / 64.));
        let handles = curve
            .stops
            .iter()
            .enumerate()
            .filter(|_| include_handles)
            .flat_map(|(index, _)| {
                [BezierHandle::In, BezierHandle::Out]
                    .into_iter()
                    .filter_map(move |handle| {
                        curve.handle_position(index, handle).map(|point| point[1])
                    })
            });
        let (minimum, maximum) = samples
            .chain(handles)
            .chain(self.stops.iter().map(|stop| stop[1]))
            .fold(
                (f32::INFINITY, f32::NEG_INFINITY),
                |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
            );
        let padding = ((maximum - minimum) * 0.15).max(0.1);
        [minimum - padding, maximum + padding]
    }
}

impl SelectedCurve {
    fn fitted_value_range(&self, include_handles: bool) -> [f64; 2] {
        self.curve
            .value_range(include_handles)
            .map(|value| self.value_min + (self.value_max - self.value_min) * f64::from(value))
    }

    fn set_value_range(&mut self, [minimum, maximum]: [f64; 2]) {
        for stop in &mut self.curve.stops {
            let value = self.value_min + (self.value_max - self.value_min) * f64::from(stop[1]);
            stop[1] = AnimationCurveEditor::normalized_value(value, minimum, maximum);
        }
        self.value_min = minimum;
        self.value_max = maximum;
    }
}

impl HandleFitTarget {
    pub(super) fn matches(&self, selected: &SelectedCurve) -> bool {
        selected.curve.has_handles()
            && self.address == selected.address
            && self.source_segment == selected.source_segment
            && self.source_positions
                == selected.source_stop_positions
                    [selected.source_segment..=selected.source_segment + 1]
    }
}

impl AnimationCurveEditor {
    fn source_segment_for_progress(
        stop_positions: &[f32],
        progress: f32,
        focused_segment: Option<usize>,
    ) -> Option<usize> {
        const BOUNDARY_EPSILON: f32 = 0.000_001;

        if !progress.is_finite() {
            return None;
        }
        let last_segment = stop_positions.len().checked_sub(2)?;
        if let Some(segment) = focused_segment.filter(|segment| *segment <= last_segment) {
            let start = *stop_positions.get(segment)?;
            let end = *stop_positions.get(segment + 1)?;
            if progress >= start - BOUNDARY_EPSILON && progress <= end + BOUNDARY_EPSILON {
                return Some(segment);
            }
        }
        Some(
            stop_positions
                .partition_point(|position| *position < progress)
                .saturating_sub(1)
                .min(last_segment),
        )
    }

    pub(crate) fn has_selected_curve(&self, cx: &App) -> bool {
        self.selected_curve(cx).is_some()
    }

    pub(super) fn selected_curve(&self, cx: &App) -> Option<SelectedCurve> {
        if let Some(view) = &self.handle_drag_view {
            let editor = self.editor.read(cx);
            let track = editor.item(view.address.item_id)?.animation_track(
                view.address.effect_id,
                &view.address.property_id,
                view.address.element_id,
                view.address.scalar_index,
            )?;
            let mut selected = view.clone();
            selected.curve.interpolations[0] = *track.interpolations().get(view.source_segment)?;
            return Some(selected);
        }
        let mut selected = self.source_curve(cx)?;
        let include_handles = self
            .handle_fit_target
            .as_ref()
            .is_some_and(|target| target.matches(&selected));
        selected.set_value_range(selected.fitted_value_range(include_handles));
        Some(selected)
    }

    fn source_curve(&self, cx: &App) -> Option<SelectedCurve> {
        let selection = self.selection.read(cx);
        let address = selection.address()?.clone();
        let focused_segment = selection.focused_segment();
        let editor = self.editor.read(cx);
        let item = editor.item(address.item_id)?;
        if !editor.is_item_selected(item.id) {
            return None;
        }
        let presentation = AnimationPresentation::for_address(editor, item, &address)?;
        let track = item.animation_track(
            address.effect_id,
            &address.property_id,
            address.element_id,
            address.scalar_index,
        )?;
        let source_progress =
            item.animation_progress_at_time(TimelineTime::from_frame(editor.playhead()));
        let source_stop_positions = track
            .stops()
            .iter()
            .map(|stop| stop.position())
            .collect::<Vec<_>>();
        let source_segment = Self::source_segment_for_progress(
            &source_stop_positions,
            source_progress,
            focused_segment,
        )?;
        self.selection.read(cx).focus_segment(source_segment);
        let source_progress_start = track.stops().get(source_segment)?.position();
        let source_progress_end = track.stops().get(source_segment + 1)?.position();
        let source_progress_span = source_progress_end - source_progress_start;
        let playhead_progress =
            ((source_progress - source_progress_start) / source_progress_span).clamp(0., 1.);
        let numeric_stops = track.numeric_stops().map(|stops| {
            stops[source_segment..=source_segment + 1]
                .iter()
                .map(|(_, value)| *value)
                .collect::<Vec<_>>()
        });
        let (value_min, value_max, stops, axis_suffix) = if let Some(values) = numeric_stops {
            let minimum = values.iter().copied().min_by(f64::total_cmp)?;
            let maximum = values.iter().copied().max_by(f64::total_cmp)?;
            let padding = if maximum == minimum {
                (presentation.step * 10.).max(minimum.abs() * 0.1).max(1.)
            } else {
                0.
            };
            let from = minimum - padding;
            let to = maximum + padding;
            let stops = values
                .into_iter()
                .enumerate()
                .map(|(index, value)| [index as f32, Self::normalized_value(value, from, to)])
                .collect();
            (from, to, stops, presentation.suffix.clone())
        } else {
            (0., 100., vec![[0., 0.], [1., 1.]], "%".to_owned())
        };
        let curve = GraphCurve {
            stops,
            interpolations: vec![*track.interpolations().get(source_segment)?],
        };
        let frame_rate = editor.frame_rate();
        let frames_per_second = frame_rate.frames_per_second();
        let animation_start_frame = item.animation_timeline_frame(source_progress_start);
        let animation_span_frames =
            item.animation_span_frames() * f64::from(source_progress_end - source_progress_start);
        let start_seconds = (animation_start_frame / frames_per_second) as f32;
        let duration_seconds = (animation_span_frames / frames_per_second) as f32;
        Some(SelectedCurve {
            address,
            presentation,
            value_min,
            value_max,
            curve,
            axis_suffix,
            source_segment,
            source_stop_count: track.stops().len(),
            source_stop_positions,
            source_playhead_progress: source_progress.clamp(0., 1.),
            playhead_progress,
            clip_start: item.start,
            clip_duration: item.duration,
            animation_start_frame,
            animation_span_frames,
            start_seconds,
            duration_seconds,
            frame_rate,
        })
    }

    pub(super) fn fit_value_view(&mut self, include_handles: bool, cx: &mut Context<Self>) {
        self.handle_fit_target = if include_handles {
            self.source_curve(cx)
                .filter(|selected| selected.curve.has_handles())
                .map(|selected| HandleFitTarget {
                    source_positions: [
                        selected.source_stop_positions[selected.source_segment],
                        selected.source_stop_positions[selected.source_segment + 1],
                    ],
                    address: selected.address,
                    source_segment: selected.source_segment,
                })
        } else {
            None
        };
        cx.notify();
    }

    pub(super) fn format_number(value: impl Into<f64>) -> String {
        let mut value = format!("{:.4}", value.into());
        while value.contains('.') && value.ends_with('0') {
            value.pop();
        }
        if value.ends_with('.') {
            value.pop();
        }
        if value == "-0" {
            value = "0".to_owned();
        }
        value
    }

    pub(super) fn begin_handle_drag(
        &mut self,
        point: CurvePoint,
        synchronize: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(edit) = self.editor.read(cx).begin_animation_edit(
            &selected.address,
            AnimationEditTarget::Segment(selected.source_segment),
            synchronize,
        ) else {
            return;
        };
        self.animation_edit = Some(edit);
        self.handle_drag_view = Some(selected);
        self.graph_interaction = GraphInteraction::HandleDrag { point };
        self.finish_playhead_scrub(cx);
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        cx.notify();
    }

    pub(super) fn finish_history_drag(&mut self, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
    }
}
