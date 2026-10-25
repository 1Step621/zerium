use crate::ui::{
    TimelineEditorEntityExt as _,
    components::input::{
        InputControl,
        number::{
            NumberDragStart, NumberEdit, number_input_drag, subscribe_number_input,
            value::{NumericDrag, NumericInput},
        },
        set_input_text,
    },
    panels::animation_curve::*,
};
use ::ui::input::{InputState, NumberInput, NumberInputEvent, StepAction};
use rust_i18n::t;
use std::num::NonZeroU32;

pub(in crate::ui::panels::animation_curve) struct StepInput {
    target: SegmentIdentity,
    control: InputControl,
    model: NonZeroU32,
    edit: Option<AnimationEdit>,
}

impl StepInput {
    fn count(&self, editor: &TimelineEditor) -> Option<NonZeroU32> {
        let track = editor.animation_track(&self.target.address)?;
        let index = self.target.source_segment;
        if [
            track.stops().get(index)?.frame(),
            track.stops().get(index + 1)?.frame(),
        ] != self.target.source_frames
        {
            return None;
        }
        match track.interpolations().get(index)? {
            SegmentInterpolation::Steps { count } => Some(*count),
            _ => None,
        }
    }

    pub(in crate::ui::panels::animation_curve) fn edit_for(
        &self,
        selected: &SelectedCurve,
    ) -> Option<&AnimationEdit> {
        self.edit.as_ref().filter(|_| self.target.matches(selected))
    }

    pub(in crate::ui::panels::animation_curve) fn discard_stale_edit(
        &mut self,
        editor: &TimelineEditor,
    ) -> bool {
        self.edit
            .as_ref()
            .is_some_and(|edit| !edit.is_current(editor))
            && self.finish_edit()
    }

    pub(in crate::ui::panels::animation_curve) fn finish_edit(&mut self) -> bool {
        self.edit.take().is_some()
    }
}

impl AnimationCurveEditor {
    fn step_count_for_input(&self, input: &Entity<InputState>, cx: &App) -> Option<NonZeroU32> {
        self.step_input
            .as_ref()
            .filter(|state| state.control.input == *input)?
            .count(self.editor.read(cx))
    }

    pub(in crate::ui::panels::animation_curve) fn set_step_count(
        &mut self,
        input: &Entity<InputState>,
        value: f64,
        synchronize: bool,
        cx: &mut Context<Self>,
    ) {
        if !value.is_finite() {
            return;
        }
        let Some(current) = self.step_count_for_input(input, cx) else {
            return;
        };
        let count = NonZeroU32::new(value.round().clamp(1., f64::from(u32::MAX)) as u32)
            .expect("step count is positive");
        let Some(state) = self.step_input.as_mut() else {
            return;
        };
        if current == count {
            return;
        }
        if state.edit.is_none() {
            state.edit = self.editor.read(cx).begin_animation_edit(
                &state.target.address,
                AnimationEditTarget::Segment(state.target.source_segment),
                synchronize,
            );
        }
        let Some(edit) = &mut state.edit else {
            return;
        };
        if self.editor.update_if_changed(cx, |editor| {
            editor.set_animation_interpolation(edit, SegmentInterpolation::Steps { count })
        }) {
            state.model = count;
        }
    }

    pub(in crate::ui::panels::animation_curve) fn sync_step_text(
        &self,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(count) = self.step_count_for_input(input, cx) {
            set_input_text(input, count.to_string(), window, cx);
        }
    }

    fn ensure_step_input(
        &mut self,
        selected: &SelectedCurve,
        count: NonZeroU32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = &mut self.step_input
            && state.target.matches(selected)
        {
            if state.model != count {
                state.finish_edit();
                if self
                    .number_drag
                    .as_ref()
                    .is_some_and(|drag| drag.input == state.control.input)
                {
                    self.number_drag = None;
                }
                state.model = count;
                set_input_text(&state.control.input, count.to_string(), window, cx);
            }
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).default_value(count.to_string()));
        let subscriptions = subscribe_number_input(
            &input,
            NumericInput::U32,
            window,
            cx,
            |this, input, edit, window, cx| match edit {
                NumberEdit::Value(value) => {
                    this.set_step_count(input, value, !window.modifiers().alt, cx);
                }
                NumberEdit::Step(NumberInputEvent::Step { action, .. }) => {
                    let Some(count) = this.step_count_for_input(input, cx) else {
                        return;
                    };
                    let delta = if *action == StepAction::Increment {
                        1.
                    } else {
                        -1.
                    };
                    let value = f64::from(count.get()) + delta;
                    this.set_step_count(input, value, !window.modifiers().alt, cx);
                    this.sync_step_text(input, window, cx);
                }
                NumberEdit::Commit => {
                    let Some(_) = this
                        .step_input
                        .as_ref()
                        .filter(|state| state.control.input == *input)
                    else {
                        return;
                    };
                    // Moving focus from the text field to a numeric drag can blur it.
                    if this.number_drag.is_none() {
                        this.finish_history_drag(cx);
                    }
                    this.sync_step_text(input, window, cx);
                }
            },
        );
        self.step_input = Some(StepInput {
            target: selected.segment_identity(),
            control: InputControl::new(input, subscriptions),
            model: count,
            edit: None,
        });
    }

    fn prepare_step_drag(&mut self, event: &NumberDragStart, cx: &mut Context<Self>) {
        self.end_pointer_drag(cx);
        self.finish_history_drag(cx);
        let Some(state) = &mut self.step_input else {
            return;
        };
        let Some(count) = state.count(self.editor.read(cx)) else {
            return;
        };
        let Some(edit) = self.editor.read(cx).begin_animation_edit(
            &state.target.address,
            AnimationEditTarget::Segment(state.target.source_segment),
            !event.modifiers.alt,
        ) else {
            return;
        };
        state.edit = Some(edit);
        self.number_drag = Some(CurveNumberDrag {
            parameter: CurveNumberParameter::StepCount,
            input: state.control.input.clone(),
            motion: NumericDrag {
                start_x: f32::from(event.position.x),
                start_value: f64::from(count.get()),
                step: 1.,
                sensitivity: 1.,
            },
        });
        cx.notify();
    }

    pub(in crate::ui::panels::animation_curve) fn step_controls(
        &mut self,
        selected: &SelectedCurve,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let SegmentInterpolation::Steps { count } = selected.curve.interpolation else {
            self.step_input = None;
            return None;
        };
        self.ensure_step_input(selected, count, window, cx);
        let input = self.step_input.as_ref().unwrap().control.input.clone();
        let control = NumberInput::new(&input).small().min_w_0();
        Some(
            div()
                .id("animation-step-count")
                .flex()
                .flex_none()
                .items_center()
                .w(px(140.))
                .gap_1()
                .text_xs()
                .tooltip(|window, cx| {
                    ::ui::tooltip::Tooltip::new(t!("curve.step_count_hint").to_string())
                        .build(window, cx)
                })
                .child(t!("curve.step_count").to_string())
                .child(number_input_drag(
                    &cx.entity(),
                    &input,
                    control,
                    false,
                    Self::prepare_step_drag,
                ))
                .into_any_element(),
        )
    }
}
