use super::{
    NumberValueDrag, number_input_drag,
    value::{NumericDrag, NumericInput},
};
use crate::ui::components::input::set_input_text;
use ::ui::{
    Sizable as _,
    input::{InputState, NumberInput, NumberInputEvent, StepAction},
};
use gpui::{
    App, Context, DragMoveEvent, Entity, MouseButton, Render, SharedString, Subscription, Window,
    div, prelude::*,
};

/// Edits a local input value without applying it to a project or property.
/// Dialogs can keep their usual Apply/Cancel behavior while sharing numeric gestures.
pub(crate) struct SettingsNumberInput {
    input: Entity<InputState>,
    number: NumericInput,
    step: f64,
    suffix: String,
    drag: Option<NumericDrag>,
    _subscription: Subscription,
}

impl SettingsNumberInput {
    pub(crate) fn new(
        value: impl Into<SharedString>,
        number: NumericInput,
        step: f64,
        suffix: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, input, event: &NumberInputEvent, window, cx| {
                let Some(value) = this.number.parse_number(&input.read(cx).value()) else {
                    return;
                };
                let NumberInputEvent::Step { action, fine } = event;
                let step = this.number.step(this.step * if *fine { 0.1 } else { 1. });
                let delta = if *action == StepAction::Increment {
                    step
                } else {
                    -step
                };
                this.set_number(value + delta, window, cx);
            },
        );
        Self {
            input,
            number,
            step,
            suffix: suffix.to_owned(),
            drag: None,
            _subscription: subscription,
        }
    }

    pub(crate) fn value(&self, cx: &App) -> SharedString {
        self.input.read(cx).value()
    }

    fn set_number(&mut self, value: f64, window: &mut Window, cx: &mut Context<Self>) {
        let (min, max) = self.number.bounds();
        let Some(text) = self
            .number
            .value_from_number(value.clamp(min, max))
            .and_then(|value| value.numeric_text())
        else {
            return;
        };
        set_input_text(&self.input, text, window, cx);
    }

    fn move_number(
        &mut self,
        event: &DragMoveEvent<NumberValueDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = event.drag(cx);
        if target.owner_id != cx.entity_id() {
            return;
        }
        if event.event.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            return;
        }
        let Some(drag) = &self.drag else {
            return;
        };
        let value = drag.value_at(
            f32::from(event.event.position.x),
            event.event.modifiers.shift,
        );
        cx.set_active_drag_cursor_style(gpui::CursorStyle::ResizeLeftRight, window);
        self.set_number(value, window, cx);
    }
}

impl Render for SettingsNumberInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("settings-number-input")
            .w_full()
            .min_w_0()
            .flex()
            .on_drag_move(cx.listener(Self::move_number))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.drag = None),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.drag = None),
            )
            .child(number_input_drag(
                &cx.entity(),
                &self.input,
                NumberInput::new(&self.input)
                    .small()
                    .w_full()
                    .when(!self.suffix.is_empty(), |input| {
                        input.suffix(div().child(self.suffix.clone()))
                    }),
                false,
                |this, event, cx| {
                    let step = this.number.step(this.step);
                    this.drag =
                        this.number
                            .parse_number(&this.input.read(cx).value())
                            .map(|value| NumericDrag {
                                start_x: f32::from(event.position.x),
                                start_value: value,
                                step,
                                sensitivity: step,
                            });
                },
            ))
    }
}
