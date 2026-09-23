use super::*;

#[derive(Clone, Copy)]
pub(super) enum ArrayEdit {
    MoveUp(PropertyElementId),
    MoveDown(PropertyElementId),
    Remove(PropertyElementId),
}

impl ArrayEdit {
    fn element_id(self) -> PropertyElementId {
        match self {
            Self::MoveUp(id) | Self::MoveDown(id) | Self::Remove(id) => id,
        }
    }
}

impl PropertyInspector {
    fn selected_item_at_playhead(&self, cx: &App) -> Option<TimelineItem> {
        let editor = self.editor.read(cx);
        editor.selected_item().map(|item| {
            item.evaluated_at_time(crate::domain::timeline::TimelineTime::from_frame(
                editor.playhead(),
            ))
        })
    }

    /// The single domain write channel for every resolved scalar control.
    /// Parsing, clamping, and event filtering stay at the UI boundary; this
    /// method only applies the already validated value to the current
    /// selection.
    pub(super) fn set_scalar(
        &mut self,
        target: &PropertyTarget,
        value: PropertyValue,
        cx: &mut Context<Self>,
    ) -> bool {
        self.editor.update_if_changed(cx, |editor| {
            editor.update_selected_scalar(
                target.effect_id,
                &target.property_id,
                target.element_id,
                target.scalar_index,
                value,
            )
        })
    }

    fn live_numeric_value(item: &TimelineItem, target: &PropertyTarget) -> Option<f64> {
        let value = match target.effect_id {
            Some(effect_id) => item
                .effects
                .iter()
                .find(|effect| effect.id == effect_id)?
                .properties
                .property(&target.property_id)?,
            None => item.properties.property(&target.property_id)?,
        };
        match target.element_id {
            Some(id) => match value {
                PropertyValue::Array(values) => values
                    .iter()
                    .find(|element| element.element_id() == id)?
                    .value()
                    .scalar_at(target.scalar_index)?
                    .numeric_scalar(),
                _ => None,
            },
            None => value.scalar_at(target.scalar_index)?.numeric_scalar(),
        }
    }

    pub(super) fn update_numeric_scalar(
        &mut self,
        target: &PropertyTarget,
        value: f64,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(updated) = self.selected_item_at_playhead(cx).and_then(|item| {
            let current = match target.effect_id {
                Some(effect_id) => item
                    .effects
                    .iter()
                    .find(|effect| effect.id == effect_id)?
                    .properties
                    .property(&target.property_id)?,
                None => item.properties.property(&target.property_id)?,
            };
            let current = match target.element_id {
                Some(id) => match current {
                    PropertyValue::Array(values) => values
                        .iter()
                        .find(|element| element.element_id() == id)?
                        .value(),
                    _ => return None,
                },
                None => current,
            };
            current
                .scalar_at(target.scalar_index)?
                .with_numeric_scalar(value)
        }) else {
            return false;
        };
        self.set_scalar(target, updated, cx)
    }

    /// Single update channel for number text input.
    pub(super) fn apply_scalar_text(
        &mut self,
        target: &PropertyTarget,
        spec: &NumericInputSpec,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        if !input.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        let input_value = input.read(cx).value().to_string();
        let current_value = self
            .selected_item_at_playhead(cx)
            .and_then(|item| Self::live_numeric_value(&item, target));
        if current_value.is_some_and(|value| input_value == Self::format_value(value)) {
            return;
        }
        let Some(number) = NumericInput::new(spec.scalar_type.clone()) else {
            return;
        };
        let Some(value) = number
            .parse(&input_value)
            .and_then(|value| value.numeric_scalar())
        else {
            return;
        };
        let value = value.clamp(spec.min, spec.max);
        self.update_numeric_scalar(target, value, cx);
    }

    pub(super) fn apply_scalar_step(
        &mut self,
        target: &PropertyTarget,
        spec: &NumericInputSpec,
        input: &Entity<InputState>,
        event: &NumberInputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = input.read(cx).value().parse::<f64>().unwrap_or(spec.min);
        let value = Self::normalize_field_value(
            spec,
            match event {
                NumberInputEvent::Step {
                    action: StepAction::Increment,
                    ..
                } => value + spec.step,
                NumberInputEvent::Step {
                    action: StepAction::Decrement,
                    ..
                } => value - spec.step,
            },
        );
        self.update_numeric_scalar(target, value, cx);
    }

    fn animation_stop_value(
        editor: &TimelineEditor,
        binding: &AnimationStopBinding,
    ) -> Option<PropertyValue> {
        let item = editor.selected_item()?;
        (item.id == binding.item_id).then_some(())?;
        let stop = item
            .animation_track(
                binding.effect_id,
                &binding.property_id,
                binding.element_id,
                binding.scalar_index,
            )?
            .stops()
            .get(binding.stop)?;
        Some(stop.value().clone())
    }

    fn set_animation_stop_value(
        &mut self,
        binding: &AnimationStopBinding,
        value: PropertyValue,
        cx: &mut Context<Self>,
    ) -> bool {
        let focused_segment = {
            let selection = self.animation_selection.read(cx);
            selection
                .address()
                .filter(|target| {
                    target.item_id == binding.item_id
                        && target.effect_id == binding.effect_id
                        && target.property_id == binding.property_id
                        && target.element_id == binding.element_id
                        && target.scalar_index == binding.scalar_index
                })
                .and_then(|_| selection.focused_segment())
        };
        self.editor.update_if_changed(cx, |editor| {
            let Some(current) = Self::animation_stop_value(editor, binding) else {
                return false;
            };
            if current == value {
                return false;
            }
            editor.set_selected_property_animation_stop(
                binding.effect_id,
                binding.property_id.clone(),
                binding.element_id,
                binding.scalar_index,
                binding.stop,
                value,
                focused_segment,
            )
        })
    }

    fn update_animation_stop_numeric(
        &mut self,
        binding: &AnimationStopBinding,
        displayed_value: f64,
        cx: &mut Context<Self>,
    ) -> bool {
        if !binding.value_factor.is_finite() || binding.value_factor == 0. {
            return false;
        }
        let source_value = displayed_value / binding.value_factor;
        let Some(value) = Self::animation_stop_value(self.editor.read(cx), binding)
            .and_then(|value| value.with_numeric_scalar(source_value))
        else {
            return false;
        };
        self.set_animation_stop_value(binding, value, cx)
    }

    pub(super) fn apply_animation_stop_text(
        &mut self,
        binding: &AnimationStopBinding,
        spec: &NumericInputSpec,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        if !input.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        let input_value = input.read(cx).value().to_string();
        let current = Self::animation_stop_value(self.editor.read(cx), binding)
            .and_then(|value| value.numeric_scalar())
            .map(|value| value * binding.value_factor);
        if current.is_some_and(|value| input_value == Self::format_value(value)) {
            return;
        }
        let Some(value) = NumericInput::new(spec.scalar_type.clone())
            .and_then(|number| number.parse(&input_value))
            .and_then(|value| value.numeric_scalar())
        else {
            return;
        };
        self.update_animation_stop_numeric(binding, value.clamp(spec.min, spec.max), cx);
    }

    pub(super) fn apply_animation_stop_step(
        &mut self,
        binding: &AnimationStopBinding,
        spec: &NumericInputSpec,
        input: &Entity<InputState>,
        event: &NumberInputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = input.read(cx).value().parse::<f64>().unwrap_or(spec.min);
        let value = Self::normalize_field_value(
            spec,
            match event {
                NumberInputEvent::Step {
                    action: StepAction::Increment,
                    ..
                } => value + spec.step,
                NumberInputEvent::Step {
                    action: StepAction::Decrement,
                    ..
                } => value - spec.step,
            },
        );
        self.update_animation_stop_numeric(binding, value, cx);
    }

    pub(super) fn apply_animation_stop_color(
        &mut self,
        binding: &AnimationStopBinding,
        event: &ColorPickerEvent,
        cx: &mut Context<Self>,
    ) {
        let ColorPickerEvent::Change(Some(color)) = event else {
            return;
        };
        let color = Rgba::from(*color);
        self.set_animation_stop_value(
            binding,
            PropertyValue::Color([color.r, color.g, color.b, color.a]),
            cx,
        );
    }

    /// Single update channel for string text input.
    pub(super) fn apply_string_text(
        &mut self,
        binding: &PropertyBinding,
        input: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let value = input.read(cx).value().to_string();
        if self
            .editor
            .read(cx)
            .selected_item()
            .is_none_or(|item| item.id != binding.item_id)
        {
            return;
        }
        self.set_scalar(&binding.target, PropertyValue::String(value), cx);
    }

    /// Single update channel for color pickers.
    pub(super) fn apply_color(
        &mut self,
        binding: &PropertyBinding,
        event: &ColorPickerEvent,
        cx: &mut Context<Self>,
    ) {
        let ColorPickerEvent::Change(Some(color)) = event else {
            return;
        };
        if !self.editor.read(cx).selected_item().is_some_and(|item| {
            item.id == binding.item_id
                && match binding.target.effect_id {
                    Some(effect_id) => item
                        .effects
                        .iter()
                        .find(|effect| effect.id == effect_id)
                        .and_then(|effect| effect.properties.property(&binding.target.property_id))
                        .is_some(),
                    None => item
                        .properties
                        .property(&binding.target.property_id)
                        .is_some(),
                }
        }) {
            return;
        }
        let color = Rgba::from(*color);
        let value = PropertyValue::Color([color.r, color.g, color.b, color.a]);
        if self
            .selected_item_at_playhead(cx)
            .and_then(|item| Self::color_value(&item, &binding.target))
            .is_some_and(|current| {
                current
                    .into_iter()
                    .zip([color.r, color.g, color.b, color.a])
                    .all(|(current, next)| (current - next).abs() <= 0.000_01)
            })
        {
            return;
        }
        self.set_scalar(&binding.target, value, cx);
    }

    pub(super) fn handle_value_drag(
        &mut self,
        drag: &PropertyValueDrag,
        pointer_x: f32,
        fine_adjustment: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if drag.inspector_id != cx.entity_id() || self.editor.read(cx).selected_item().is_none() {
            return;
        }
        let Some(origin) = self
            .store
            .value_drag_origin
            .as_ref()
            .filter(|origin| origin.input_id == drag.input_id)
            .cloned()
        else {
            return;
        };
        let sensitivity = origin.sensitivity * if fine_adjustment { 0.1 } else { 1. };
        let value = model::snap_to_step(
            origin.start_value + f64::from(pointer_x - origin.start_x) * sensitivity,
            origin.step,
        )
        .clamp(origin.min, origin.max);

        if let Some(binding) = &origin.animation_stop {
            self.update_animation_stop_numeric(binding, value, cx);
        } else {
            self.update_numeric_scalar(&origin.target, value, cx);
        }
        if let Some(input) = self
            .store
            .states
            .get(&drag.input_id)
            .and_then(state::ControlState::text)
        {
            Self::set_input_value(&input.input, Self::format_value(value), window, cx);
        }
    }

    pub(super) fn prepare_value_drag(
        &mut self,
        target: &PropertyTarget,
        spec: &NumericInputSpec,
        input_id: &ControlId,
        animation_stop: Option<AnimationStopBinding>,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        let input = self
            .store
            .states
            .get(input_id)
            .and_then(state::ControlState::text);
        let start_value = input
            .and_then(|state| state.input.read(cx).value().parse::<f64>().ok())
            .unwrap_or(spec.min);
        self.store.value_drag_origin = Some(PropertyValueDragOrigin {
            target: target.clone(),
            input_id: input_id.clone(),
            animation_stop,
            start_x: f32::from(event.position.x),
            start_value,
            min: spec.min,
            max: spec.max,
            step: spec.step,
            sensitivity: Self::drag_sensitivity(spec.min, spec.max, spec.step),
        });
    }

    pub(super) fn finish_value_drag(&mut self, cx: &mut Context<Self>) {
        self.store.value_drag_origin = None;
        self.store.scene_argument_value_drag_origin = None;
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
    }

    pub(super) fn select_number_animation(
        &mut self,
        target: &PropertyTarget,
        cx: &mut Context<Self>,
    ) {
        let selected_items = self.editor.read(cx).selected_items();
        let address = match selected_items.as_slice() {
            [item] => {
                number_animation_source(item, &target.address(item.id)).map(|source| source.address)
            }
            _ => None,
        };
        self.animation_selection
            .update(cx, |selection, cx| match address {
                Some(address) => selection.select(address, cx),
                None => selection.clear(cx),
            });
    }

    pub(super) fn set_number_animation_enabled(
        &mut self,
        target: &PropertyTarget,
        enabled: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self.editor.read(cx).selected_item() else {
            return;
        };
        let mut resolved = target.clone();
        if !enabled {
            let Some(display) = number_animation_source(&item, &target.address(item.id)) else {
                return;
            };
            resolved.property_id = display.address.property_id;
            resolved.element_id = display.address.element_id;
            resolved.scalar_index = display.address.scalar_index;
        }
        self.set_animation_enabled(&resolved, enabled, _window, cx);
    }

    pub(super) fn select_animation(&mut self, property: &PropertyTarget, cx: &mut Context<Self>) {
        let items = self.editor.read(cx).selected_items();
        let target = match items.as_slice() {
            [item] if property.animation_enabled(item) => Some(property.address(item.id)),
            _ => None,
        };
        self.animation_selection
            .update(cx, |selection, cx| match target {
                Some(target) => selection.select(target, cx),
                None => selection.clear(cx),
            });
    }

    pub(super) fn set_animation_enabled(
        &mut self,
        property: &PropertyTarget,
        enabled: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self.editor.read(cx).selected_item() else {
            return;
        };
        let address = property.address(item.id);
        let changed = self.editor.update(cx, |editor, cx| {
            let changed = editor.set_selected_property_animation_enabled(
                property.effect_id,
                address.property_id.clone(),
                address.element_id,
                address.scalar_index,
                enabled,
            );
            if changed {
                cx.notify();
            }
            changed
        });
        if !changed {
            return;
        }
        self.animation_selection.update(cx, |selection, cx| {
            if enabled {
                selection.select(address, cx);
            } else {
                selection.clear_if(&address, cx);
            }
        });
        cx.notify();
    }

    pub(super) fn update_elements(
        editor: &mut TimelineEditor,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
        value: PropertyValue,
    ) -> bool {
        match effect_id {
            Some(effect_id) => {
                editor.update_selected_effect_property(effect_id, property_id, value)
            }
            None => editor.update_selected_property(property_id, value),
        }
    }

    pub(super) fn edit_selected_array(
        editor: &mut TimelineEditor,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
        edit: ArrayEdit,
    ) -> bool {
        let Some(PropertyValue::Array(mut elements)) =
            Self::selected_property_value(editor, effect_id, property_id)
        else {
            return false;
        };
        let Some(index) = elements
            .iter()
            .position(|element| element.element_id() == edit.element_id())
        else {
            return false;
        };
        match edit {
            ArrayEdit::MoveUp(_) if index > 0 => elements.swap(index, index - 1),
            ArrayEdit::MoveDown(_) if index + 1 < elements.len() => elements.swap(index, index + 1),
            ArrayEdit::Remove(_) => {
                elements.remove(index);
            }
            _ => return false,
        }
        Self::update_elements(
            editor,
            effect_id,
            property_id,
            PropertyValue::Array(elements),
        )
    }

    fn selected_property_value(
        editor: &TimelineEditor,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
    ) -> Option<PropertyValue> {
        editor
            .selected_item()?
            .property_values(effect_id)?
            .property(property_id)
            .cloned()
    }

    pub(super) fn push_element(
        editor: &mut TimelineEditor,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
        value: PropertyValue,
    ) -> bool {
        let Some(mut updated) = Self::selected_property_value(editor, effect_id, property_id)
        else {
            return false;
        };
        if !updated.push_element(value) {
            return false;
        }
        Self::update_elements(editor, effect_id, property_id, updated)
    }

    pub(super) fn choose_file(
        &mut self,
        input_id: String,
        _: &gpui::ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.loading_file {
            return;
        }
        let Some(item) = self.editor.read(cx).selected_item() else {
            return;
        };
        let Some(schema) = item.schema() else {
            return;
        };
        if schema.file(&input_id).is_none() {
            return;
        }

        let item_id = item.id;
        let expected_plugin_id = item.plugin_id().unwrap_or_default().to_owned();
        let expected_item_id = item.item_id().unwrap_or_default().to_owned();
        let expected_input_id = input_id;
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("ファイルを選択".into()),
        });
        self.loading_file = true;
        self.file_error = None;
        let editor = self.editor.clone();
        let media_readers = self.media_readers.clone();
        let session = self.session.clone();
        let operation = session.update(cx, |session, cx| {
            let operation = session.begin(ProjectActivity::Probe);
            cx.notify();
            operation
        });
        self._file_task = cx.spawn(async move |inspector, cx| {
            let path = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(error)) => {
                    if let Some(inspector) = inspector.upgrade() {
                        inspector.update(cx, |inspector, cx| {
                            let message = format!("ファイルを選択できません: {error}");
                            inspector.loading_file = false;
                            inspector.file_error = Some(message.clone().into());
                            inspector.notifications.update(cx, |notifications, cx| {
                                notifications.push(message, cx);
                            });
                            cx.notify();
                        });
                    }
                    session.update(cx, |session, cx| {
                        if session.finish(operation) {
                            cx.notify();
                        }
                    });
                    return;
                }
                Err(error) => {
                    if let Some(inspector) = inspector.upgrade() {
                        inspector.update(cx, |inspector, cx| {
                            let message =
                                format!("ファイル選択ダイアログから応答を取得できません: {error}");
                            inspector.loading_file = false;
                            inspector.file_error = Some(message.clone().into());
                            inspector.notifications.update(cx, |notifications, cx| {
                                notifications.push(message, cx);
                            });
                            cx.notify();
                        });
                    }
                    session.update(cx, |session, cx| {
                        if session.finish(operation) {
                            cx.notify();
                        }
                    });
                    return;
                }
            };
            if !session.update(cx, |session, _| session.operation_is_current(operation)) {
                return;
            }
            let Some(path) = path else {
                if let Some(inspector) = inspector.upgrade() {
                    inspector.update(cx, |inspector, cx| {
                        inspector.loading_file = false;
                        cx.notify();
                    });
                }
                session.update(cx, |session, cx| {
                    if session.finish(operation) {
                        cx.notify();
                    }
                });
                return;
            };
            let probe_plugin_id = expected_plugin_id.clone();
            let probe_item_id = expected_item_id.clone();
            let probe_input_id = expected_input_id.clone();
            let result = cx
                .background_spawn(async move {
                    media_readers.probe_for_item(
                        path,
                        &probe_plugin_id,
                        &probe_item_id,
                        &probe_input_id,
                    )
                })
                .await;
            if !session.update(cx, |session, _| session.operation_is_current(operation)) {
                return;
            }
            if let Some(inspector) = inspector.upgrade() {
                inspector.update(cx, |inspector, cx| {
                    inspector.loading_file = false;
                    match result {
                        Ok(imported)
                            if imported.plugin_id == expected_plugin_id
                                && imported.item_id == expected_item_id
                                && imported.input_id == expected_input_id =>
                        {
                            let result = editor.update(cx, |editor, cx| {
                                let result = editor.set_item_asset(item_id, imported);
                                if result.is_ok() {
                                    cx.notify();
                                }
                                result
                            });
                            if let Err(error) = result {
                                inspector.file_error = Some(error.to_string().into());
                            }
                        }
                        Ok(_) => {
                            inspector.file_error =
                                Some("選択したファイルの種類がこのアイテムと一致しません".into());
                        }
                        Err(error) => inspector.file_error = Some(error.to_string().into()),
                    }
                    if let Some(error) = inspector.file_error.clone() {
                        inspector.notifications.update(cx, |notifications, cx| {
                            notifications.push(error, cx);
                        });
                    }
                    cx.notify();
                });
            }
            session.update(cx, |session, cx| {
                if session.finish(operation) {
                    cx.notify();
                }
            });
        });
        cx.notify();
    }
}
