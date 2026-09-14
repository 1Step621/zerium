use super::*;

impl GraphCurve {
    pub(super) fn stops(&self) -> &[[f32; 2]] {
        &self.stops
    }

    pub(super) fn interpolation(&self, segment: usize) -> Option<SegmentInterpolation> {
        self.interpolations.get(segment).copied()
    }

    pub(super) fn is_custom(&self, segment: usize) -> bool {
        self.interpolations
            .get(segment)
            .is_some_and(|interpolation| interpolation.is_custom())
    }

    fn segment_for_handle(index: usize, handle: BezierHandle) -> Option<usize> {
        match handle {
            BezierHandle::In => index.checked_sub(1),
            BezierHandle::Out => Some(index),
        }
    }

    pub(super) fn handle_position(&self, index: usize, handle: BezierHandle) -> Option<[f32; 2]> {
        let segment = Self::segment_for_handle(index, handle)?;
        let local = self.interpolations.get(segment)?.handle_position(handle)?;
        let start = self.stops.get(segment)?;
        let end = self.stops.get(segment + 1)?;
        Some([
            start[0] + (end[0] - start[0]) * local[0],
            start[1] + (end[1] - start[1]) * local[1],
        ])
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
                    point[1].clamp(0., 1.)
                } else {
                    ((point[1] - start[1]) / dy).clamp(0., 1.)
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
        let selection = self.selection.read(cx);
        let target = selection.target()?.clone();
        let focused_segment = selection.focused_segment();
        let editor = self.editor.read(cx);
        let item = editor.selected_item()?;
        if item.id != target.item_id {
            return None;
        }
        let presentation =
            crate::ui::property_inspector::PropertyInspector::animation_presentation(
                editor, &item, &target,
            )?;
        let track = item.animation(target.effect_id, &target.address)?;
        let timeline_item = editor.item(item.id)?;
        let source_progress =
            timeline_item.animation_progress_at_time(TimelineTime::from_frame(editor.playhead()));
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
                .map(|(_, value)| *value * presentation.value_factor)
                .collect::<Vec<_>>()
        });
        let (value_min, value_max, stops, axis_suffix) = if let Some(values) = numeric_stops {
            let minimum = values.iter().copied().min_by(f64::total_cmp)?;
            let maximum = values.iter().copied().max_by(f64::total_cmp)?;
            let padding = if (maximum - minimum).abs() <= f64::EPSILON {
                (presentation.step * 10.).max(minimum.abs() * 0.1).max(1.)
            } else {
                0.
            };
            let from = minimum - padding;
            let to = maximum + padding;
            let stops = values
                .into_iter()
                .enumerate()
                .map(|(index, value)| [index as f32, ((value - from) / (to - from)) as f32])
                .collect();
            (from, to, stops, presentation.suffix.clone())
        } else {
            (0., 100., vec![[0., 0.], [1., 1.]], "%".to_owned())
        };
        let animation = GraphAnimation {
            value_min,
            value_max,
            curve: GraphCurve {
                stops,
                interpolations: vec![*track.interpolations().get(source_segment)?],
            },
        };
        let frame_rate = editor.frame_rate();
        let frames_per_second = frame_rate.frames_per_second();
        let animation_start_frame = timeline_item.animation_timeline_frame(source_progress_start);
        let animation_span_frames = timeline_item.animation_span_frames()
            * f64::from(source_progress_end - source_progress_start);
        let start_seconds = (animation_start_frame / frames_per_second) as f32;
        let duration_seconds = (animation_span_frames / frames_per_second) as f32;
        Some(SelectedCurve {
            target,
            presentation,
            animation,
            axis_suffix,
            source_segment,
            source_stop_count: track.stops().len(),
            source_stop_positions,
            source_playhead_progress: source_progress.clamp(0., 1.),
            playhead_progress,
            clip_start: timeline_item.start,
            clip_duration: timeline_item.duration,
            animation_start_frame,
            animation_span_frames,
            start_seconds,
            duration_seconds,
            frame_rate,
        })
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.graph_interaction = GraphInteraction::HandleDrag { point };
        self.finish_playhead_scrub(cx);
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        self.selected_segment = None;
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    pub(super) fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected_segment.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn finish_history_drag(&mut self, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
    }
}
