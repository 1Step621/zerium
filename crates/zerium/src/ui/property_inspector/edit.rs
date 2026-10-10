use rust_i18n::t;

use super::*;
use crate::ui::input::set_input_text;

#[derive(Clone, Copy)]
pub(super) enum ArrayEdit {
    MoveUp(PropertyElementId),
    MoveDown(PropertyElementId),
    Remove(PropertyElementId),
}

impl ArrayEdit {
    pub(super) fn element_id(self) -> PropertyElementId {
        match self {
            Self::MoveUp(id) | Self::MoveDown(id) | Self::Remove(id) => id,
        }
    }
}

impl PropertyInspector {
    pub(super) fn reset_property(&mut self, address: PropertyAddress, cx: &mut Context<Self>) {
        let result = self.editor.update(cx, |editor, cx| {
            if !editor.is_item_selected(address.item_id) {
                return Ok(false);
            }
            let result = editor.reset_property(&address);
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

    pub(super) fn inspector_item_id(&self, cx: &App) -> Option<ItemId> {
        self.item_id
            .filter(|id| self.editor.read(cx).is_item_selected(*id))
    }

    fn inspector_value_at_playhead(
        &self,
        target: &PropertyAddress,
        cx: &App,
    ) -> Option<PropertyValue> {
        if self.inspector_item_id(cx)? != target.item_id {
            return None;
        }
        let editor = self.editor.read(cx);
        editor.evaluated_property_value(target, TimelineTime::from_frame(editor.playhead()))
    }

    /// The single domain write channel for every resolved scalar control.
    /// Parsing, clamping, and event filtering stay at the UI boundary; this
    /// method applies the validated value to the displayed item.
    pub(super) fn set_scalar(
        &mut self,
        target: &PropertyAddress,
        value: PropertyValue,
        cx: &mut Context<Self>,
    ) -> bool {
        let item_id = target.item_id;
        if self.inspector_item_id(cx) != Some(item_id) {
            return false;
        }
        let editor = self.editor.read(cx);
        let Some(item) = editor.item(item_id) else {
            return false;
        };
        if item
            .animation_track(
                target.effect_id,
                &target.property_id,
                target.element_id,
                target.scalar_index,
            )
            .is_some()
        {
            return false;
        }
        let result = self.editor.update(cx, |editor, cx| {
            let result = editor.edit_property(target, value);
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

    fn live_numeric_value(&self, target: &PropertyAddress, cx: &App) -> Option<f64> {
        self.inspector_value_at_playhead(target, cx)?
            .numeric_scalar()
    }

    pub(super) fn update_numeric_scalar(
        &mut self,
        target: &PropertyAddress,
        value: f64,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(updated) = self
            .inspector_value_at_playhead(target, cx)
            .and_then(|current| current.with_numeric_scalar(value))
        else {
            return false;
        };
        self.set_scalar(target, updated, cx)
    }

    pub(super) fn apply_scalar_step(
        &mut self,
        target: &PropertyAddress,
        spec: &NumericInputSpec,
        input: &Entity<InputState>,
        event: &NumberInputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = spec
            .parse_number(&input.read(cx).value())
            .or_else(|| self.live_numeric_value(target, cx))
            .unwrap_or(spec.min);
        let value = spec.stepped_value(value, event);
        self.update_numeric_scalar(target, value, cx);
    }

    fn animation_stop_value(&self, id: &ControlId, cx: &App) -> Option<PropertyValue> {
        let stop = self.store.tree.animation_stop(id)?;
        let address = stop.edit.address();
        let editor = self.editor.read(cx);
        let item = editor.item(address.item_id)?;
        Some(
            item.animation_track(
                address.effect_id,
                &address.property_id,
                address.element_id,
                address.scalar_index,
            )?
            .stops()
            .get(stop.edit.index())?
            .value()
            .clone(),
        )
    }

    fn set_animation_stop_value(
        &mut self,
        id: &ControlId,
        value: PropertyValue,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(stop) = self.store.tree.animation_stop(id) else {
            return false;
        };
        self.editor.update_if_changed(cx, |editor| {
            editor.set_property_animation_stop(&stop.edit, value)
        })
    }

    pub(super) fn update_animation_stop_numeric(
        &mut self,
        id: &ControlId,
        displayed_value: f64,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(value) = self
            .animation_stop_value(id, cx)
            .and_then(|value| value.with_numeric_scalar(displayed_value))
        else {
            return false;
        };
        self.set_animation_stop_value(id, value, cx)
    }

    pub(super) fn apply_animation_stop_step(
        &mut self,
        id: &ControlId,
        spec: &NumericInputSpec,
        input: &Entity<InputState>,
        event: &NumberInputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = spec
            .parse_number(&input.read(cx).value())
            .or_else(|| {
                self.animation_stop_value(id, cx)
                    .and_then(|value| value.numeric_scalar())
            })
            .unwrap_or(spec.min);
        let value = spec.stepped_value(value, event);
        self.update_animation_stop_numeric(id, value, cx);
    }

    pub(super) fn apply_animation_stop_color(
        &mut self,
        id: &ControlId,
        event: &ColorPickerEvent,
        cx: &mut Context<Self>,
    ) {
        let ColorPickerEvent::Change(Some(color)) = event else {
            return;
        };
        let color = Rgba::from(*color);
        self.set_animation_stop_value(
            id,
            PropertyValue::Color([color.r, color.g, color.b, color.a]),
            cx,
        );
    }

    /// Single update channel for string text input.
    pub(super) fn apply_string_text(
        &mut self,
        target: &PropertyAddress,
        input: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let value = input.read(cx).value().to_string();
        self.set_scalar(target, PropertyValue::String(value), cx);
    }

    /// Single update channel for color pickers.
    pub(super) fn apply_color(
        &mut self,
        target: &PropertyAddress,
        event: &ColorPickerEvent,
        cx: &mut Context<Self>,
    ) {
        let ColorPickerEvent::Change(Some(color)) = event else {
            return;
        };
        let color = Rgba::from(*color);
        let value = PropertyValue::Color([color.r, color.g, color.b, color.a]);
        self.set_scalar(target, value, cx);
    }

    pub(super) fn handle_value_drag(
        &mut self,
        origin: &PropertyValueDragOrigin,
        pointer_x: f32,
        fine_adjustment: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor.read(cx).selected_item_ids().next().is_none() {
            return;
        }
        let value = origin
            .adjustment
            .value_at(pointer_x, fine_adjustment)
            .clamp(origin.min, origin.max);

        if matches!(origin.input_id, ControlId::AnimationStop { .. }) {
            self.update_animation_stop_numeric(&origin.input_id, value, cx);
        } else {
            self.update_numeric_scalar(&origin.target, value, cx);
        }
        if let Some(input) = self.store.text_inputs.get(&origin.input_id) {
            let displayed = if matches!(origin.input_id, ControlId::AnimationStop { .. }) {
                self.animation_stop_value(&origin.input_id, cx)
            } else {
                self.inspector_value_at_playhead(&origin.target, cx)
            }
            .map(|value| Self::numeric_value_text(&value))
            .unwrap_or_else(|| Self::format_value(value));
            set_input_text(&input.input, displayed, window, cx);
        }
    }

    pub(super) fn prepare_value_drag(
        &mut self,
        target: &PropertyAddress,
        spec: &NumericInputSpec,
        input_id: &ControlId,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let input = self.store.text_inputs.get(input_id);
        let start_value = input
            .and_then(|state| spec.parse_number(&state.input.read(cx).value()))
            .or_else(|| {
                if matches!(input_id, ControlId::AnimationStop { .. }) {
                    self.animation_stop_value(input_id, cx)
                        .and_then(|value| value.numeric_scalar())
                } else {
                    self.live_numeric_value(target, cx)
                }
            })
            .unwrap_or(spec.min);
        self.begin_number_drag(
            Rc::new(PropertyValueDragOrigin {
                target: target.clone(),
                input_id: input_id.clone(),
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

    pub(super) fn select_animation(&mut self, address: &PropertyAddress, cx: &mut Context<Self>) {
        if self.inspector_item_id(cx) != Some(address.item_id)
            || self
                .editor
                .read(cx)
                .item(address.item_id)
                .is_none_or(|item| {
                    item.animation_track(
                        address.effect_id,
                        &address.property_id,
                        address.element_id,
                        address.scalar_index,
                    )
                    .is_none()
                })
        {
            return;
        }
        self.editor.update(cx, |editor, cx| {
            if editor.set_active_edit_effect(address.effect_id) {
                cx.notify();
            }
        });
        self.animation_selection
            .update(cx, |selection, cx| selection.select(address.clone(), cx));
    }

    pub(super) fn set_animation_enabled(
        &mut self,
        address: &PropertyAddress,
        enabled: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.inspector_item_id(cx) != Some(address.item_id) {
            return;
        }
        let changed = self.editor.update(cx, |editor, cx| {
            let changed = editor.set_property_animation_enabled(address, enabled);
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
                selection.select(address.clone(), cx);
            } else {
                selection.clear_if(address, cx);
            }
        });
        cx.notify();
    }

    pub(super) fn edit_array(
        editor: &mut TimelineEditor,
        address: &PropertyAddress,
        edit: ArrayEdit,
    ) -> bool {
        let Some(PropertyValue::Array(mut elements)) = editor.property_value(address) else {
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
        editor.update_property(address, PropertyValue::Array(elements))
    }

    pub(super) fn push_element(
        editor: &mut TimelineEditor,
        address: &PropertyAddress,
        value: PropertyValue,
    ) -> bool {
        let Some(mut updated) = editor.property_value(address) else {
            return false;
        };
        updated.push_element(value) && editor.update_property(address, updated)
    }
}
