use super::*;

impl PropertyInspector {
    fn numeric_value(
        item: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
        parameter_id: &str,
        array_index: Option<usize>,
        tuple_element: Option<usize>,
    ) -> Option<f64> {
        let value = match effect_id {
            Some(effect_id) => item
                .effects
                .iter()
                .find(|effect| effect.id == effect_id)?
                .parameters
                .get(parameter_id)?,
            None => item.parameters.get(parameter_id)?,
        };
        match array_index {
            Some(index) => match value {
                ParameterValue::Array(values) => values
                    .get(index)?
                    .scalar_at(tuple_element)?
                    .numeric_scalar(),
                _ => None,
            },
            None => value.scalar_at(tuple_element)?.numeric_scalar(),
        }
    }

    pub(super) fn ensure_numeric_inputs(
        &mut self,
        item: &TimelineItem,
        fields: impl IntoIterator<Item = NumberField>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for field in fields {
            let value = Self::numeric_value(
                item,
                field.target.effect_id,
                &field.target.parameter_id,
                field.target.value_path.array_element(),
                field.target.value_path.tuple_element(),
            )
            .map(|value| Self::scaled_display_value(value, field.input.display_scale))
            .map(Self::format_value)
            .unwrap_or_default();
            if let Some(input) = self.controls.inputs.get(&field.target.key) {
                Self::set_input_value(input, value, window, cx);
                continue;
            }
            let input =
                cx.new(|cx| InputState::new(window, cx).default_value(SharedString::from(value)));
            let binding = field.target.key.clone();
            let change_binding = binding.clone();
            self.controls.input_subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, input, event, window, cx| {
                    this.handle_input_change(&change_binding, input, event, window, cx);
                },
            ));
            self.controls.input_subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, input, event, window, cx| {
                    this.handle_step(&binding, input, event, window, cx);
                },
            ));
            self.controls.inputs.insert(field.target.key, input);
        }
    }

    fn color_value(
        item: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
        parameter_id: &str,
        path: SceneBindingValuePath,
    ) -> Option<[f32; 4]> {
        let value = match effect_id {
            Some(effect_id) => item
                .effects
                .iter()
                .find(|effect| effect.id == effect_id)?
                .parameters
                .get(parameter_id)?,
            None => item.parameters.get(parameter_id)?,
        };
        let value = match path.array_element() {
            Some(index) => match value {
                ParameterValue::Array(values) => values.get(index)?,
                _ => return None,
            },
            None => value,
        }
        .scalar_at(path.tuple_element())?;
        match value {
            ParameterValue::Color(color) => Some(*color),
            _ => None,
        }
    }

    pub(super) fn ensure_color_inputs(
        &mut self,
        item: &TimelineItem,
        fields: impl IntoIterator<Item = ColorField>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for field in fields {
            let color = Self::color_value(
                item,
                field.target.effect_id,
                &field.target.parameter_id,
                field.target.value_path,
            )
            .map(Self::color_to_hsla)
            .unwrap_or_default();
            if let Some(picker) = self.controls.color_pickers.get(&field.target.key) {
                if picker.read(cx).value() != Some(color) {
                    picker.update(cx, |picker, cx| picker.set_value(color, window, cx));
                }
                continue;
            }
            let picker = cx.new(|cx| ColorPickerState::new(window, cx).default_value(color));
            let binding = ParameterBinding {
                item_id: item.id,
                target: field.target.clone(),
            };

            self.controls.input_subscriptions.push(cx.subscribe_in(
                &picker,
                window,
                move |this, _, event, _, cx| this.handle_color_change(&binding, event, cx),
            ));
            self.controls.color_pickers.insert(field.target.key, picker);
        }
    }

    pub(super) fn ensure_string_inputs(
        &mut self,
        item: &TimelineItem,
        fields: impl IntoIterator<Item = StringField>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for field in fields {
            self.ensure_string_input(
                item.id,
                field.target,
                field.value,
                field.multiline,
                window,
                cx,
            );
        }
    }

    pub(super) fn ensure_string_input(
        &mut self,
        item_id: ItemId,
        target: PropertyTarget,
        value: String,
        multiline: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = self.controls.inputs.get(&target.key) {
            Self::set_input_value(input, value, window, cx);
            return;
        }
        let input = cx.new(|cx| {
            let input = InputState::new(window, cx);
            let input = if multiline {
                input.auto_grow(2, 6)
            } else {
                input
            };
            input.default_value(SharedString::from(value))
        });
        let path = target.key.clone();
        let binding = ParameterBinding { item_id, target };

        self.controls.input_subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |this, input, event, window, cx| {
                this.handle_string_change(&binding, input, event, window, cx);
            },
        ));
        self.controls.inputs.insert(path, input);
    }

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

    pub(super) fn ensure_item_inputs(
        &mut self,
        item: &TimelineItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_numeric_inputs(item, Self::property_fields(item), window, cx);
        let strings =
            Self::property_controls(item)
                .into_iter()
                .filter_map(|control| match control {
                    PropertyControl::String(field) => Some(field),
                    _ => None,
                });
        self.ensure_string_inputs(item, strings, window, cx);
        self.ensure_color_inputs(item, Self::color_fields(item), window, cx);
    }

    pub(super) fn ensure_effect_inputs(
        &mut self,
        item: &TimelineItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for effect in &item.effects {
            self.ensure_numeric_inputs(item, Self::effect_property_fields(effect), window, cx);
            self.ensure_color_inputs(item, Self::effect_color_fields(effect), window, cx);
            let strings = Self::effect_property_controls(effect)
                .into_iter()
                .filter_map(|control| match control {
                    PropertyControl::String(field) => Some(field),
                    _ => None,
                });
            self.ensure_string_inputs(item, strings, window, cx);
        }
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

    pub(super) fn reset_input_state(&mut self) {
        self.controls = InspectorControls::default();
    }

    fn reconcile_item_inputs(
        &mut self,
        item: &TimelineItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_item_inputs(item, window, cx);
        self.ensure_scene_inputs(item, window, cx);
        self.ensure_effect_inputs(item, window, cx);
        self.ensure_array_inputs(item, window, cx);
        self.ensure_animation_inputs(item, window, cx);
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
            .controls
            .input_structure
            .as_ref()
            .and_then(|structure| structure.item_id);
        if self.controls.input_structure.as_ref() != Some(&input_structure) {
            self.reset_input_state();
            self.controls.input_structure = Some(input_structure);
        }
        if previous_item_id != selected_item.as_ref().map(|item| item.id) {
            self.file_error = None;
        }
        self.ensure_active_scene_argument_names(window, cx);
        if let Some(item) = &selected_item {
            self.reconcile_item_inputs(item, window, cx);
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

    pub(super) fn handle_input_change(
        &mut self,
        path: &PropertyPath,
        input: &Entity<InputState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let Some(field) = self.field_for_path(path, cx) else {
            return;
        };
        let input_value = input.read(cx).value().to_string();
        let current_value = self.editor.read(cx).selected_item().and_then(|item| {
            Self::numeric_value(
                &item,
                field.target.effect_id,
                &field.target.parameter_id,
                field.target.value_path.array_element(),
                field.target.value_path.tuple_element(),
            )
        });
        if current_value.is_some_and(|value| {
            input_value
                == Self::format_value(Self::scaled_display_value(value, field.input.display_scale))
        }) {
            return;
        }
        let Some(number) = NumericInput::new(field.scalar_type.clone(), field.input.display_scale)
        else {
            return;
        };
        let Some(value) = number
            .parse(&input_value)
            .and_then(|value| value.numeric_scalar())
        else {
            return;
        };
        let value = value.clamp(
            field.input.min / field.input.display_scale,
            field.input.max / field.input.display_scale,
        );

        self.update_numeric_scalar(
            field.target.effect_id,
            &field.target.parameter_id,
            field.target.value_path.array_element(),
            field.target.value_path.tuple_element(),
            value,
            cx,
        );
    }

    pub(super) fn handle_string_change(
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
        self.editor.update_if_changed(cx, |editor| {
            Self::set_scalar_value(
                editor,
                binding.target.effect_id,
                &binding.target.parameter_id,
                SceneBindingValuePath::from_elements(
                    binding.target.value_path.array_element(),
                    binding.target.value_path.tuple_element(),
                ),
                ParameterValue::String(value),
            )
        });
    }

    pub(super) fn update_numeric_scalar(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        parameter_id: &str,
        array_index: Option<usize>,
        tuple_element: Option<usize>,
        value: f64,
        cx: &mut Context<Self>,
    ) -> bool {
        if array_index.is_none() && effect_id.is_none() {
            return self.editor.update_if_changed(cx, |editor| {
                editor.update_selected_parameter_numeric_scalar(parameter_id, tuple_element, value)
            });
        }
        let Some(updated) = self.editor.read(cx).selected_item().and_then(|item| {
            let current = match effect_id {
                Some(effect_id) => item
                    .effects
                    .iter()
                    .find(|effect| effect.id == effect_id)?
                    .parameters
                    .get(parameter_id)?,
                None => item.parameters.get(parameter_id)?,
            };
            let current = match array_index {
                Some(index) => match current {
                    ParameterValue::Array(values) => values.get(index)?,
                    _ => return None,
                },
                None => current,
            };
            current.scalar_at(tuple_element)?.with_numeric_scalar(value)
        }) else {
            return false;
        };
        self.editor.update_if_changed(cx, |editor| {
            Self::set_scalar_value(
                editor,
                effect_id,
                parameter_id,
                SceneBindingValuePath::from_elements(array_index, tuple_element),
                updated,
            )
        })
    }

    pub(super) fn handle_color_change(
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
        self.editor.update_if_changed(cx, |editor| {
            Self::set_scalar_value(
                editor,
                binding.target.effect_id,
                &binding.target.parameter_id,
                binding.target.value_path,
                value,
            )
        });
    }

    pub(super) fn handle_animation_color_change(
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

    pub(super) fn field_for_path(
        &self,
        path: &PropertyPath,
        cx: &Context<Self>,
    ) -> Option<NumberField> {
        self.editor.read(cx).selected_item().and_then(|item| {
            Self::property_fields(&item)
                .into_iter()
                .chain(self.selected_scene_property_fields(&item, cx))
                .chain(item.effects.iter().flat_map(Self::effect_property_fields))
                .find(|field| &field.target.key == path)
                .or_else(|| Self::array_property_field_for_path(&item, path))
        })
    }

    pub(super) fn handle_step(
        &mut self,
        path: &PropertyPath,
        input: &Entity<InputState>,
        event: &NumberInputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(field) = self.field_for_path(path, cx) else {
            return;
        };
        let value = input
            .read(cx)
            .value()
            .parse::<f64>()
            .unwrap_or(field.input.min);
        let value = Self::normalize_field_value(
            &field,
            match event {
                NumberInputEvent::Step {
                    action: StepAction::Increment,
                    ..
                } => value + field.input.step,
                NumberInputEvent::Step {
                    action: StepAction::Decrement,
                    ..
                } => value - field.input.step,
            },
        ) / field.input.display_scale;

        self.update_numeric_scalar(
            field.target.effect_id,
            &field.target.parameter_id,
            field.target.value_path.array_element(),
            field.target.value_path.tuple_element(),
            value,
            cx,
        );
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
            .controls
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
            let current_from = current_from * origin.animation_scale;
            let current_to = current_to * origin.animation_scale;
            let (from, to) = match endpoint {
                AnimationEndpoint::From => (value, current_to),
                AnimationEndpoint::To => (current_from, value),
            };
            self.editor.update_if_changed(cx, |editor| {
                editor.update_selected_parameter_animation_numeric_range(
                    origin.target.effect_id,
                    &origin.target.parameter_id,
                    origin.target.animation_address(),
                    from / origin.animation_scale,
                    to / origin.animation_scale,
                )
            });
        } else {
            self.update_numeric_scalar(
                origin.target.effect_id,
                &origin.target.parameter_id,
                origin.target.value_path.array_element(),
                origin.target.value_path.tuple_element(),
                value / origin.display_scale,
                cx,
            );
            if let Some(input) = self.controls.inputs.get(&drag.path) {
                Self::set_input_value(input, Self::format_value(value), window, cx);
            }
        }
    }

    pub(super) fn prepare_value_drag(
        &mut self,
        field: &NumberField,
        event: &MouseDownEvent,
        animation_endpoint: Option<AnimationEndpoint>,
        cx: &mut Context<Self>,
    ) {
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        let input = match animation_endpoint {
            Some(AnimationEndpoint::From) => self
                .controls
                .animation_inputs
                .get(&field.target.key)
                .map(|(from, _)| from),
            Some(AnimationEndpoint::To) => self
                .controls
                .animation_inputs
                .get(&field.target.key)
                .map(|(_, to)| to),
            None => self.controls.inputs.get(&field.target.key),
        };
        let start_value = input
            .and_then(|input| input.read(cx).value().parse::<f64>().ok())
            .unwrap_or(field.input.min);
        let display = animation_endpoint.and_then(|_| {
            self.editor
                .read(cx)
                .selected_item()
                .and_then(|item| Self::number_animation(&item, field))
        });
        let mut target = field.target.clone();
        if let Some(display) = &display {
            target.parameter_id = display.source_parameter_id.clone();
            target.value_path = SceneBindingValuePath::from_elements(
                display.source_address.array_index,
                display.source_address.channel.coordinate(),
            );
        }
        self.controls.value_drag_origin = Some(PropertyValueDragOrigin {
            target,
            animation_endpoint,
            start_x: f32::from(event.position.x),
            start_value,
            min: field.input.min,
            max: field.input.max,
            step: field.input.step,
            sensitivity: Self::drag_sensitivity(field.input.min, field.input.max, field.input.step),
            animation_scale: display.map_or(1., |display| display.value_scale),
            display_scale: field.input.display_scale,
        });
    }

    pub(super) fn finish_value_drag(&mut self, cx: &mut Context<Self>) {
        self.controls.value_drag_origin = None;
        self.controls.scene_argument_value_drag_origin = None;
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
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
