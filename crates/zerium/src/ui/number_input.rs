use ::ui::input::{InputState, NumberInput};
use gpui::{
    Context, Empty, Entity, EntityId, MouseButton, MouseDownEvent, Render, Window, div, prelude::*,
};

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
