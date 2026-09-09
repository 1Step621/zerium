use super::*;
use crate::ui::property_inspector::control::NumberSpec;

impl PropertyInspector {
    fn write_scalar(
        editor: &mut TimelineEditor,
        effect_id: Option<EffectInstanceId>,
        parameter_id: &str,
        path: SceneBindingValuePath,
        value: ParameterValue,
    ) -> bool {
        editor.update_selected_scalar(effect_id, parameter_id, path, value)
    }

    /// The single domain write channel for every resolved scalar control.
    /// Parsing, clamping, and event filtering stay at the UI boundary; this
    /// method only applies the already validated value to the current
    /// selection.
    pub(super) fn set_scalar(
        &mut self,
        target: &PropertyTarget,
        value: ParameterValue,
        cx: &mut Context<Self>,
    ) -> bool {
        self.editor.update_if_changed(cx, |editor| {
            Self::write_scalar(
                editor,
                target.effect_id,
                &target.parameter_id,
                target.value_path,
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
                .parameters
                .get(&target.parameter_id)?,
            None => item.parameters.get(&target.parameter_id)?,
        };
        match target.value_path.array_element() {
            Some(index) => match value {
                ParameterValue::Array(values) => values
                    .get(index)?
                    .scalar_at(target.value_path.tuple_element())?
                    .numeric_scalar(),
                _ => None,
            },
            None => value
                .scalar_at(target.value_path.tuple_element())?
                .numeric_scalar(),
        }
    }

    pub(super) fn update_numeric_scalar(
        &mut self,
        target: &PropertyTarget,
        value: f64,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(updated) = self.editor.read(cx).selected_item().and_then(|item| {
            let current = match target.effect_id {
                Some(effect_id) => item
                    .effects
                    .iter()
                    .find(|effect| effect.id == effect_id)?
                    .parameters
                    .get(&target.parameter_id)?,
                None => item.parameters.get(&target.parameter_id)?,
            };
            let current = match target.value_path.array_element() {
                Some(index) => match current {
                    ParameterValue::Array(values) => values.get(index)?,
                    _ => return None,
                },
                None => current,
            };
            current
                .scalar_at(target.value_path.tuple_element())?
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
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let input_value = input.read(cx).value().to_string();
        let current_value = self
            .editor
            .read(cx)
            .selected_item()
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

    /// Single update channel for string text input.
    pub(super) fn apply_string_text(
        &mut self,
        binding: &ParameterBinding,
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
        self.set_scalar(&binding.target, ParameterValue::String(value), cx);
    }

    /// Single update channel for color pickers.
    pub(super) fn apply_color(
        &mut self,
        binding: &ParameterBinding,
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
                        .and_then(|effect| effect.parameters.get(&binding.target.parameter_id))
                        .is_some(),
                    None => item.parameters.get(&binding.target.parameter_id).is_some(),
                }
        }) {
            return;
        }
        let color = Rgba::from(*color);
        let value = ParameterValue::Color([color.r, color.g, color.b, color.a]);
        self.set_scalar(&binding.target, value, cx);
    }

    pub(super) fn apply_animation_color(
        &mut self,
        binding: &ParameterBinding,
        endpoint: AnimationEndpoint,
        event: &ColorPickerEvent,
        cx: &mut Context<Self>,
    ) {
        let ColorPickerEvent::Change(Some(color)) = event else {
            return;
        };
        let Some(animation) = self.editor.read(cx).selected_item().and_then(|item| {
            if item.id != binding.item_id {
                return None;
            }
            item.animation(
                binding.target.effect_id,
                &binding.target.parameter_id,
                binding.target.value_path.array_element(),
            )
            .cloned()
        }) else {
            return;
        };
        let color = Rgba::from(*color);
        let value = ParameterValue::Color([color.r, color.g, color.b, color.a]);
        let Some((current_from, current_to)) =
            animation.endpoints(binding.target.animation_address().channel)
        else {
            return;
        };
        let (from, to) = match endpoint {
            AnimationEndpoint::From => (value, current_to.clone()),
            AnimationEndpoint::To => (current_from.clone(), value),
        };
        self.editor.update_if_changed(cx, |editor| {
            editor.update_selected_parameter_animation_range(
                binding.target.effect_id,
                &binding.target.parameter_id,
                binding.target.animation_address(),
                from,
                to,
            )
        });
    }

    /// Single update channel for animation endpoint text input.
    pub(super) fn apply_animation_text(
        &mut self,
        target: &PropertyTarget,
        spec: &NumberSpec,
        endpoint: AnimationEndpoint,
        input: &Entity<InputState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let input_value = input.read(cx).value().to_string();
        let display = self
            .editor
            .read(cx)
            .selected_item()
            .and_then(|item| Self::number_animation(&item, target, spec));
        let Some(display) = display else {
            return;
        };
        let current_endpoint = match endpoint {
            AnimationEndpoint::From => display.from,
            AnimationEndpoint::To => display.to,
        };
        if input_value == Self::format_value(current_endpoint) {
            return;
        }
        if self
            .store
            .states
            .get(&ControlId::property(&target.key))
            .and_then(state::ControlState::animation_text)
            .is_none()
        {
            return;
        }
        let parsed = input_value.parse::<f64>();
        let Ok(value) = parsed else {
            return;
        };
        let value = Self::normalize_field_value(spec, value);
        let (from, to) = match endpoint {
            AnimationEndpoint::From => (value, display.to),
            AnimationEndpoint::To => (display.from, value),
        };
        self.editor.update_if_changed(cx, |editor| {
            editor.update_selected_parameter_animation_numeric_range(
                target.effect_id,
                &display.source_parameter_id,
                display.source_address,
                from / display.value_factor,
                to / display.value_factor,
            )
        });
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
            .filter(|origin| {
                origin.target.key == drag.path
                    && origin.animation_endpoint == drag.animation_endpoint
            })
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

        if let Some(endpoint) = origin.animation_endpoint {
            let current = self
                .editor
                .read(cx)
                .selected_item()
                .as_ref()
                .and_then(|item| {
                    item.animation(
                        origin.target.effect_id,
                        &origin.target.parameter_id,
                        origin.target.value_path.array_element(),
                    )
                })
                .cloned();
            let Some(current) = current else {
                return;
            };
            let Some((current_from, current_to)) =
                current.numeric_range(origin.target.animation_address().channel)
            else {
                return;
            };
            let current_from = current_from * origin.animation_factor;
            let current_to = current_to * origin.animation_factor;
            let (from, to) = match endpoint {
                AnimationEndpoint::From => (value, current_to),
                AnimationEndpoint::To => (current_from, value),
            };
            self.editor.update_if_changed(cx, |editor| {
                editor.update_selected_parameter_animation_numeric_range(
                    origin.target.effect_id,
                    &origin.target.parameter_id,
                    origin.target.animation_address(),
                    from / origin.animation_factor,
                    to / origin.animation_factor,
                )
            });
        } else {
            self.update_numeric_scalar(&origin.target, value, cx);
            if let Some(input) = self
                .store
                .states
                .get(&ControlId::property(&drag.path))
                .and_then(state::ControlState::text)
            {
                Self::set_input_value(&input.input, Self::format_value(value), window, cx);
            }
        }
    }

    pub(super) fn prepare_value_drag(
        &mut self,
        target: &PropertyTarget,
        spec: &NumberSpec,
        animation_endpoint: Option<AnimationEndpoint>,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        let key = &target.key;
        let input = match animation_endpoint {
            Some(AnimationEndpoint::From) => self
                .store
                .states
                .get(&ControlId::property(key))
                .and_then(state::ControlState::animation_text)
                .map(|(from, _)| from),
            Some(AnimationEndpoint::To) => self
                .store
                .states
                .get(&ControlId::property(key))
                .and_then(state::ControlState::animation_text)
                .map(|(_, to)| to),
            None => self
                .store
                .states
                .get(&ControlId::property(key))
                .and_then(state::ControlState::text),
        };
        let start_value = input
            .and_then(|state| state.input.read(cx).value().parse::<f64>().ok())
            .unwrap_or(spec.min);
        let display = animation_endpoint.and_then(|_| {
            self.editor
                .read(cx)
                .selected_item()
                .and_then(|item| Self::number_animation(&item, target, spec))
        });
        let mut resolved = target.clone();
        if let Some(display) = &display {
            resolved.parameter_id = display.source_parameter_id.clone();
            resolved.value_path = SceneBindingValuePath::from_elements(
                display.source_address.array_index,
                display.source_address.channel.coordinate(),
            );
        }
        self.store.value_drag_origin = Some(PropertyValueDragOrigin {
            target: resolved,
            animation_endpoint,
            start_x: f32::from(event.position.x),
            start_value,
            min: spec.min,
            max: spec.max,
            step: spec.step,
            sensitivity: Self::drag_sensitivity(spec.min, spec.max, spec.step),
            animation_factor: display.map_or(1., |display| display.value_factor),
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
    ) -> Option<(f64, String, AnimationChannel)> {
        if target.effect_id.is_some()
            || !spec.is_size
            || target.value_path.tuple_element() != Some(1)
        {
            return None;
        }
        let schema = item.schema()?;
        if !item.aspect_ratio_locked {
            return None;
        }
        let size = schema.size_parameter()?;
        let value = item.parameters.get(size.id())?;
        let width = value.scalar_at(Some(0))?.numeric_scalar()?;
        let height = value.scalar_at(Some(1))?.numeric_scalar()?;
        (width.is_finite() && height.is_finite() && width > 0. && height > 0.).then_some((
            width / height,
            size.id().to_owned(),
            AnimationChannel::TupleElement(0),
        ))
    }

    pub(super) fn number_animation(
        item: &TimelineItem,
        target: &PropertyTarget,
        spec: &NumberSpec,
    ) -> Option<NumberAnimationDisplay> {
        if let Some(animation) = item.animation(
            target.effect_id,
            &target.parameter_id,
            target.value_path.array_element(),
        ) && let Some((from, to)) = animation.numeric_range(target.animation_address().channel)
        {
            return Some(NumberAnimationDisplay {
                source_parameter_id: target.parameter_id.clone(),
                source_address: target.animation_address(),
                value_factor: 1.,
                from,
                to,
            });
        }

        let (aspect_ratio, source_parameter_id, source_channel) =
            Self::linked_animation_aspect_ratio(item, target, spec)?;
        let source_array_index = None;
        let source = item.animation(None, &source_parameter_id, source_array_index)?;
        let value_factor = aspect_ratio.recip();
        let (from, to) = source.numeric_range(source_channel)?;
        Some(NumberAnimationDisplay {
            source_parameter_id,
            source_address: ParameterAnimationAddress {
                array_index: source_array_index,
                channel: source_channel,
            },
            value_factor,
            from: from * value_factor,
            to: to * value_factor,
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
            [item] => Self::number_animation(item, target, spec).map(|display| AnimationTarget {
                item_id: item.id,
                effect_id: target.effect_id,
                parameter_id: display.source_parameter_id,
                address: display.source_address,
                property: target.key.clone(),
            }),
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
            let Some(display) = Self::number_animation(&item, target, spec) else {
                return;
            };
            resolved.parameter_id = display.source_parameter_id;
            resolved.value_path = SceneBindingValuePath::from_elements(
                display.source_address.array_index,
                display.source_address.channel.coordinate(),
            );
        }
        self.set_animation_enabled(&resolved, enabled, _window, cx);
    }

    pub(super) fn select_animation(&mut self, property: &PropertyTarget, cx: &mut Context<Self>) {
        let items = self.editor.read(cx).selected_items();
        let target = match items.as_slice() {
            [item] if property.animation_enabled(item) => Some(property.animation_target(item.id)),
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
        let target = property.animation_target(item.id);
        let changed = self.editor.update(cx, |editor, cx| {
            let changed = editor.set_selected_parameter_animation_enabled(
                property.effect_id,
                &property.parameter_id,
                property.animation_address(),
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

    fn animation_parameter(
        editor: &TimelineEditor,
        item: &TimelineItem,
        target: &AnimationTarget,
    ) -> Option<ParameterSchema> {
        if let Some(scene_id) = item.scene_id()
            && let Some(parameter) = editor
                .scene(scene_id)?
                .arguments
                .iter()
                .find(|argument| argument.schema.id() == target.parameter_id)
                .map(|argument| argument.schema.parameter().clone())
        {
            return Some(parameter);
        }
        if let Some(effect_id) = target.effect_id {
            return item
                .effects
                .iter()
                .find(|effect| effect.id == effect_id)
                .and_then(|effect| effect.schema().parameter(&target.parameter_id))
                .cloned();
        }
        item.schema()?.parameter(&target.parameter_id).cloned()
    }

    fn animation_property_target(target: &AnimationTarget) -> PropertyTarget {
        PropertyTarget {
            key: target.property.clone(),
            parameter_id: target.parameter_id.clone(),
            effect_id: target.effect_id,
            value_path: SceneBindingValuePath::from_elements(
                target.address.array_index,
                target.address.channel.coordinate(),
            ),
        }
    }

    fn animation_label(parameter: &ParameterSchema, target: &AnimationTarget) -> String {
        let label = target.address.array_index.map_or_else(
            || parameter.label().to_owned(),
            |index| format!("{} {}", parameter.label(), index + 1),
        );
        match target.address.channel {
            AnimationChannel::TupleElement(element) => parameter
                .scalar_label(Some(element))
                .map_or(label.clone(), |element_label| {
                    format!("{label} {element_label}")
                }),
            AnimationChannel::Scalar => label,
        }
    }

    pub(crate) fn animation_presentation(
        editor: &TimelineEditor,
        item: &TimelineItem,
        target: &AnimationTarget,
    ) -> Option<AnimationPresentation> {
        let parameter = Self::animation_parameter(editor, item, target)?;
        let property_target = Self::animation_property_target(target);
        let tuple_element = target.address.channel.coordinate();
        let scalar_type = parameter
            .ty()
            .element_type()
            .scalar_at(tuple_element)?
            .clone();
        let label = Self::animation_label(&parameter, target);
        if matches!(scalar_type, ScalarParameterType::Color) {
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
                .is_some_and(|schema| schema.is_size_parameter(&target.parameter_id));
        let spec = Self::number_spec(&parameter, tuple_element, is_size)?;
        // Scene-bound values resolve through the same display mapping as the
        // inspector rows; size linkage only applies to size parameters.
        let display = Self::number_animation(item, &property_target, &spec)?;
        if display.source_parameter_id != target.parameter_id
            || display.source_address != target.address
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

    pub(super) fn update_array_parameter(
        editor: &mut TimelineEditor,
        effect_id: Option<EffectInstanceId>,
        parameter_id: &str,
        value: ParameterValue,
    ) -> bool {
        match effect_id {
            Some(effect_id) => {
                editor.update_selected_effect_parameter(effect_id, parameter_id, value)
            }
            None => editor.update_selected_parameter(parameter_id, value),
        }
    }

    pub(super) fn push_array_element(
        editor: &mut TimelineEditor,
        effect_id: Option<EffectInstanceId>,
        parameter_id: &str,
        value: ParameterValue,
    ) -> bool {
        let Some(item) = editor.selected_item() else {
            return false;
        };
        let current = match effect_id {
            Some(effect_id) => item
                .effects
                .iter()
                .find(|effect| effect.id == effect_id)
                .and_then(|effect| effect.parameters.get(parameter_id)),
            None => item.parameters.get(parameter_id),
        };
        let Some(ParameterValue::Array(current)) = current else {
            return false;
        };
        let mut updated = current.clone();
        updated.push(value);
        Self::update_array_parameter(
            editor,
            effect_id,
            parameter_id,
            ParameterValue::Array(updated),
        )
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
