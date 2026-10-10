use super::*;
use crate::ui::property_inspector::control::{AnimationStopControl, Control, ControlTree};
use crate::ui::{
    input::{InputControl, set_input_text},
    number_input::{NumberEdit, subscribe_number_input},
};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct InspectorSource {
    pub item_id: ItemId,
    pub properties: Vec<PropertySchema>,
}

pub(super) struct ColorState {
    pub picker: Entity<ColorPickerState>,
    pub _subscription: Subscription,
}

#[derive(Default)]
pub(super) struct ControlStore {
    pub text_inputs: HashMap<ControlId, InputControl>,
    pub color_pickers: HashMap<ControlId, ColorState>,
    pub tree: ControlTree,
    pub source: Option<InspectorSource>,
    pub number_drag_origin: Option<Rc<PropertyValueDragOrigin>>,
}

impl ControlStore {
    pub(super) fn text(&self, id: &ControlId) -> Option<Entity<InputState>> {
        self.text_inputs.get(id).map(|state| state.input.clone())
    }

    pub(super) fn color(&self, id: &ControlId) -> Option<Entity<ColorPickerState>> {
        self.color_pickers.get(id).map(|state| state.picker.clone())
    }
}

impl PropertyInspector {
    /// Get-or-create an input state for a control, syncing its displayed text.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn ensure_text(
        &mut self,
        key: ControlId,
        initial: String,
        multiline: bool,
        subscribe: impl FnOnce(
            &Entity<InputState>,
            &mut Window,
            &mut Context<Self>,
        ) -> Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(state) = self.store.text_inputs.get(&key) {
            set_input_text(&state.input, initial, window, cx);
            return state.input.clone();
        }
        let input = cx.new(|cx| {
            let input = InputState::new(window, cx);
            let input = if multiline {
                input.auto_grow(2, 6)
            } else {
                input
            };
            input.default_value(SharedString::from(initial))
        });
        let subscriptions = subscribe(&input, window, cx);
        let cloned = input.clone();
        let state = InputControl::new(input, subscriptions);
        self.store.text_inputs.insert(key, state);
        cloned
    }

    pub(super) fn ensure_color(
        &mut self,
        key: ControlId,
        initial: gpui::Hsla,
        subscribe: impl FnOnce(
            &Entity<ColorPickerState>,
            &mut Window,
            &mut Context<Self>,
        ) -> Subscription,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ColorPickerState> {
        if let Some(state) = self.store.color_pickers.get(&key) {
            if state.picker.read(cx).value() != Some(initial) {
                state
                    .picker
                    .update(cx, |picker, cx| picker.set_value(initial, window, cx));
            }
            return state.picker.clone();
        }
        let picker = cx.new(|cx| ColorPickerState::new(window, cx).default_value(initial));
        let subscription = subscribe(&picker, window, cx);
        let cloned = picker.clone();
        self.store.color_pickers.insert(
            key,
            ColorState {
                picker,
                _subscription: subscription,
            },
        );
        cloned
    }

    fn ensure_number_text(
        &mut self,
        control: &crate::ui::property_inspector::control::NumberControl,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let spec = control.spec.clone();
        let target = control.common.target.clone();
        self.ensure_text(
            control.common.id.clone(),
            text,
            false,
            |input, window, cx| {
                subscribe_number_input(
                    input,
                    NumericInput::new(spec.scalar_type.clone()).expect("numeric control type"),
                    window,
                    cx,
                    move |this, input, edit, window, cx| match edit {
                        NumberEdit::Value(value) => {
                            this.update_numeric_scalar(
                                &target,
                                value.clamp(spec.min, spec.max),
                                cx,
                            );
                        }
                        NumberEdit::Step(event) => {
                            this.apply_scalar_step(&target, &spec, input, event, window, cx)
                        }
                        NumberEdit::Commit => {}
                    },
                )
            },
            window,
            cx,
        );
        for stop in &control.common.animation_stops {
            self.ensure_number_stop(control, stop, window, cx);
        }
    }

    fn ensure_number_stop(
        &mut self,
        control: &crate::ui::property_inspector::control::NumberControl,
        stop: &AnimationStopControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if stop.edit.value.numeric_scalar().is_none() {
            return;
        }
        let spec = control.spec.clone();
        let id = stop.id.clone();
        self.ensure_text(
            stop.id.clone(),
            Self::numeric_value_text(&stop.edit.value),
            false,
            |input, window, cx| {
                subscribe_number_input(
                    input,
                    NumericInput::new(spec.scalar_type.clone()).expect("numeric control type"),
                    window,
                    cx,
                    move |this, input, edit, window, cx| match edit {
                        NumberEdit::Value(value) => {
                            this.update_animation_stop_numeric(
                                &id,
                                value.clamp(spec.min, spec.max),
                                cx,
                            );
                        }
                        NumberEdit::Step(event) => {
                            this.apply_animation_stop_step(&id, &spec, input, event, window, cx)
                        }
                        NumberEdit::Commit => {}
                    },
                )
            },
            window,
            cx,
        );
    }

    fn ensure_text_control(
        &mut self,
        control: &crate::ui::property_inspector::control::TextControl,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = control.common.target.clone();
        self.ensure_text(
            control.common.id.clone(),
            text,
            control.multiline,
            |input, window, cx| {
                vec![
                    cx.subscribe_in(input, window, move |this, input, event, window, cx| {
                        this.apply_string_text(&target, input, event, window, cx);
                    }),
                ]
            },
            window,
            cx,
        );
    }

    fn ensure_color_control(
        &mut self,
        control: &crate::ui::property_inspector::control::LeafControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let color = match control.value {
            PropertyValue::Color(color) => Self::color_to_hsla(color),
            _ => gpui::Hsla::default(),
        };
        let target = control.target.clone();
        self.ensure_color(
            control.id.clone(),
            color,
            |picker, window, cx| {
                cx.subscribe_in(picker, window, move |this, _, event, _, cx| {
                    this.apply_color(&target, event, cx);
                })
            },
            window,
            cx,
        );
        for stop in &control.animation_stops {
            let PropertyValue::Color(color) = stop.edit.value else {
                continue;
            };
            let id = stop.id.clone();
            self.ensure_color(
                stop.id.clone(),
                Self::color_to_hsla(color),
                |picker, window, cx| {
                    cx.subscribe_in(picker, window, move |this, _, event, _, cx| {
                        this.apply_animation_stop_color(&id, event, cx);
                    })
                },
                window,
                cx,
            );
        }
    }

    fn ensure_control_states(
        &mut self,
        control: &Control,
        seen: &mut HashSet<ControlId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let unique = seen.insert(control.id().clone());
        debug_assert!(unique, "duplicate control id");
        if let Some(common) = control.common() {
            seen.extend(common.animation_stops.iter().map(|stop| stop.id.clone()));
        }
        match control {
            Control::File(_) => {}
            Control::Group { children, .. } => {
                for child in children {
                    self.ensure_control_states(child, seen, window, cx);
                }
            }
            Control::Number(number) => {
                let text = Self::numeric_value_text(&number.common.value);
                self.ensure_number_text(number, text, window, cx);
            }
            Control::Text(text_control) => {
                let text = match &text_control.common.value {
                    PropertyValue::String(value) => value.clone(),
                    _ => String::new(),
                };
                self.ensure_text_control(text_control, text, window, cx);
            }
            Control::Color(color) => {
                self.ensure_color_control(color, window, cx);
            }
            Control::Bool(_) | Control::Choice(_) => {}
        }
    }

    fn ensure_tree_states(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut seen = HashSet::new();
        let roots = self.store.tree.roots.clone();
        for control in &roots {
            self.ensure_control_states(control, &mut seen, window, cx);
        }
        self.store.text_inputs.retain(|id, _| seen.contains(id));
        self.store.color_pickers.retain(|id, _| seen.contains(id));
    }

    fn resolved_control_tree(
        editor: &TimelineEditor,
        item: &TimelineItem,
        scene_arguments: &[SceneArgumentOption],
    ) -> ControlTree {
        let resolution = control::ControlResolution {
            editor,
            item,
            playhead: zerium_core::timeline::TimelineTime::from_frame(editor.playhead()),
            editing_scene: editor.active_scene_id().is_some(),
            arguments: scene_arguments,
        };
        let item_controls = Self::property_controls(None, &resolution);
        let effect_groups: Vec<Control> = item
            .effects
            .iter()
            .map(|effect| {
                let controls = Self::property_controls(Some(effect.id), &resolution);
                Control::Group {
                    id: ControlId::EffectGroup(effect.id),
                    label: effect.schema().label().to_owned(),
                    children: controls,
                    kind: control::GroupKind::Effect(control::EffectGroup {
                        id: effect.id,
                        label: effect.schema().label().to_owned(),
                        hidden: editor.is_effect_hidden(effect.id),
                    }),
                }
            })
            .collect();

        let roots = item_controls.into_iter().chain(effect_groups).collect();
        ControlTree { roots }
    }

    pub(super) fn reset_input_state(&mut self) {
        self.store = ControlStore::default();
    }

    pub(super) fn sync_from_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        {
            let editor = self.editor.read(cx);
            if self.item_id.is_none_or(|id| {
                !editor.is_item_selected(id)
                    && !(editor.selected_item_ids().next().is_none()
                        && editor.selection_remembers_item(id))
            }) {
                self.item_id = editor.selected_item_ids().next();
                self.effect_picker = None;
                self.reset_input_state();
            }
        }
        let selected_item = {
            let editor = self.editor.read(cx);
            self.item_id
                .filter(|id| editor.is_item_selected(*id))
                .and_then(|id| editor.item(id))
                .map(|item| {
                    editor.evaluated_item_at(item, TimelineTime::from_frame(editor.playhead()))
                })
        };
        let source = selected_item.as_ref().map(|item| InspectorSource {
            item_id: item.id,
            properties: self
                .editor
                .read(cx)
                .property_schemas(item, None)
                .cloned()
                .collect(),
        });
        if self.store.source != source {
            self.reset_input_state();
            self.store.source = source;
        }
        let scene_arguments = self.active_scene_argument_options(&*cx);
        self.store.tree = selected_item
            .as_ref()
            .map(|item| {
                let editor = self.editor.read(cx);
                Self::resolved_control_tree(editor, item, &scene_arguments)
            })
            .unwrap_or_default();
        if selected_item.is_some() {
            self.ensure_tree_states(window, cx);
        } else {
            self.reset_input_state();
        }
        cx.notify();
    }
}
