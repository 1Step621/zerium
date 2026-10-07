use rust_i18n::t;

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
    pub(super) fn reset_property(&mut self, address: PropertyAddress, cx: &mut Context<Self>) {
        let result = self.editor.update(cx, |editor, cx| {
            if editor.selected_item().map(|item| item.id) != Some(address.item_id) {
                return Ok(false);
            }
            let result = editor.reset_selected_property(address.effect_id, address.path());
            if result.as_ref().is_ok_and(|changed| *changed) {
                cx.notify();
            }
            result
        });
        if let Err(error) = result {
            self.notifications.update(cx, |notifications, cx| {
                notifications.push(t!("inspector.edit_failed", error = error).to_string(), cx)
            });
        }
    }

    pub(super) fn bind_scene_argument(
        &mut self,
        argument_id: &str,
        target: SceneBindingTarget,
        cx: &mut Context<Self>,
    ) {
        let result = self.editor.update(cx, |editor, cx| {
            let result = editor.connect_scene_argument(argument_id, target);
            if result.is_ok() {
                cx.notify();
            }
            result
        });
        if result.is_err() {
            self.notifications.update(cx, |notifications, cx| {
                notifications.push(t!("rows.bind_failed").to_string(), cx)
            });
        } else {
            self.request_scene_argument_settings(argument_id, cx);
        }
    }

    pub(super) fn request_scene_argument_settings(
        &self,
        argument_id: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(scene_id) = self.editor.read(cx).active_scene_id() {
            cx.emit(SceneArgumentRequested {
                scene_id,
                argument_id: argument_id.to_owned(),
            });
        }
    }

    fn selected_item_at_playhead(&self, cx: &App) -> Option<TimelineItem> {
        let editor = self.editor.read(cx);
        editor.selected_item().map(|item| {
            editor.evaluated_item_at(
                &item,
                zerium_core::timeline::TimelineTime::from_frame(editor.playhead()),
            )
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
        let result = self.editor.update(cx, |editor, cx| {
            let result = editor.edit_selected_property(
                target.effect_id,
                PropertyPath::new(&target.property_id, target.element_id, target.scalar_index),
                value,
            );
            if result == Ok(true) {
                cx.notify();
            }
            result
        });
        match result {
            Ok(changed) => changed,
            Err(error) => {
                self.notifications.update(cx, |notifications, cx| {
                    notifications.push(t!("inspector.edit_failed", error = error).to_string(), cx)
                });
                false
            }
        }
    }

    fn live_numeric_value(item: &TimelineItem, target: &PropertyTarget) -> Option<f64> {
        target.value(item)?.numeric_scalar()
    }

    pub(super) fn update_numeric_scalar(
        &mut self,
        target: &PropertyTarget,
        value: f64,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(updated) = self
            .selected_item_at_playhead(cx)
            .and_then(|item| target.value(&item)?.with_numeric_scalar(value))
        else {
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
            .and_then(|item| target.value(&item).and_then(PropertyValue::numeric_text));
        if current_value.is_some_and(|value| input_value == value) {
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
        let value = spec
            .parse_number(&input.read(cx).value())
            .or_else(|| {
                self.selected_item_at_playhead(cx)
                    .and_then(|item| Self::live_numeric_value(&item, target))
            })
            .unwrap_or(spec.min);
        let value = spec.stepped_value(value, event);
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
        let Some(value) = Self::animation_stop_value(self.editor.read(cx), binding)
            .and_then(|value| value.with_numeric_scalar(displayed_value))
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
            .and_then(|value| value.numeric_text());
        if current.is_some_and(|value| input_value == value) {
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
        let value = spec
            .parse_number(&input.read(cx).value())
            .or_else(|| {
                Self::animation_stop_value(self.editor.read(cx), binding)
                    .and_then(|value| value.numeric_scalar())
            })
            .unwrap_or(spec.min);
        let value = spec.stepped_value(value, event);
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
        origin: &PropertyValueDragOrigin,
        pointer_x: f32,
        fine_adjustment: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor.read(cx).selected_item().is_none() {
            return;
        }
        let value = origin
            .adjustment
            .value_at(pointer_x, fine_adjustment)
            .clamp(origin.min, origin.max);

        if let Some(binding) = &origin.animation_stop {
            self.update_animation_stop_numeric(binding, value, cx);
        } else {
            self.update_numeric_scalar(&origin.target, value, cx);
        }
        if let Some(input) = self.store.text_inputs.get(&origin.input_id) {
            let displayed = match origin.animation_stop.as_ref() {
                Some(binding) => Self::animation_stop_value(self.editor.read(cx), binding),
                None => self
                    .selected_item_at_playhead(cx)
                    .and_then(|item| origin.target.value(&item).cloned()),
            }
            .map(|value| Self::numeric_value_text(&value))
            .unwrap_or_else(|| Self::format_value(value));
            Self::set_input_value(&input.input, displayed, window, cx);
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
        let input = self.store.text_inputs.get(input_id);
        let start_value = input
            .and_then(|state| spec.parse_number(&state.input.read(cx).value()))
            .or_else(|| match animation_stop.as_ref() {
                Some(binding) => Self::animation_stop_value(self.editor.read(cx), binding)
                    .and_then(|value| value.numeric_scalar()),
                None => self
                    .selected_item_at_playhead(cx)
                    .and_then(|item| Self::live_numeric_value(&item, target)),
            })
            .unwrap_or(spec.min);
        self.begin_number_drag(
            Rc::new(PropertyValueDragOrigin {
                target: target.clone(),
                input_id: input_id.clone(),
                animation_stop,
                adjustment: crate::ui::numeric_property::NumericDrag {
                    start_x: f32::from(event.position.x),
                    start_value,
                    step: spec.step,
                    sensitivity: spec
                        .drag_step
                        .unwrap_or_else(|| Self::drag_sensitivity(spec.min, spec.max, spec.step)),
                },
                min: spec.min,
                max: spec.max,
            }),
            cx,
        );
    }

    pub(super) fn select_animation(&mut self, property: &PropertyTarget, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            if editor.set_active_edit_effect(property.effect_id) {
                cx.notify();
            }
        });
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
        editor.update_selected_property(
            effect_id,
            PropertyPath::new(property_id, None, None),
            value,
        )
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
}
