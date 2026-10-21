use ::ui::color_picker::ColorPickerState;
use ::ui::input::InputState;
use gpui::{AppContext as _, Context, Entity, Hsla, SharedString, Subscription, Window};
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
};

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

/// Model synchronization and unfinished drafts are independent from commit timing.
#[derive(Clone, Copy)]
pub(super) enum TextSync {
    Model,
    PreserveDraft,
}

pub(super) struct TextOptions {
    pub multiline: Option<(usize, usize)>,
    pub placeholder: String,
    pub sync: TextSync,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            multiline: None,
            placeholder: String::new(),
            sync: TextSync::Model,
        }
    }
}

pub(super) struct ColorControl {
    pub picker: Entity<ColorPickerState>,
    _subscription: Subscription,
}

/// Stable keyed controls shared by the property inspector and scene settings.
pub(super) struct InputStore<K> {
    pub text_inputs: HashMap<K, InputControl>,
    pub color_pickers: HashMap<K, ColorControl>,
    pub drafts: HashSet<K>,
}

impl<K> Default for InputStore<K> {
    fn default() -> Self {
        Self {
            text_inputs: HashMap::new(),
            color_pickers: HashMap::new(),
            drafts: HashSet::new(),
        }
    }
}

impl<K: Eq + Hash> InputStore<K> {
    pub(super) fn text(&self, key: &K) -> Option<Entity<InputState>> {
        self.text_inputs.get(key).map(|state| state.input.clone())
    }

    pub(super) fn color(&self, key: &K) -> Option<Entity<ColorPickerState>> {
        self.color_pickers
            .get(key)
            .map(|state| state.picker.clone())
    }

    pub(super) fn retain(&mut self, keys: &HashSet<K>) {
        self.text_inputs.retain(|key, _| keys.contains(key));
        self.color_pickers.retain(|key, _| keys.contains(key));
        self.drafts.retain(|key| keys.contains(key));
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn ensure_text<T: 'static>(
        &mut self,
        key: K,
        value: String,
        options: TextOptions,
        subscribe: impl FnOnce(&Entity<InputState>, &mut Window, &mut Context<T>) -> Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> Entity<InputState> {
        if let Some(input) = self.text(&key) {
            if !matches!(options.sync, TextSync::PreserveDraft) || !self.drafts.contains(&key) {
                set_input_text(&input, value, window, cx);
            }
            return input;
        }
        let input = cx.new(|cx| {
            let input = InputState::new(window, cx);
            let input = if let Some((min, max)) = options.multiline {
                input.auto_grow(min, max)
            } else {
                input
            };
            input
                .placeholder(options.placeholder)
                .default_value(SharedString::from(value))
        });
        let subscriptions = subscribe(&input, window, cx);
        self.text_inputs
            .insert(key, InputControl::new(input.clone(), subscriptions));
        input
    }

    pub(super) fn ensure_color<T: 'static>(
        &mut self,
        key: K,
        value: Hsla,
        subscribe: impl FnOnce(&Entity<ColorPickerState>, &mut Window, &mut Context<T>) -> Subscription,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> Entity<ColorPickerState> {
        if let Some(picker) = self.color(&key) {
            if picker.read(cx).value() != Some(value) {
                picker.update(cx, |picker, cx| picker.set_value(value, window, cx));
            }
            return picker;
        }
        let picker = cx.new(|cx| ColorPickerState::new(window, cx).default_value(value));
        let subscription = subscribe(&picker, window, cx);
        self.color_pickers.insert(
            key,
            ColorControl {
                picker: picker.clone(),
                _subscription: subscription,
            },
        );
        picker
    }
}

pub(super) fn color_to_hsla(color: [f32; 4]) -> Hsla {
    gpui::Rgba {
        r: color[0],
        g: color[1],
        b: color[2],
        a: color[3],
    }
    .into()
}
