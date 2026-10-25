pub(crate) mod file;
pub(in crate::ui) mod number;
pub(in crate::ui) use ::ui::color_picker::ColorPickerState;
use ::ui::input::InputState;
use gpui::{AppContext as _, Context, Entity, Hsla, SharedString, Subscription, Window};
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
};

/// Owns an input and the subscriptions that connect it to its editor.
pub(in crate::ui) struct InputControl {
    pub(in crate::ui) input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl InputControl {
    pub(in crate::ui) fn new(input: Entity<InputState>, subscriptions: Vec<Subscription>) -> Self {
        Self {
            input,
            _subscriptions: subscriptions,
        }
    }
}

pub(in crate::ui) fn set_input_text<T>(
    input: &Entity<InputState>,
    text: String,
    window: &mut Window,
    cx: &mut Context<T>,
) {
    input.update(cx, |input, cx| input.set_value_silent(text, window, cx));
}

/// Model synchronization and unfinished drafts are independent from commit timing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum TextSync {
    Model,
    PreserveDraft,
}

#[derive(PartialEq, Eq)]
pub(in crate::ui) struct TextOptions {
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

enum StoredInput {
    Text {
        control: InputControl,
        options: TextOptions,
        draft: bool,
    },
    Color {
        picker: Entity<ColorPickerState>,
        _subscription: Subscription,
    },
}

/// Stable keyed controls shared by the property inspector and scene settings.
pub(in crate::ui) struct InputStore<K> {
    controls: HashMap<K, StoredInput>,
}

impl<K> Default for InputStore<K> {
    fn default() -> Self {
        Self {
            controls: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash> InputStore<K> {
    pub(in crate::ui) fn text(&self, key: &K) -> Option<Entity<InputState>> {
        match self.controls.get(key)? {
            StoredInput::Text { control, .. } => Some(control.input.clone()),
            StoredInput::Color { .. } => None,
        }
    }

    pub(in crate::ui) fn color(&self, key: &K) -> Option<Entity<ColorPickerState>> {
        match self.controls.get(key)? {
            StoredInput::Color { picker, .. } => Some(picker.clone()),
            StoredInput::Text { .. } => None,
        }
    }

    pub(in crate::ui) fn set_draft(&mut self, key: &K, pending: bool) {
        if let Some(StoredInput::Text { draft, .. }) = self.controls.get_mut(key) {
            *draft = pending;
        }
    }

    pub(in crate::ui) fn retain(&mut self, keys: &HashSet<K>) {
        self.controls.retain(|key, _| keys.contains(key));
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::ui) fn ensure_text<T: 'static>(
        &mut self,
        key: K,
        value: String,
        options: TextOptions,
        subscribe: impl FnOnce(&Entity<InputState>, &mut Window, &mut Context<T>) -> Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> Entity<InputState> {
        if let Some(StoredInput::Text {
            control,
            options: current,
            draft,
        }) = self.controls.get(&key)
            && *current == options
        {
            if options.sync != TextSync::PreserveDraft || !draft {
                set_input_text(&control.input, value, window, cx);
            }
            return control.input.clone();
        }
        let input = cx.new(|cx| {
            let input = InputState::new(window, cx);
            let input = if let Some((min, max)) = options.multiline {
                input.auto_grow(min, max)
            } else {
                input
            };
            input
                .placeholder(options.placeholder.clone())
                .default_value(SharedString::from(value))
        });
        let subscriptions = subscribe(&input, window, cx);
        self.controls.insert(
            key,
            StoredInput::Text {
                control: InputControl::new(input.clone(), subscriptions),
                options,
                draft: false,
            },
        );
        input
    }

    pub(in crate::ui) fn ensure_color<T: 'static>(
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
        self.controls.insert(
            key,
            StoredInput::Color {
                picker: picker.clone(),
                _subscription: subscription,
            },
        );
        picker
    }
}

pub(in crate::ui) fn color_to_hsla(color: [f32; 4]) -> Hsla {
    gpui::Rgba {
        r: color[0],
        g: color[1],
        b: color[2],
        a: color[3],
    }
    .into()
}
