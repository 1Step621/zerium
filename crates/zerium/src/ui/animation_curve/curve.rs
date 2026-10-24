use super::*;
use crate::ui::animation_curve::normalized_value;

impl GraphSegment {
    pub(super) fn has_handles(&self) -> bool {
        matches!(self.interpolation, SegmentInterpolation::Custom(_))
    }

    pub(super) fn handle_position(&self, handle: BezierHandle) -> Option<[f32; 2]> {
        let SegmentInterpolation::Custom(curve) = self.interpolation else {
            return None;
        };
        let local = curve.handle(handle);
        let [start, end] = self.endpoints;
        Some([
            start[0] + (end[0] - start[0]) * local[0],
            start[1] + (end[1] - start[1]) * local[1],
        ])
    }

    /// Off-screen controls are marked where their guide reaches the plot edge.
    pub(super) fn handle_display_position(&self, handle: BezierHandle) -> Option<[f32; 2]> {
        let position = self.handle_position(handle)?;
        let edge = position[1].clamp(0., 1.);
        if edge == position[1] {
            return Some(position);
        }
        let stop = match handle {
            BezierHandle::Out => self.endpoints[0],
            BezierHandle::In => self.endpoints[1],
        };
        let span = position[1] - stop[1];
        let progress = if span.abs() <= f32::EPSILON {
            1.
        } else {
            ((edge - stop[1]) / span).clamp(0., 1.)
        };
        Some([stop[0] + (position[0] - stop[0]) * progress, edge])
    }

    pub(super) fn local_handle_position(&self, point: [f32; 2]) -> Option<[f32; 2]> {
        let [start, end] = self.endpoints;
        let dx = end[0] - start[0];
        if dx <= f32::EPSILON {
            return None;
        }
        let dy = end[1] - start[1];
        Some([
            ((point[0] - start[0]) / dx).clamp(0., 1.),
            if dy.abs() <= f32::EPSILON {
                point[1]
            } else {
                (point[1] - start[1]) / dy
            },
        ])
    }

    pub(super) fn evaluate(&self, progress: f32) -> f32 {
        let progress = progress.clamp(0., 1.);
        let [start, end] = self.endpoints;
        if progress <= start[0] {
            return start[1];
        }
        if progress >= end[0] {
            return end[1];
        }
        let span = end[0] - start[0];
        if span <= 0. {
            return start[1];
        }
        let local = (progress - start[0]) / span;
        let eased = self.interpolation.evaluate(local);
        start[1] + (end[1] - start[1]) * eased
    }

    /// Approximate smooth curves while preserving horizontal and vertical steps.
    pub(super) fn polyline(&self, plot_width: f32) -> impl Iterator<Item = [f32; 2]> + '_ {
        let [start, end] = self.endpoints;
        let visible_pixels = plot_width * (end[0] - start[0]).abs();
        let samples = ((visible_pixels * 1.5).ceil() as u64).clamp(16, 4096);
        let count = self
            .interpolation
            .step_count()
            .map(|count| u64::from(count.get()));
        let samples = count.map_or(samples, |count| count.min(samples));
        std::iter::once(start).chain((1..=samples).flat_map(move |step| {
            match count {
                Some(count) => {
                    // Coalesce subpixel steps while bounding work for large counts.
                    let progress = (step * count / samples) as f32 / count as f32;
                    let previous = ((step - 1) * count / samples) as f32 / count as f32;
                    let x = start[0] + (end[0] - start[0]) * progress;
                    let y = |progress| start[1] + (end[1] - start[1]) * progress;
                    [Some([x, y(previous)]), Some([x, y(progress)])]
                }
                None => {
                    let x = start[0] + (end[0] - start[0]) * step as f32 / samples as f32;
                    [Some([x, self.evaluate(x)]), None]
                }
            }
            .into_iter()
            .flatten()
        }))
    }

    fn value_range(&self, include_handles: bool) -> [f32; 2] {
        let samples = (0..=64).map(|index| self.evaluate(index as f32 / 64.));
        let handles = [BezierHandle::Out, BezierHandle::In]
            .into_iter()
            .filter(|_| include_handles)
            .filter_map(|handle| self.handle_position(handle).map(|point| point[1]));
        let (minimum, maximum) = samples
            .chain(handles)
            .chain(self.endpoints.iter().map(|stop| stop[1]))
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), value| {
                (min.min(value), max.max(value))
            });
        let padding = ((maximum - minimum) * 0.15).max(0.1);
        [minimum - padding, maximum + padding]
    }
}

impl SelectedCurve {
    pub(super) fn segment_identity(&self) -> SegmentIdentity {
        SegmentIdentity {
            address: self.address.clone(),
            source_segment: self.source_segment,
            source_frames: [
                self.source_stop_frames[self.source_segment],
                self.source_stop_frames[self.source_segment + 1],
            ],
        }
    }

    pub(super) fn frame_at_pattern_frame(&self, frame: Frame) -> Option<Frame> {
        self.clock
            .first_occurrence(
                frame,
                TimelineTime::from_frame(self.clip_start),
                TimelineTime::from_frames(
                    self.clip_start.get() as f64
                        + self.clip_duration.get().saturating_sub(1) as f64,
                ),
            )
            .map(TimelineTime::nearest_frame)
    }

    pub(super) fn stop_positions(&self) -> impl Iterator<Item = f32> + '_ {
        self.source_stop_frames
            .iter()
            .map(|frame| (frame.get() as f64 / self.clock.span_frames()) as f32)
    }

    fn fitted_value_range(&self, include_handles: bool) -> [f64; 2] {
        self.curve
            .value_range(include_handles)
            .map(|value| self.value_min + (self.value_max - self.value_min) * f64::from(value))
    }

    fn set_value_range(&mut self, [minimum, maximum]: [f64; 2]) {
        for stop in &mut self.curve.endpoints {
            let value = self.value_min + (self.value_max - self.value_min) * f64::from(stop[1]);
            stop[1] = normalized_value(value, minimum, maximum);
        }
        self.value_min = minimum;
        self.value_max = maximum;
    }
}

impl SegmentIdentity {
    pub(super) fn matches(&self, selected: &SelectedCurve) -> bool {
        self.address == selected.address
            && self.source_segment == selected.source_segment
            && selected
                .source_stop_frames
                .get(selected.source_segment..=selected.source_segment + 1)
                == Some(self.source_frames.as_slice())
    }
}

impl AnimationCurveEditor {
    pub(crate) fn has_visible_curve(&self, cx: &App) -> bool {
        self.selection.read(cx).address().is_some_and(|address| {
            self.editor
                .read(cx)
                .is_item_active_selected(address.item_id)
        }) && self.selected_curve(cx).is_some()
    }

    pub(super) fn selected_curve(&self, cx: &App) -> Option<SelectedCurve> {
        if let GraphInteraction::HandleDrag { view, .. } = &self.graph_interaction {
            let editor = self.editor.read(cx);
            let track = editor.animation_track(&view.address)?;
            let mut selected = view.as_ref().clone();
            selected.curve.interpolation = *track.interpolations().get(view.source_segment)?;
            return Some(selected);
        }
        let mut selected = self.source_curve(cx)?;
        let include_handles = self
            .handle_fit_target
            .as_ref()
            .is_some_and(|target| selected.curve.has_handles() && target.matches(&selected));
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
        let track = editor.animation_track(&address)?;
        let clock = item.animation_clock(track);
        let source_frame = clock.local_frame_at(TimelineTime::from_frame(editor.playhead()));
        let source_stop_frames = track
            .stops()
            .iter()
            .map(|stop| stop.frame())
            .collect::<Vec<_>>();
        let source_segment =
            source_segment_for_frame(&source_stop_frames, source_frame, focused_segment)?;
        self.selection.read(cx).focus_segment(source_segment);
        let source_frame_start = track.stops().get(source_segment)?.frame();
        let source_frame_end = track.stops().get(source_segment + 1)?.frame();
        let segment_span = (source_frame_end.get() - source_frame_start.get()) as f64;
        let playhead_progress =
            ((source_frame - source_frame_start.get() as f64) / segment_span).clamp(0., 1.) as f32;
        let numeric_stops = [source_segment, source_segment + 1]
            .map(|index| track.stops()[index].value().numeric_scalar());
        let (value_min, value_max, endpoints, axis_suffix) =
            if let [Some(start), Some(end)] = numeric_stops {
                let minimum = start.min(end);
                let maximum = start.max(end);
                let padding = if maximum == minimum {
                    (presentation.step * 10.).max(minimum.abs() * 0.1).max(1.)
                } else {
                    0.
                };
                let from = minimum - padding;
                let to = maximum + padding;
                let endpoints = [
                    [0., normalized_value(start, from, to)],
                    [1., normalized_value(end, from, to)],
                ];
                (from, to, endpoints, presentation.suffix.clone())
            } else {
                (0., 100., [[0., 0.], [1., 1.]], "%".to_owned())
            };
        let curve = GraphSegment {
            endpoints,
            interpolation: *track.interpolations().get(source_segment)?,
        };
        let frame_rate = editor.frame_rate();
        let frames_per_second = frame_rate.frames_per_second();
        let animation_start_frame = clock.key_time_at(source_frame_start).frames();
        let animation_span_frames = segment_span;
        let axis_start = if track.repeat().mode() != RepeatMode::None {
            animation_start_frame - clock.start_time().frames()
        } else {
            animation_start_frame
        };
        let start_seconds = (axis_start / frames_per_second) as f32;
        let duration_seconds = (animation_span_frames / frames_per_second) as f32;
        Some(SelectedCurve {
            address,
            presentation,
            value_min,
            value_max,
            curve,
            axis_suffix,
            source_segment,
            source_stop_frames,
            source_playhead_progress: (source_frame / clock.span_frames()).clamp(0., 1.) as f32,
            playhead_progress,
            clock,
            repeat: track.repeat(),
            clip_start: item.start,
            clip_duration: item.duration,
            start_seconds,
            duration_seconds,
            frame_rate,
        })
    }

    pub(super) fn fit_value_view(&mut self, include_handles: bool, cx: &mut Context<Self>) {
        self.handle_fit_target = if include_handles {
            self.source_curve(cx)
                .filter(|selected| selected.curve.has_handles())
                .map(|selected| selected.segment_identity())
        } else {
            None
        };
        cx.notify();
    }
}

fn source_segment_for_frame(
    stop_frames: &[Frame],
    frame: f64,
    focused_segment: Option<usize>,
) -> Option<usize> {
    if !frame.is_finite() {
        return None;
    }
    let last_segment = stop_frames.len().checked_sub(2)?;
    if let Some(segment) = focused_segment.filter(|segment| *segment <= last_segment) {
        let start = stop_frames.get(segment)?.get() as f64;
        let end = stop_frames.get(segment + 1)?.get() as f64;
        if frame >= start && frame <= end {
            return Some(segment);
        }
    }
    Some(
        stop_frames
            .partition_point(|stop| (stop.get() as f64) < frame)
            .saturating_sub(1)
            .min(last_segment),
    )
}

pub(in crate::ui::animation_curve) fn format_number(value: impl Into<f64>) -> String {
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
