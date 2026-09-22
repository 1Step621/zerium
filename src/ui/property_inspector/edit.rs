use super::*;
use crate::ui::property_inspector::control::NumberSpec;

impl PropertyInspector {
    fn selected_item_at_playhead(&self, cx: &App) -> Option<TimelineItem> {
        let editor = self.editor.read(cx);
        editor.selected_item().map(|item| {
            item.evaluated_at_time(crate::domain::timeline::TimelineTime::from_frame(
                editor.playhead(),
            ))
        })
    }

    fn write_scalar(
        editor: &mut TimelineEditor,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
        path: InspectorPath,
        value: PropertyValue,
    ) -> bool {
        editor.update_selected_scalar(
            effect_id,
            property_id,
            path.element_id(),
            path.scalar_index(),
            value,
        )
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
            Self::write_scalar(
                editor,
                target.effect_id,
                &target.property_id,
                target.path.clone(),
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
        match target.path.element_id() {
            Some(id) => match value {
                PropertyValue::Array(values) => values
                    .iter()
                    .find(|element| element.element_id() == id)?
                    .value()
                    .scalar_at(target.path.scalar_index())?
                    .numeric_scalar(),
                _ => None,
            },
            None => value
                .scalar_at(target.path.scalar_index())?
                .numeric_scalar(),
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
            let current = match target.path.element_id() {
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
                .scalar_at(target.path.scalar_index())?
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
        spec: &NumberSpec,
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
        spec: &NumberSpec,
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
                .target()
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
        spec: &NumberSpec,
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
        spec: &NumberSpec,
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
        spec: &NumberSpec,
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

    fn linked_animation_aspect_ratio(
        item: &TimelineItem,
        target: &PropertyTarget,
        spec: &NumberSpec,
    ) -> Option<(f64, String, InspectorPath)> {
        if target.effect_id.is_some() || !spec.is_size || target.path.scalar_index() != Some(1) {
            return None;
        }
        let schema = item.schema()?;
        if !item.aspect_ratio_locked {
            return None;
        }
        let size = schema.size_property()?;
        let value = item.properties.property(size.id())?;
        let width = value.scalar_at(Some(0))?.numeric_scalar()?;
        let height = value.scalar_at(Some(1))?.numeric_scalar()?;
        (width.is_finite() && height.is_finite() && width > 0. && height > 0.).then_some((
            width / height,
            size.id().to_owned(),
            InspectorPath::new(None, Some(0)),
        ))
    }

    pub(super) fn number_animation_source(
        item: &TimelineItem,
        target: &PropertyTarget,
        spec: &NumberSpec,
    ) -> Option<NumberAnimationSource> {
        let target_element_id = target.path.element_id();
        let target_scalar_index = target.path.scalar_index();
        if item
            .animation_track(
                target.effect_id,
                &target.property_id,
                target_element_id,
                target_scalar_index,
            )
            .is_some()
        {
            return Some(NumberAnimationSource {
                property_id: target.property_id.clone(),
                element_id: target_element_id,
                scalar_index: target_scalar_index,
                value_factor: 1.,
            });
        }

        let (aspect_ratio, source_property_id, source_path) =
            Self::linked_animation_aspect_ratio(item, target, spec)?;
        let source_element_id = source_path.element_id();
        let source_scalar_index = source_path.scalar_index();
        item.animation_track(
            None,
            &source_property_id,
            source_element_id,
            source_scalar_index,
        )?;
        let value_factor = aspect_ratio.recip();
        Some(NumberAnimationSource {
            property_id: source_property_id,
            element_id: source_element_id,
            scalar_index: source_scalar_index,
            value_factor,
        })
    }

    pub(super) fn select_number_animation(
        &mut self,
        target: &PropertyTarget,
        spec: &NumberSpec,
        cx: &mut Context<Self>,
    ) {
        let selected_items = self.editor.read(cx).selected_items();
        let target = match selected_items.as_slice() {
            [item] => {
                Self::number_animation_source(item, target, spec).map(|display| AnimationTarget {
                    item_id: item.id,
                    effect_id: target.effect_id,
                    property_id: display.property_id,
                    element_id: display.element_id,
                    scalar_index: display.scalar_index,
                    property: target.key.clone(),
                })
            }
            _ => None,
        };
        self.animation_selection
            .update(cx, |selection, cx| match target {
                Some(target) => selection.select(target, cx),
                None => selection.clear(cx),
            });
    }

    pub(super) fn set_number_animation_enabled(
        &mut self,
        target: &PropertyTarget,
        spec: &NumberSpec,
        enabled: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self.editor.read(cx).selected_item() else {
            return;
        };
        let mut resolved = target.clone();
        if !enabled {
            let Some(display) = Self::number_animation_source(&item, target, spec) else {
                return;
            };
            resolved.property_id = display.property_id;
            resolved.path = InspectorPath::new(display.element_id, display.scalar_index);
        }
        self.set_animation_enabled(&resolved, enabled, _window, cx);
    }

    pub(super) fn select_animation(&mut self, property: &PropertyTarget, cx: &mut Context<Self>) {
        let items = self.editor.read(cx).selected_items();
        let target = match items.as_slice() {
            [item] if property.animation_enabled(item) => Some(property.animation_target(item)),
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
        let target = property.animation_target(&item);
        let changed = self.editor.update(cx, |editor, cx| {
            let changed = editor.set_selected_property_animation_enabled(
                property.effect_id,
                target.property_id.clone(),
                target.element_id,
                target.scalar_index,
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
                selection.select(target, cx);
            } else {
                selection.clear_if(&target, cx);
            }
        });
        cx.notify();
    }

    fn animation_property(
        editor: &TimelineEditor,
        item: &TimelineItem,
        target: &AnimationTarget,
    ) -> Option<PropertySchema> {
        if let Some(scene_id) = item.scene_id()
            && let Some(property) = editor
                .scene(scene_id)?
                .arguments
                .iter()
                .find(|argument| argument.schema.id() == target.property_id)
                .map(|argument| argument.schema.property().clone())
        {
            return Some(property);
        }
        if let Some(effect_id) = target.effect_id {
            return item
                .effects
                .iter()
                .find(|effect| effect.id == effect_id)
                .and_then(|effect| effect.schema().property(&target.property_id))
                .cloned();
        }
        item.schema()?.property(&target.property_id).cloned()
    }

    fn animation_property_target(
        item: &TimelineItem,
        target: &AnimationTarget,
    ) -> Option<PropertyTarget> {
        if let Some(id) = target.element_id {
            item.property_values(target.effect_id)?
                .property(&target.property_id)?
                .element_index(id)?;
        }
        Some(PropertyTarget {
            key: target.property.clone(),
            property_id: target.property_id.clone(),
            effect_id: target.effect_id,
            path: InspectorPath::new(target.element_id, target.scalar_index),
        })
    }

    fn animation_label(
        property: &PropertySchema,
        target: &AnimationTarget,
        element_index: Option<usize>,
    ) -> String {
        let label = element_index.map_or_else(
            || property.label().to_owned(),
            |index| format!("{} {}", property.label(), index + 1),
        );
        match target.scalar_index {
            Some(scalar_index) => property
                .configuration_label(Some(scalar_index))
                .map_or(label.clone(), |scalar_label| {
                    format!("{label} {scalar_label}")
                }),
            None => label,
        }
    }

    pub(crate) fn animation_presentation(
        editor: &TimelineEditor,
        item: &TimelineItem,
        target: &AnimationTarget,
    ) -> Option<AnimationPresentation> {
        let property = Self::animation_property(editor, item, target)?;
        let property_target = Self::animation_property_target(item, target)?;
        let scalar_index = target.scalar_index;
        let value_type = match property.ty() {
            PropertyType::Value(value_type)
            | PropertyType::Array {
                element_type: value_type,
                ..
            } => value_type,
        };
        let scalar_type = value_type.scalar_at(scalar_index)?.clone();
        let label = Self::animation_label(
            &property,
            target,
            property_target.path.element_id().and_then(|id| {
                item.property_values(target.effect_id)?
                    .property(&target.property_id)?
                    .element_index(id)
            }),
        );
        if matches!(scalar_type, ScalarPropertyType::Color) {
            if !property_target.animation_enabled(item) {
                return None;
            }
            return Some(AnimationPresentation {
                label,
                suffix: String::new(),
                step: 0.01,
                value_factor: 1.,
            });
        }
        let is_size = target.effect_id.is_none()
            && item.scene_id().is_none()
            && item
                .schema()
                .is_some_and(|schema| schema.is_size_property(&target.property_id));
        let spec = Self::number_spec(&property, scalar_index, is_size)?;
        // Scene-bound values resolve through the same display mapping as the
        // inspector elements; size linkage only applies to size properties.
        let display = Self::number_animation_source(item, &property_target, &spec)?;
        if display.property_id != target.property_id
            || display.element_id != target.element_id
            || display.scalar_index != target.scalar_index
        {
            return None;
        }
        Some(AnimationPresentation {
            label,
            suffix: spec.suffix.clone(),
            step: spec.step,
            value_factor: display.value_factor,
        })
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

    pub(super) fn push_element(
        editor: &mut TimelineEditor,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
        value: PropertyValue,
    ) -> bool {
        let Some(item) = editor.selected_item() else {
            return false;
        };
        let current = match effect_id {
            Some(effect_id) => item
                .effects
                .iter()
                .find(|effect| effect.id == effect_id)
                .and_then(|effect| effect.properties.property(property_id)),
            None => item.properties.property(property_id),
        };
        let Some(current) = current.cloned() else {
            return false;
        };
        let mut updated = current;
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
        let operation = session.update(cx, |session, cx| session.begin(ProjectActivity::Probe, cx));
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
                        session.finish(operation, cx);
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
                        session.finish(operation, cx);
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
                    session.finish(operation, cx);
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
                session.finish(operation, cx);
            });
        });
        cx.notify();
    }
}
