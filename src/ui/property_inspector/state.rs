use super::*;
use crate::ui::property_inspector::control::{Control, ControlTree};

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
    Number {
        input: TextState,
        animation: Option<(TextState, TextState)>,
    },
    Color {
        picker: ColorState,
        animation: Option<(ColorState, ColorState)>,
    },
}

impl ControlState {
    pub(super) fn text(&self) -> Option<&TextState> {
        match self {
            Self::Text(state) => Some(state),
            Self::Number { input, .. } => Some(input),
            Self::Color { .. } => None,
        }
    }

    pub(super) fn color(&self) -> Option<&ColorState> {
        match self {
            Self::Color { picker, .. } => Some(picker),
            Self::Text(_) | Self::Number { .. } => None,
        }
    }

    pub(super) fn animation_text(&self) -> Option<&(TextState, TextState)> {
        match self {
            Self::Number { animation, .. } => animation.as_ref(),
            Self::Text(_) | Self::Color { .. } => None,
        }
    }

    pub(super) fn animation_color(&self) -> Option<&(ColorState, ColorState)> {
        match self {
            Self::Color { animation, .. } => animation.as_ref(),
            Self::Text(_) | Self::Number { .. } => None,
        }
    }

    fn insert_number_animation(&mut self, animation: (TextState, TextState)) {
        if let Self::Number {
            animation: current, ..
        } = self
        {
            *current = Some(animation);
        }
    }

    fn insert_color_animation(&mut self, animation: (ColorState, ColorState)) {
        if let Self::Color {
            animation: current, ..
        } = self
        {
            *current = Some(animation);
        }
    }

    fn clear_inactive_animation(&mut self, text_active: bool, color_active: bool) {
        match self {
            Self::Number { animation, .. } if !text_active => *animation = None,
            Self::Color { animation, .. } if !color_active => *animation = None,
            _ => {}
        }
    }
}

#[derive(Default)]
pub(super) struct ActiveAnimations {
    pub text: HashSet<ControlId>,
    pub color: HashSet<ControlId>,
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

    pub(super) fn animation_text(
        &self,
        id: &ControlId,
    ) -> Option<(Entity<InputState>, Entity<InputState>)> {
        self.states
            .get(id)
            .and_then(ControlState::animation_text)
            .map(|(from, to)| (from.input.clone(), to.input.clone()))
    }

    pub(super) fn animation_color_pair(
        &self,
        id: &ControlId,
    ) -> Option<(Entity<ColorPickerState>, Entity<ColorPickerState>)> {
        self.states
            .get(id)
            .and_then(ControlState::animation_color)
            .map(|(from, to)| (from.picker.clone(), to.picker.clone()))
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

    fn numeric_text(item: &TimelineItem, target: &PropertyTarget) -> String {
        let value = match target.effect_id {
            Some(effect_id) => item
                .effects
                .iter()
                .find(|effect| effect.id == effect_id)
                .and_then(|effect| effect.parameters.get(&target.parameter_id)),
            None => item.parameters.get(&target.parameter_id),
        };
        let Some(value) = value else {
            return String::new();
        };
        let value = match target.value_path.array_element() {
            Some(index) => match value {
                ParameterValue::Array(values) => values
                    .get(index)
                    .and_then(|value| value.scalar_at(target.value_path.tuple_element()))
                    .and_then(ParameterValue::numeric_scalar),
                _ => None,
            },
            None => value
                .scalar_at(target.value_path.tuple_element())
                .and_then(ParameterValue::numeric_scalar),
        };
        value.map(Self::format_value).unwrap_or_default()
    }

    fn color_value(item: &TimelineItem, target: &PropertyTarget) -> Option<[f32; 4]> {
        let value = match target.effect_id {
            Some(effect_id) => item
                .effects
                .iter()
                .find(|effect| effect.id == effect_id)
                .and_then(|effect| effect.parameters.get(&target.parameter_id)),
            None => item.parameters.get(&target.parameter_id),
        };
        let value = match target.value_path.array_element() {
            Some(index) => match value? {
                ParameterValue::Array(values) => values.get(index)?,
                _ => return None,
            },
            None => value?,
        }
        .scalar_at(target.value_path.tuple_element())?;
        match value {
            ParameterValue::Color(color) => Some(*color),
            _ => None,
        }
    }

    /// Get-or-create an input state for a control, syncing its displayed text.
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
                ControlState::Number {
                    input: state,
                    animation: None,
                }
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
            ControlState::Color {
                picker: ColorState {
                    picker,
                    _subscriptions: vec![subscription],
                },
                animation: None,
            },
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
        let _ = item_id;
    }

    fn ensure_text_control(
        &mut self,
        item_id: ItemId,
        control: &crate::ui::property_inspector::control::TextControl,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let binding = ParameterBinding {
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
        item: &TimelineItem,
        control: &crate::ui::property_inspector::control::ColorControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let color = Self::color_value(item, &control.common.target)
            .map(Self::color_to_hsla)
            .unwrap_or_default();
        let binding = ParameterBinding {
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
    }

    fn ensure_animation_state(
        &mut self,
        item: &TimelineItem,
        control: &Control,
        active: &mut ActiveAnimations,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match control {
            Control::Number(number) => {
                let Some(display) = number.animation.as_ref() else {
                    return;
                };
                active.text.insert(number.common.id.clone());
                let key = number.common.id.clone();
                if let Some((from, to)) = self
                    .store
                    .states
                    .get(&key)
                    .and_then(ControlState::animation_text)
                {
                    Self::set_input_value(
                        &from.input,
                        Self::format_value(display.from),
                        window,
                        cx,
                    );
                    Self::set_input_value(&to.input, Self::format_value(display.to), window, cx);
                    return;
                }
                let target = number.common.target.clone();
                let spec = number.spec.clone();
                let from = cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(SharedString::from(Self::format_value(display.from)))
                });
                let to = cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(SharedString::from(Self::format_value(display.to)))
                });
                let from_target = target.clone();
                let from_spec = spec.clone();
                let from_sub =
                    cx.subscribe_in(&from, window, move |this, input, event, window, cx| {
                        this.apply_animation_text(
                            &from_target,
                            &from_spec,
                            AnimationEndpoint::From,
                            input,
                            event,
                            window,
                            cx,
                        );
                    });
                let to_target = target.clone();
                let to_spec = spec.clone();
                let to_sub = cx.subscribe_in(&to, window, move |this, input, event, window, cx| {
                    this.apply_animation_text(
                        &to_target,
                        &to_spec,
                        AnimationEndpoint::To,
                        input,
                        event,
                        window,
                        cx,
                    );
                });
                if let Some(state) = self.store.states.get_mut(&key) {
                    state.insert_number_animation((
                        TextState {
                            input: from,
                            _subscriptions: vec![from_sub],
                        },
                        TextState {
                            input: to,
                            _subscriptions: vec![to_sub],
                        },
                    ));
                }
            }
            Control::Color(color) => {
                let Some(display) = color.animation.as_ref() else {
                    return;
                };
                active.color.insert(color.common.id.clone());
                let from_color = Self::color_to_hsla(display.from);
                let to_color = Self::color_to_hsla(display.to);
                let key = color.common.id.clone();
                if let Some((from_picker, to_picker)) = self
                    .store
                    .states
                    .get(&key)
                    .and_then(ControlState::animation_color)
                    .map(|(from, to)| (from.picker.clone(), to.picker.clone()))
                {
                    if from_picker.read(cx).value() != Some(from_color) {
                        from_picker
                            .update(cx, |picker, cx| picker.set_value(from_color, window, cx));
                    }
                    if to_picker.read(cx).value() != Some(to_color) {
                        to_picker.update(cx, |picker, cx| picker.set_value(to_color, window, cx));
                    }
                    return;
                }
                let target = color.common.target.clone();
                let mut make_picker = |color, endpoint, cx: &mut Context<Self>| {
                    let picker =
                        cx.new(|cx| ColorPickerState::new(window, cx).default_value(color));
                    let binding = ParameterBinding {
                        item_id: item.id,
                        target: target.clone(),
                    };
                    let subscription =
                        cx.subscribe_in(&picker, window, move |this, _, event, _, cx| {
                            this.apply_animation_color(&binding, endpoint, event, cx);
                        });
                    ColorState {
                        picker,
                        _subscriptions: vec![subscription],
                    }
                };
                let from_state = make_picker(from_color, AnimationEndpoint::From, cx);
                let to_state = make_picker(to_color, AnimationEndpoint::To, cx);
                if let Some(state) = self.store.states.get_mut(&key) {
                    state.insert_color_animation((from_state, to_state));
                }
            }
            _ => {}
        }
    }

    fn ensure_control_states(
        &mut self,
        item: &TimelineItem,
        control: &Control,
        active: &mut ActiveAnimations,
        seen: &mut HashSet<ControlId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        debug_assert!(seen.insert(control.id().clone()), "duplicate control id");
        match control {
            Control::Group { children, .. } => {
                for child in children {
                    self.ensure_control_states(item, child, active, seen, window, cx);
                }
            }
            Control::Number(number) => {
                let text = Self::numeric_text(item, &number.common.target);
                self.ensure_number_text(item.id, number, text, window, cx);
                self.ensure_animation_state(item, control, active, window, cx);
            }
            Control::Text(text_control) => {
                let text = match &text_control.common.value {
                    ParameterValue::String(value) => value.clone(),
                    _ => String::new(),
                };
                self.ensure_text_control(item.id, text_control, text, window, cx);
            }
            Control::Color(color) => {
                self.ensure_color_control(item.id, item, color, window, cx);
                self.ensure_animation_state(item, control, active, window, cx);
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
        let mut active = ActiveAnimations::default();
        let mut seen = HashSet::new();
        let roots = self.store.tree.roots.clone();
        for control in &roots {
            self.ensure_control_states(item, control, &mut active, &mut seen, window, cx);
        }
        self.store.states.retain(|key, state| {
            state.clear_inactive_animation(active.text.contains(key), active.color.contains(key));
            true
        });
    }

    fn current_input_structure(
        editor: &TimelineEditor,
        item: Option<&TimelineItem>,
    ) -> InspectorInputStructure {
        let mut array_lengths = Vec::new();
        if let Some(item) = item {
            array_lengths.extend(item.parameters.iter().filter_map(|(parameter_id, value)| {
                let ParameterValue::Array(values) = value else {
                    return None;
                };
                Some((None, parameter_id.to_owned(), values.len()))
            }));
            for effect in &item.effects {
                array_lengths.extend(effect.parameters.iter().filter_map(
                    |(parameter_id, value)| {
                        let ParameterValue::Array(values) = value else {
                            return None;
                        };
                        Some((Some(effect.id.get()), parameter_id.to_owned(), values.len()))
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
            editing_scene,
            arguments: scene_arguments,
        };
        let mut item_controls = if let Some(scene_id) = item.scene_id() {
            editor
                .scene(scene_id)
                .map(|scene| {
                    Self::scene_argument_value_controls(
                        scene_id,
                        &scene.arguments,
                        item,
                        &resolution,
                    )
                })
                .unwrap_or_default()
        } else {
            Self::item_controls(item, &resolution)
        };
        if item.scene_id().is_none() {
            item_controls.retain(|control| {
                Self::parameter_is_common(selected_items, control.parameter_id())
            });
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
        let selected_items = editor.read(cx).selected_items();
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
        if selected_item.is_some() {
            let item = selected_item.as_ref().unwrap();
            self.reconcile_states(item, window, cx);
        }
        let animation_target = self.animation_selection.read(cx).target().cloned();
        let invalid_animation_target = animation_target.as_ref().is_some_and(|target| {
            let editor = editor.read(cx);
            let target_exists = editor
                .item(target.item_id)
                .and_then(|item| {
                    item.animation(
                        target.effect_id,
                        &target.parameter_id,
                        target.address.array_index,
                    )
                })
                .is_some_and(|animation| animation.channel_enabled(target.address.channel));
            let selected_another_item = selected_items.len() != 1
                || selected_item
                    .as_ref()
                    .is_some_and(|item| item.id != target.item_id);
            !target_exists || selected_another_item
        });
        if invalid_animation_target {
            self.animation_selection
                .update(cx, |selection, cx| selection.clear(cx));
        }
        cx.notify();
    }
}
