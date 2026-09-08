use super::*;

impl AnimationCurveEditor {
    pub(crate) fn has_selected_curve(&self, cx: &App) -> bool {
        self.selected_curve(cx).is_some()
    }

    pub(super) fn selected_curve(&self, cx: &App) -> Option<SelectedCurve> {
        let target = self.selection.read(cx).target()?.clone();
        let editor = self.editor.read(cx);
        let item = editor.selected_item()?;
        if item.id != target.item_id {
            return None;
        }
        let presentation =
            crate::ui::property_inspector::PropertyInspector::animation_presentation(
                editor, &item, &target,
            )?;
        let background_curves = item
            .animations
            .iter()
            .flat_map(|(candidate, animation)| {
                animation
                    .curves()
                    .filter(|(channel, _)| {
                        target.effect_id.is_some()
                            || candidate.parameter_id != target.parameter_id
                            || candidate.array_index != target.address.array_index
                            || *channel != target.address.channel
                    })
                    .map(|(_, curve)| curve.clone())
            })
            .chain(item.effects.iter().flat_map(|effect| {
                effect.animations.iter().flat_map(|(candidate, animation)| {
                    animation
                        .curves()
                        .filter(|(channel, _)| {
                            target.effect_id != Some(effect.id)
                                || candidate.parameter_id != target.parameter_id
                                || candidate.array_index != target.address.array_index
                                || *channel != target.address.channel
                        })
                        .map(|(_, curve)| curve.clone())
                })
            }))
            .collect();
        let animation = item
            .animation(
                target.effect_id,
                &target.parameter_id,
                target.address.array_index,
            )?
            .clone();
        let (from, to) = animation.endpoints(target.address.channel)?;
        let (from, to, axis_suffix) = match from.numeric_scalar().zip(to.numeric_scalar()) {
            Some((from, to)) => (
                from * presentation.value_scale,
                to * presentation.value_scale,
                presentation.suffix.clone(),
            ),
            None => (0., 100., "%".to_owned()),
        };
        let animation = GraphAnimation {
            from,
            to,
            curve: animation.curve(target.address.channel)?.clone(),
        };
        let timeline_item = editor.item(item.id)?;
        let progress =
            timeline_item.animation_progress_at_time(TimelineTime::from_frame(editor.playhead()));
        let frame_rate = editor.frame_rate();
        let frames_per_second = frame_rate.frames_per_second();
        let animation_start_frame = timeline_item.animation_timeline_frame(0.);
        let animation_span_frames = timeline_item.animation_span_frames();
        let start_seconds = (animation_start_frame / frames_per_second) as f32;
        let duration_seconds = (animation_span_frames / frames_per_second) as f32;
        Some(SelectedCurve {
            target,
            presentation,
            animation,
            axis_suffix,
            background_curves,
            playhead_progress: progress,
            clip_start: timeline_item.start,
            clip_duration: timeline_item.duration,
            animation_start_frame,
            animation_span_frames,
            visible_progress_range: [0., 1.],
            start_seconds,
            duration_seconds,
            frame_rate,
        })
    }

    pub(super) fn curve_point_position(
        curve: &AnimationCurve,
        point: CurvePoint,
    ) -> Option<[f32; 2]> {
        let anchor = curve.anchors().get(match point {
            CurvePoint::Anchor(index)
            | CurvePoint::HandleIn(index)
            | CurvePoint::HandleOut(index) => index,
        })?;
        let index = match point {
            CurvePoint::Anchor(index)
            | CurvePoint::HandleIn(index)
            | CurvePoint::HandleOut(index) => index,
        };
        Some(match point {
            CurvePoint::Anchor(_) => *anchor,
            CurvePoint::HandleIn(_) => curve.control(index, BezierHandle::In)?,
            CurvePoint::HandleOut(_) => curve.control(index, BezierHandle::Out)?,
        })
    }

    pub(super) fn format_number(value: impl Into<f64>) -> String {
        let value = value.into();
        let mut value = format!("{value:.4}");
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

    pub(super) fn actual_value(animation: &GraphAnimation, normalized: f32) -> f64 {
        animation.from + (animation.to - animation.from) * f64::from(normalized)
    }

    pub(super) fn normalized_value(animation: &GraphAnimation, actual: f64) -> Option<f32> {
        let range = animation.to - animation.from;
        (range.abs() > f64::EPSILON).then_some(((actual - animation.from) / range) as f32)
    }

    pub(super) fn snap_to_step(value: f64, step: f64) -> f64 {
        (value / step).round() * step
    }

    pub(super) fn drag_sensitivity(min: f64, max: f64, step: f64) -> f64 {
        ((max - min).abs() / 200.).clamp(step * 0.1, step * 2.)
    }

    pub(super) fn set_input_value(
        input: &Entity<InputState>,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if input.read(cx).value().as_ref() != value {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
    }

    pub(super) fn sync_point_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.syncing_point_inputs {
            return;
        }
        let value = self.selected_point.and_then(|point| {
            let selected = self.selected_curve(cx)?;
            let position = Self::curve_point_position(&selected.animation.curve, point)?;
            Some(Self::format_number(Self::actual_value(
                &selected.animation,
                position[1],
            )))
        });
        self.syncing_point_inputs = true;
        Self::set_input_value(
            &self.point_value_input,
            value.unwrap_or_default(),
            window,
            cx,
        );
        self.syncing_point_inputs = false;
    }

    pub(super) fn select_point(
        &mut self,
        point: CurvePoint,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        self.selected_point = Some(point);
        self.selected_segment = None;
        self.sync_point_inputs(window, cx);
        cx.notify();
    }

    pub(super) fn clear_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let point_changed = self.selected_point.take().is_some();
        let segment_changed = self.selected_segment.take().is_some();
        if point_changed || segment_changed {
            self.sync_point_inputs(window, cx);
            cx.notify();
        }
    }

    pub(super) fn update_selected_point(
        &mut self,
        position: [f32; 2],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(point) = self.selected_point else {
            return;
        };
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let target = selected.target;
        self.editor.update(cx, |editor, cx| {
            let changed = match point {
                CurvePoint::Anchor(index) => editor.set_selected_animation_anchor(
                    target.effect_id,
                    &target.parameter_id,
                    target.address,
                    index,
                    position,
                ),
                CurvePoint::HandleIn(index) => editor.set_selected_animation_handle(
                    target.effect_id,
                    &target.parameter_id,
                    target.address,
                    index,
                    BezierHandle::In,
                    position,
                ),
                CurvePoint::HandleOut(index) => editor.set_selected_animation_handle(
                    target.effect_id,
                    &target.parameter_id,
                    target.address,
                    index,
                    BezierHandle::Out,
                    position,
                ),
            };
            if changed {
                cx.notify();
            }
        });
        self.sync_point_inputs(window, cx);
    }

    pub(super) fn handle_point_input_change(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.syncing_point_inputs || !matches!(event, InputEvent::Change) {
            return;
        }
        let Ok(value) = input.read(cx).value().parse::<f64>() else {
            return;
        };
        let Some(point) = self.selected_point else {
            return;
        };
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let value = Self::snap_to_step(value, selected.presentation.step).clamp(
            selected.animation.from.min(selected.animation.to),
            selected.animation.from.max(selected.animation.to),
        );
        let Some(mut position) = Self::curve_point_position(&selected.animation.curve, point)
        else {
            return;
        };
        let Some(normalized) = Self::normalized_value(&selected.animation, value) else {
            return;
        };
        position[1] = normalized;
        self.update_selected_point(position, window, cx);
    }

    pub(super) fn handle_point_input_step(
        &mut self,
        input: &Entity<InputState>,
        event: &NumberInputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let current = input.read(cx).value().parse::<f64>().unwrap_or_default();
        let base_step = selected.presentation.step;
        let fine = match event {
            NumberInputEvent::Step { fine, .. } => *fine,
        };
        let step = if fine { base_step * 0.1 } else { base_step };
        let value = match event {
            NumberInputEvent::Step {
                action: StepAction::Increment,
                ..
            } => current + step,
            NumberInputEvent::Step {
                action: StepAction::Decrement,
                ..
            } => current - step,
        };
        self.syncing_point_inputs = true;
        input.update(cx, |input, cx| {
            input.set_value(Self::format_number(value), window, cx)
        });
        self.syncing_point_inputs = false;
        self.handle_point_input_change(input, &InputEvent::Change, window, cx);
    }

    pub(super) fn prepare_point_value_drag(
        &mut self,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        if (selected.animation.to - selected.animation.from).abs() <= f64::EPSILON {
            return;
        }
        let min = selected.animation.from.min(selected.animation.to);
        let max = selected.animation.from.max(selected.animation.to);
        let start_value = self
            .point_value_input
            .read(cx)
            .value()
            .parse::<f64>()
            .unwrap_or(min);
        self.value_drag_origin = Some(CurveValueDragOrigin {
            start_x: f32::from(event.position.x),
            start_value,
            min,
            max,
            step: selected.presentation.step,
            sensitivity: Self::drag_sensitivity(min, max, selected.presentation.step),
        });
    }

    pub(super) fn handle_point_value_drag(
        &mut self,
        drag: &CurveValueDrag,
        pointer_x: f32,
        fine_adjustment: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if drag.editor_id != cx.entity_id() {
            return;
        }
        let Some(origin) = self.value_drag_origin else {
            return;
        };
        let sensitivity = origin.sensitivity * if fine_adjustment { 0.1 } else { 1. };
        let value = Self::snap_to_step(
            origin.start_value + f64::from(pointer_x - origin.start_x) * sensitivity,
            origin.step,
        )
        .clamp(origin.min, origin.max);

        self.syncing_point_inputs = true;
        Self::set_input_value(
            &self.point_value_input,
            Self::format_number(value),
            window,
            cx,
        );
        self.syncing_point_inputs = false;
        let input = self.point_value_input.clone();
        self.handle_point_input_change(&input, &InputEvent::Change, window, cx);
    }

    pub(super) fn finish_history_drag(&mut self, cx: &mut Context<Self>) {
        self.value_drag_origin = None;
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
    }
}
