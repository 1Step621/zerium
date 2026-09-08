use super::*;

impl AnimationCurveEditor {
    pub(super) fn add_anchor(
        &mut self,
        position: gpui::Point<Pixels>,
        snap_disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(mut position) = self.normalized_position(position) else {
            return;
        };
        if !snap_disabled {
            position[0] = self.snapped_anchor_progress(position[0], None, &selected, cx);
        }
        if !(selected.visible_progress_range[0]..=selected.visible_progress_range[1])
            .contains(&position[0])
        {
            return;
        }
        let target = selected.target;
        let selected = self.editor.update(cx, |editor, cx| {
            let selected = editor.add_selected_animation_anchor(
                target.effect_id,
                &target.parameter_id,
                target.address,
                position,
            );
            if selected.is_some() {
                cx.notify();
            }
            selected
        });
        if let Some(index) = selected {
            self.selected_point = Some(CurvePoint::Anchor(index));
            self.selected_segment = None;
            self.sync_point_inputs(window, cx);
            cx.notify();
        }
    }

    pub(super) fn move_anchor(
        &mut self,
        index: usize,
        position: gpui::Point<Pixels>,
        snap_disabled: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(mut position) = self.normalized_position(position) else {
            return;
        };
        if !snap_disabled {
            position[0] = self.snapped_anchor_progress(position[0], Some(index), &selected, cx);
        }
        position[0] = position[0].clamp(
            selected.visible_progress_range[0].clamp(0., 1.),
            selected.visible_progress_range[1].clamp(0., 1.),
        );
        let target = selected.target;
        self.editor.update(cx, |editor, cx| {
            if editor.set_selected_animation_anchor(
                target.effect_id,
                &target.parameter_id,
                target.address,
                index,
                position,
            ) {
                cx.notify();
            }
        });
    }

    pub(super) fn move_point(
        &mut self,
        point: CurvePoint,
        position: gpui::Point<Pixels>,
        snap_disabled: bool,
        cx: &mut Context<Self>,
    ) {
        match point {
            CurvePoint::Anchor(index) => self.move_anchor(index, position, snap_disabled, cx),
            CurvePoint::HandleIn(index) => {
                self.move_handle(index, BezierHandle::In, position, cx);
            }
            CurvePoint::HandleOut(index) => {
                self.move_handle(index, BezierHandle::Out, position, cx);
            }
        }
    }

    pub(super) fn graph_pixels_per_second(&self, duration_seconds: f32) -> f64 {
        let plot_width = self.graph_bounds.map_or(220., |bounds| {
            f32::from(bounds.size.width - px(Self::GRAPH_INSET_LEFT + Self::GRAPH_INSET_RIGHT))
                .max(1.)
        });
        let visible_duration = self.viewport.x_span() * duration_seconds.max(f32::EPSILON);
        f64::from(plot_width / visible_duration.max(f32::EPSILON))
    }

    pub(super) fn snapped_anchor_progress(
        &self,
        progress: f32,
        moving_index: Option<usize>,
        selected: &SelectedCurve,
        cx: &Context<Self>,
    ) -> f32 {
        let duration = selected.duration_seconds.max(f32::EPSILON);
        let absolute_time = selected.start_seconds + progress * duration;
        let mut targets = vec![self.editor.read(cx).playhead_seconds()];
        targets.extend(
            selected
                .animation
                .curve
                .anchors()
                .iter()
                .enumerate()
                .filter(|(index, _)| Some(*index) != moving_index)
                .map(|(_, anchor)| f64::from(selected.start_seconds + (*anchor)[0] * duration)),
        );
        targets.extend(selected.background_curves.iter().flat_map(|curve| {
            curve
                .anchors()
                .iter()
                .map(|anchor| f64::from(selected.start_seconds + (*anchor)[0] * duration))
        }));

        let pixels_per_second = self.graph_pixels_per_second(duration);
        let offset = time_grid::snap_offset_seconds(
            &[f64::from(absolute_time)],
            &targets,
            time_grid::ruler_step(pixels_per_second),
            pixels_per_second,
        );
        ((f64::from(absolute_time) + offset - f64::from(selected.start_seconds))
            / f64::from(duration))
        .clamp(0., 1.) as f32
    }

    pub(super) fn move_handle(
        &mut self,
        index: usize,
        handle: BezierHandle,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let target = selected.target;
        let Some(position) = self.normalized_position(position) else {
            return;
        };
        self.editor.update(cx, |editor, cx| {
            if editor.set_selected_animation_handle(
                target.effect_id,
                &target.parameter_id,
                target.address,
                index,
                handle,
                position,
            ) {
                cx.notify();
            }
        });
    }

    pub(super) fn remove_anchor(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let target = selected.target;
        self.editor.update(cx, |editor, cx| {
            if editor.remove_selected_animation_anchor(
                target.effect_id,
                &target.parameter_id,
                target.address,
                index,
            ) {
                cx.notify();
            }
        });
        self.selected_point = None;
        self.selected_segment = None;
        self.sync_point_inputs(window, cx);
    }

    pub(super) fn set_interpolation(
        &mut self,
        segment: usize,
        interpolation: SegmentInterpolation,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let target = selected.target;
        self.editor.update(cx, |editor, cx| {
            if editor.set_selected_animation_interpolation(
                target.effect_id,
                &target.parameter_id,
                target.address,
                segment,
                interpolation,
            ) {
                cx.notify();
            }
        });
    }

    pub(super) fn set_custom(&mut self, segment: usize, cx: &mut Context<Self>) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let target = selected.target;
        self.editor.update(cx, |editor, cx| {
            if editor.set_selected_animation_custom(
                target.effect_id,
                &target.parameter_id,
                target.address,
                segment,
            ) {
                cx.notify();
            }
        });
    }
}
