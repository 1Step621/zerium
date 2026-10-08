use super::numeric_property::NumericInput;
use ::ui::input::{InputEvent, InputState, NumberInput, NumberInputEvent};
use gpui::{
    Context, Empty, Entity, EntityId, MouseButton, MouseDownEvent, Render, Subscription, Window,
    div, prelude::*,
};
use std::rc::Rc;

pub(super) enum NumberEdit<'a> {
    Value(f64),
    Step(&'a NumberInputEvent),
    Commit,
}

/// Text parsing and input events are shared; each caller resolves its own
/// model targets, step starting value, and commit behavior.
pub(super) fn subscribe_number_input<T: 'static>(
    input: &Entity<InputState>,
    number: NumericInput,
    window: &mut Window,
    cx: &mut Context<T>,
    apply: impl Fn(&mut T, &Entity<InputState>, NumberEdit<'_>, &mut Window, &mut Context<T>) + 'static,
) -> Vec<Subscription> {
    let apply = Rc::new(apply);
    let text_apply = apply.clone();
    vec![
        cx.subscribe_in(input, window, move |this, input, event, window, cx| {
            let edit = match event {
                InputEvent::Change => {
                    let Some(value) = number.parse_number(&input.read(cx).value()) else {
                        return;
                    };
                    NumberEdit::Value(value)
                }
                InputEvent::Blur | InputEvent::PressEnter { .. } => NumberEdit::Commit,
                _ => return,
            };
            text_apply(this, input, edit, window, cx);
        }),
        cx.subscribe_in(
            input,
            window,
            move |this, input, event: &NumberInputEvent, window, cx| {
                apply(this, input, NumberEdit::Step(event), window, cx);
            },
        ),
    ]
}

#[derive(Clone)]
pub(super) struct NumberValueDrag {
    pub owner_id: EntityId,
    pub input_id: EntityId,
}

impl Render for NumberValueDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

pub(super) fn number_input_drag<T: 'static>(
    owner: &Entity<T>,
    input: &Entity<InputState>,
    value_input: NumberInput,
    disabled: bool,
    prepare: impl Fn(&mut T, &MouseDownEvent, &mut Context<T>) + 'static,
) -> gpui::AnyElement {
    let drag = NumberValueDrag {
        owner_id: owner.entity_id(),
        input_id: input.entity_id(),
    };
    let owner = owner.clone();
    let input = input.clone();
    div()
        .id(("number-value-drag", input.entity_id()))
        .w_0()
        .min_w_0()
        .flex_1()
        .flex()
        .when(!disabled, |this| {
            this.on_mouse_down(MouseButton::Left, move |event, _, cx| {
                owner.update(cx, |this, cx| prepare(this, event, cx));
            })
            .on_drag(drag, move |drag, _, window, cx| {
                cx.stop_propagation();
                input.update(cx, |input, cx| input.unselect(window, cx));
                cx.new(|_| drag.clone())
            })
        })
        .child(value_input)
        .into_any_element()
}
