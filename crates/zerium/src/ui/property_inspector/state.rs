use super::*;
use crate::ui::property_inspector::control::{AnimationStopControl, Control, ControlTree};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct InspectorInputStructure {
    pub item_id: Option<ItemId>,
    pub effect_ids: Vec<EffectInstanceId>,
    pub array_lengths: Vec<(Option<u64>, String, usize)>,
    pub item_scene_arguments: Vec<String>,
    pub active_scene: Option<SceneId>,
    pub active_scene_arguments: Vec<String>,
}

pub(super) struct TextState {
    pub input: Entity<InputState>,
    pub _subscriptions: Vec<Subscription>,
}

pub(super) struct ColorState {
    pub picker: Entity<ColorPickerState>,
    pub _subscriptions: Vec<Subscription>,
}

#[derive(Default)]
pub(super) struct ControlStore {
    pub text_inputs: HashMap<ControlId, TextState>,
    pub color_pickers: HashMap<ControlId, ColorState>,
    pub tree: ControlTree,
    pub input_structure: Option<InspectorInputStructure>,
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
    pub(super) fn set_input_value(
        input: &Entity<InputState>,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if input.read(cx).value().as_ref() == value {
            return;
        }
        input.update(cx, |input, cx| input.set_value(value, window, cx));
    }

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
            Self::set_input_value(&state.input, initial, window, cx);
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
        let state = TextState {
            input,
            _subscriptions: subscriptions,
        };
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
                _subscriptions: vec![subscription],
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
                let change_target = target.clone();
                let change_spec = spec.clone();
                let step_target = target.clone();
                let step_spec = spec.clone();
                let step_input = input.clone();
                vec![
                    cx.subscribe_in(input, window, move |this, input, event, window, cx| {
                        this.apply_scalar_text(
                            &change_target,
                            &change_spec,
                            input,
                            event,
                            window,
                            cx,
                        );
                    }),
                    cx.subscribe_in(
                        &step_input,
                        window,
                        move |this, input, event, window, cx| {
                            this.apply_scalar_step(
                                &step_target,
                                &step_spec,
                                input,
                                event,
                                window,
                                cx,
                            );
                        },
                    ),
                ]
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
            if stop.edit.mixed {
                String::new()
            } else {
                Self::numeric_value_text(&stop.edit.value)
            },
            false,
            |input, window, cx| {
                let change_id = id.clone();
                let change_spec = spec.clone();
                let step_id = id.clone();
                let step_spec = spec.clone();
                let step_input = input.clone();
                vec![
                    cx.subscribe_in(input, window, move |this, input, event, window, cx| {
                        this.apply_animation_stop_text(
                            &change_id,
                            &change_spec,
                            input,
                            event,
                            window,
                            cx,
                        );
                    }),
                    cx.subscribe_in(
                        &step_input,
                        window,
                        move |this, input, event, window, cx| {
                            this.apply_animation_stop_step(
                                &step_id, &step_spec, input, event, window, cx,
                            );
                        },
                    ),
                ]
            },
            window,
            cx,
        );
    }

    fn ensure_text_control(
        &mut self,
        item_id: ItemId,
        control: &crate::ui::property_inspector::control::TextControl,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let binding = PropertyBinding {
            item_id,
            target: control.common.target.clone(),
        };
        self.ensure_text(
            control.common.id.clone(),
            text,
            control.multiline,
            |input, window, cx| {
                vec![
                    cx.subscribe_in(input, window, move |this, input, event, window, cx| {
                        this.apply_string_text(&binding, input, event, window, cx);
                    }),
                ]
            },
            window,
            cx,
        );
    }

    fn ensure_color_control(
        &mut self,
        item_id: ItemId,
        control: &crate::ui::property_inspector::control::LeafControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let color = match control.value {
            PropertyValue::Color(color) => Self::color_to_hsla(color),
            _ => gpui::Hsla::default(),
        };
        let binding = PropertyBinding {
            item_id,
            target: control.target.clone(),
        };
        self.ensure_color(
            control.id.clone(),
            color,
            |picker, window, cx| {
                cx.subscribe_in(picker, window, move |this, _, event, _, cx| {
                    this.apply_color(&binding, event, cx);
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
        item: &TimelineItem,
        control: &Control,
        seen: &mut HashSet<ControlId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        debug_assert!(seen.insert(control.id().clone()), "duplicate control id");
        match control {
            Control::File(_) => {}
            Control::Group { children, .. } => {
                for child in children {
                    self.ensure_control_states(item, child, seen, window, cx);
                }
            }
            Control::Number(number) => {
                let text = if number.common.mixed {
                    String::new()
                } else {
                    Self::numeric_value_text(&number.common.value)
                };
                self.ensure_number_text(number, text, window, cx);
            }
            Control::Text(text_control) => {
                let text = match &text_control.common.value {
                    PropertyValue::String(value) => value.clone(),
                    _ => String::new(),
                };
                self.ensure_text_control(item.id, text_control, text, window, cx);
            }
            Control::Color(color) => {
                self.ensure_color_control(item.id, color, window, cx);
            }
            Control::Bool(_) | Control::Choice(_) => {}
        }
    }

    fn ensure_tree_states(
        &mut self,
        item: &TimelineItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut seen = HashSet::new();
        let roots = self.store.tree.roots.clone();
        for control in &roots {
            self.ensure_control_states(item, control, &mut seen, window, cx);
        }
    }

    fn current_input_structure(
        editor: &TimelineEditor,
        item: Option<&TimelineItem>,
    ) -> InspectorInputStructure {
        let mut array_lengths = Vec::new();
        if let Some(item) = item {
            array_lengths.extend(item.properties.iter().filter_map(|(property_id, value)| {
                let PropertyValue::Array(values) = value else {
                    return None;
                };
                Some((None, property_id.to_owned(), values.len()))
            }));
            for effect in &item.effects {
                array_lengths.extend(effect.properties.iter().filter_map(
                    |(property_id, value)| {
                        let PropertyValue::Array(values) = value else {
                            return None;
                        };
                        Some((Some(effect.id.get()), property_id.to_owned(), values.len()))
                    },
                ));
            }
        }
        array_lengths.sort_unstable();
        let item_scene_arguments = item
            .and_then(|item| item.scene_id())
            .and_then(|scene_id| editor.scene(scene_id))
            .map(|scene| {
                scene
                    .arguments
                    .iter()
                    .map(|argument| argument.schema.id().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        let active_scene = editor.active_scene_id();
        let active_scene_arguments = active_scene
            .and_then(|scene_id| editor.scene(scene_id))
            .map(|scene| {
                scene
                    .arguments
                    .iter()
                    .map(|argument| argument.schema.id().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        InspectorInputStructure {
            item_id: item.map(|item| item.id),
            effect_ids: item
                .into_iter()
                .flat_map(|item| item.effects.iter().map(|effect| effect.id))
                .collect(),
            array_lengths,
            item_scene_arguments,
            active_scene,
            active_scene_arguments,
        }
    }

    fn resolved_control_tree(
        &self,
        editor: &TimelineEditor,
        selected_items: &[TimelineItem],
        scene_arguments: &[SceneArgumentOption],
        editing_scene: bool,
    ) -> ControlTree {
        let Some(item) = selected_items.first() else {
            return ControlTree::default();
        };
        let multiple = selected_items.len() > 1;
        let resolution = control::ControlResolution {
            editor,
            item,
            selected_items,
            playhead: zerium_core::timeline::TimelineTime::from_frame(editor.playhead()),
            editing_scene,
            arguments: scene_arguments,
        };
        let mut item_controls = Self::item_controls(editor, item, &resolution);
        item_controls.retain(|control| {
            Self::property_is_common(editor, selected_items, control.property_id())
        });

        let effects = if multiple {
            Self::common_effects(selected_items)
        } else {
            item.effects.clone()
        };
        let effect_groups: Vec<Control> = effects
            .into_iter()
            .map(|effect| {
                let controls = Self::effect_controls(&effect, &resolution);
                Control::Group {
                    id: ControlId::effect_group(effect.id),
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

    pub(super) fn sync_from_editor(
        &mut self,
        editor: &Entity<TimelineEditor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        {
            let editor = editor.read(cx);
            let stale = matches!(self.scope, EditScope::Item(id)
                if !editor.is_item_selected(id)
                    && !(editor.selected_item_ids().next().is_none() && editor.selection_remembers_item(id)));
            if stale {
                self.scope = EditScope::Selection;
                self.effect_picker = None;
                self.reset_input_state();
            }
        }
        let selected_items = {
            let editor = editor.read(cx);
            let time = zerium_core::timeline::TimelineTime::from_frame(editor.playhead());
            editor
                .items_in_scope(self.scope)
                .into_iter()
                .map(|item| editor.evaluated_item_at(&item, time))
                .collect::<Vec<_>>()
        };
        let selected_item = selected_items.first().cloned();
        let input_structure = {
            let editor = editor.read(cx);
            Self::current_input_structure(editor, selected_item.as_ref())
        };
        if self.store.input_structure.as_ref() != Some(&input_structure) {
            self.reset_input_state();
            self.store.input_structure = Some(input_structure);
        }
        let scene_arguments = self.active_scene_argument_options(&*cx);
        let editing_scene =
            self.editor.read(cx).active_scene_id().is_some() && selected_items.len() == 1;
        self.store.tree = {
            let editor = editor.read(cx);
            self.resolved_control_tree(editor, &selected_items, &scene_arguments, editing_scene)
        };
        if let Some(item) = selected_item.as_ref() {
            self.ensure_tree_states(item, window, cx);
        }
        cx.notify();
    }
}
