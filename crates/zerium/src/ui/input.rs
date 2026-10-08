use ::ui::input::InputState;
use gpui::{Context, Entity, Subscription, Window};

/// Owns an input and the subscriptions that connect it to its editor.
pub(super) struct InputControl {
    pub(super) input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl InputControl {
    pub(super) fn new(input: Entity<InputState>, subscriptions: Vec<Subscription>) -> Self {
        Self {
            input,
            _subscriptions: subscriptions,
        }
    }
}

pub(super) fn set_input_text<T>(
    input: &Entity<InputState>,
    text: String,
    window: &mut Window,
    cx: &mut Context<T>,
) {
    input.update(cx, |input, cx| input.set_value_silent(text, window, cx));
}
