use super::*;
use crate::ui::property_inspector::control::{AnimationStopControl, Control, ControlTree};

pub(super) struct TextState {
    pub input: Entity<InputState>,
    pub _subscriptions: Vec<Subscription>,
}

pub(super) struct ColorState {
    pub picker: Entity<ColorPickerState>,
    pub _subscriptions: Vec<Subscription>,
}

pub(super) enum ControlState {
    Text(TextState),
    Number(TextState),
    Color(ColorState),
}

impl ControlState {
    pub(super) fn text(&self) -> Option<&TextState> {
        match self {
            Self::Text(state) => Some(state),
            Self::Number(input) => Some(input),
            Self::Color(_) => None,
        }
    }

    pub(super) fn color(&self) -> Option<&ColorState> {
        match self {
            Self::Color(picker) => Some(picker),
            Self::Text(_) | Self::Number(_) => None,
        }
    }
}

#[derive(Default)]
pub(super) struct ControlStore {
    pub states: HashMap<ControlId, ControlState>,
    pub tree: ControlTree,
    pub input_structure: Option<InspectorInputStructure>,
    pub value_drag_origin: Option<PropertyValueDragOrigin>,
    pub scene_argument_value_drag_origin: Option<SceneArgumentValueDragOrigin>,
}

impl ControlStore {
    pub(super) fn text(&self, id: &ControlId) -> Option<Entity<InputState>> {
        self.states
            .get(id)
            .and_then(ControlState::text)
            .map(|state| state.input.clone())
    }

    pub(super) fn color(&self, id: &ControlId) -> Option<Entity<ColorPickerState>> {
        self.states
            .get(id)
            .and_then(ControlState::color)
            .map(|state| state.picker.clone())
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

    pub(super) fn color_value(item: &TimelineItem, target: &PropertyTarget) -> Option<[f32; 4]> {
        let value = match target.effect_id {
            Some(effect_id) => item
                .effects
                .iter()
                .find(|effect| effect.id == effect_id)
                .and_then(|effect| effect.properties.property(&target.property_id)),
            None => item.properties.property(&target.property_id),
        };
        let value = match target.path.element_id() {
            Some(id) => match value? {
                PropertyValue::Array(values) => {
                    values.iter().find(|row| row.element_id() == id)?.value()
                }
                _ => return None,
            },
            None => value?,
        }
        .scalar_at(target.path.scalar_index())?;
        match value {
            PropertyValue::Color(color) => Some(*color),
            _ => None,
        }
    }

    /// Get-or-create an input state for a control, syncing its displayed text.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn ensure_text(
        &mut self,
        key: ControlId,
        initial: String,
        multiline: bool,
        numeric: bool,
        subscribe: impl FnOnce(
            &Entity<InputState>,
            &mut Window,
            &mut Context<Self>,
        ) -> Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(state) = self.store.states.get(&key).and_then(ControlState::text) {
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
        self.store.states.insert(
            key,
            if numeric {
                ControlState::Number(state)
            } else {
                ControlState::Text(state)
            },
        );
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
        if let Some(state) = self.store.states.get(&key).and_then(ControlState::color) {
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
        self.store.states.insert(
            key,
            ControlState::Color(ColorState {
                picker,
                _subscriptions: vec![subscription],
            }),
        );
        cloned
    }

    fn ensure_number_text(
        &mut self,
        item_id: ItemId,
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
            true,
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
            self.ensure_number_stop(item_id, control, stop, window, cx);
        }
    }

    fn ensure_number_stop(
        &mut self,
        item_id: ItemId,
        control: &crate::ui::property_inspector::control::NumberControl,
        stop: &AnimationStopControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(value) = stop.value.numeric_scalar() else {
            return;
        };
        let spec = control.spec.clone();
        let binding = AnimationStopBinding::new(item_id, control.common.target.effect_id, stop);
        self.ensure_text(
            stop.id.clone(),
            Self::format_value(value),
            false,
            true,
            |input, window, cx| {
                let change_binding = binding.clone();
                let change_spec = spec.clone();
                let step_binding = binding.clone();
                let step_spec = spec.clone();
                let step_input = input.clone();
                vec![
                    cx.subscribe_in(input, window, move |this, input, event, window, cx| {
                        this.apply_animation_stop_text(
                            &change_binding,
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
                                &step_binding,
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
            false,
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
        control: &crate::ui::property_inspector::control::ColorControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let color = match control.common.value {
            PropertyValue::Color(color) => Self::color_to_hsla(color),
            _ => gpui::Hsla::default(),
        };
        let binding = PropertyBinding {
            item_id,
            target: control.common.target.clone(),
        };
        self.ensure_color(
            control.common.id.clone(),
            color,
            |picker, window, cx| {
                cx.subscribe_in(picker, window, move |this, _, event, _, cx| {
                    this.apply_color(&binding, event, cx);
                })
            },
            window,
            cx,
        );
        for stop in &control.common.animation_stops {
            let PropertyValue::Color(color) = stop.value else {
                continue;
            };
            let binding = AnimationStopBinding::new(item_id, control.common.target.effect_id, stop);
            self.ensure_color(
                stop.id.clone(),
                Self::color_to_hsla(color),
                |picker, window, cx| {
                    cx.subscribe_in(picker, window, move |this, _, event, _, cx| {
                        this.apply_animation_stop_color(&binding, event, cx);
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
            Control::Group { children, .. } => {
                for child in children {
                    self.ensure_control_states(item, child, seen, window, cx);
                }
            }
            Control::Number(number) => {
                let text = number
                    .common
                    .value
                    .numeric_scalar()
                    .map(Self::format_value)
                    .unwrap_or_default();
                self.ensure_number_text(item.id, number, text, window, cx);
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
            item,
            selected_items,
            playhead: crate::domain::timeline::TimelineTime::from_frame(editor.playhead()),
            editing_scene,
            arguments: scene_arguments,
        };
        let mut item_controls = if let Some(scene_id) = item.scene_id() {
            let values = editor
                .evaluated_scene_argument_values_at(item, resolution.playhead)
                .unwrap_or_default();
            editor
                .scene(scene_id)
                .map(|scene| {
                    Self::scene_argument_value_controls(
                        scene_id,
                        &scene.arguments,
                        &values,
                        &resolution,
                    )
                })
                .unwrap_or_default()
        } else {
            Self::item_controls(item, &resolution)
        };
        if item.scene_id().is_none() {
            item_controls
                .retain(|control| Self::property_is_common(selected_items, control.property_id()));
        }
        if multiple {
            for control in &mut item_controls {
                control.disable_animation();
            }
        }

        let effects = if multiple {
            Self::common_effects(selected_items)
        } else {
            item.effects.clone()
        };
        let effect_groups: Vec<Control> = effects
            .into_iter()
            .map(|effect| {
                let mut controls = Self::effect_controls(&effect, &resolution);
                if multiple {
                    for control in &mut controls {
                        control.disable_animation();
                    }
                }
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

    fn reconcile_states(
        &mut self,
        item: &TimelineItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_tree_states(item, window, cx);
        self.ensure_active_scene_argument_names(window, cx);
    }

    pub(super) fn sync_from_editor(
        &mut self,
        editor: &Entity<TimelineEditor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected_items = {
            let editor = editor.read(cx);
            let time = crate::domain::timeline::TimelineTime::from_frame(editor.playhead());
            editor
                .selected_items()
                .into_iter()
                .map(|item| item.evaluated_at_time(time))
                .collect::<Vec<_>>()
        };
        let selected_item = selected_items.first().cloned();
        let input_structure = {
            let editor = editor.read(cx);
            Self::current_input_structure(editor, selected_item.as_ref())
        };
        let previous_item_id = self
            .store
            .input_structure
            .as_ref()
            .and_then(|structure| structure.item_id);
        if self.store.input_structure.as_ref() != Some(&input_structure) {
            self.reset_input_state();
            self.store.input_structure = Some(input_structure);
        }
        if previous_item_id != selected_item.as_ref().map(|item| item.id) {
            self.file_error = None;
        }
        let scene_arguments = self.active_scene_argument_options(&*cx);
        let editing_scene =
            self.editor.read(cx).active_scene_id().is_some() && selected_items.len() == 1;
        self.store.tree = {
            let editor = editor.read(cx);
            self.resolved_control_tree(editor, &selected_items, &scene_arguments, editing_scene)
        };
        if let Some(item) = selected_item.as_ref() {
            self.reconcile_states(item, window, cx);
        }
        let animation_address = self.animation_selection.read(cx).address().cloned();
        let invalid_animation_address = animation_address.as_ref().is_some_and(|target| {
            let editor = editor.read(cx);
            let target_exists = editor
                .item(target.item_id)
                .and_then(|item| {
                    item.animation_track(
                        target.effect_id,
                        &target.property_id,
                        target.element_id,
                        target.scalar_index,
                    )
                })
                .is_some();
            let selected_another_item = match selected_items.as_slice() {
                [] => !editor.selection_remembers_item(target.item_id),
                [item] => item.id != target.item_id,
                _ => true,
            };
            !target_exists || selected_another_item
        });
        if invalid_animation_address {
            self.animation_selection
                .update(cx, |selection, cx| selection.clear(cx));
        }
        cx.notify();
    }
}
