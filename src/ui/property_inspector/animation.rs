use super::*;

impl PropertyInspector {
    pub(crate) fn animation_presentation(
        editor: &TimelineEditor,
        item: &TimelineItem,
        target: &AnimationTarget,
    ) -> Option<AnimationPresentation> {
        let mut fields = Self::property_fields(item)
            .into_iter()
            .chain(Self::array_property_fields(item))
            .chain(item.effects.iter().flat_map(Self::effect_property_fields))
            .chain(
                item.effects
                    .iter()
                    .flat_map(Self::effect_array_property_fields),
            )
            .collect::<Vec<_>>();
        let mut colors = Self::color_fields(item)
            .into_iter()
            .chain(item.effects.iter().flat_map(Self::effect_color_fields))
            .collect::<Vec<_>>();
        if let Some(scene_id) = item.scene_id()
            && let Some(scene) = editor.scene(scene_id)
        {
            fields.extend(Self::scene_property_fields(scene_id, &scene.arguments));
            colors.extend(Self::scene_color_fields(scene_id, &scene.arguments));
        }
        if let Some(color) = colors
            .into_iter()
            .find(|color| color.target.key == target.property)
        {
            let expected = color.target.animation_target(item.id);
            if expected != *target || !color.target.animation_enabled(item) {
                return None;
            }
            // Color curves show interpolation progress, independently of the color picker.
            return Some(AnimationPresentation {
                label: color.label,
                suffix: String::new(),
                step: 0.01,
                value_scale: 1.,
            });
        }
        let field = fields
            .into_iter()
            .find(|field| field.target.key == target.property)?;
        let display = Self::number_animation(item, &field)?;
        if display.source_parameter_id != target.parameter_id
            || display.source_address != target.address
        {
            return None;
        }
        let label = field.element_label.as_ref().map_or_else(
            || field.label.clone(),
            |label| format!("{} {label}", field.label),
        );
        Some(AnimationPresentation {
            label,
            suffix: field.input.suffix,
            step: field.input.step,
            value_scale: display.value_scale,
        })
    }

    fn linked_animation_aspect_ratio(
        item: &TimelineItem,
        field: &NumberField,
    ) -> Option<(f64, String, AnimationChannel)> {
        if field.target.effect_id.is_some()
            || !field.is_size
            || field.target.value_path.tuple_element() != Some(1)
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
        field: &NumberField,
    ) -> Option<NumberAnimationDisplay> {
        if let Some(animation) = item.animation(
            field.target.effect_id,
            &field.target.parameter_id,
            field.target.value_path.array_element(),
        ) && let Some((from, to)) =
            animation.numeric_range(field.target.animation_address().channel)
        {
            return Some(NumberAnimationDisplay {
                source_parameter_id: field.target.parameter_id.clone(),
                source_address: field.target.animation_address(),
                value_scale: field.input.display_scale,
                from: from * field.input.display_scale,
                to: to * field.input.display_scale,
            });
        }

        let (aspect_ratio, source_parameter_id, source_channel) =
            Self::linked_animation_aspect_ratio(item, field)?;
        let source_array_index = None;
        let source = item.animation(None, &source_parameter_id, source_array_index)?;
        let value_scale = aspect_ratio.recip() * field.input.display_scale;
        let (from, to) = source.numeric_range(source_channel)?;
        Some(NumberAnimationDisplay {
            source_parameter_id,
            source_address: ParameterAnimationAddress {
                array_index: source_array_index,
                channel: source_channel,
            },
            value_scale,
            from: from * value_scale,
            to: to * value_scale,
        })
    }

    pub(super) fn ensure_animation_inputs(
        &mut self,
        item: &TimelineItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let scene_fields = self.selected_scene_property_fields(item, cx);
        let fields = Self::property_fields(item)
            .into_iter()
            .chain(scene_fields)
            .chain(Self::array_property_fields(item))
            .chain(item.effects.iter().flat_map(Self::effect_property_fields))
            .chain(
                item.effects
                    .iter()
                    .flat_map(Self::effect_array_property_fields),
            )
            .collect::<Vec<_>>();
        let active_keys = fields
            .iter()
            .filter(|field| Self::number_animation(item, field).is_some())
            .map(|field| field.target.key.clone())
            .collect::<HashSet<_>>();
        self.controls
            .animation_inputs
            .retain(|key, _| active_keys.contains(key));
        self.controls
            .animation_input_subscriptions
            .retain(|key, _| active_keys.contains(key));
        for field in fields {
            let Some(display) = Self::number_animation(item, &field) else {
                continue;
            };
            if let Some((from, to)) = self.controls.animation_inputs.get(&field.target.key) {
                Self::set_input_value(from, Self::format_value(display.from), window, cx);
                Self::set_input_value(to, Self::format_value(display.to), window, cx);
                continue;
            }
            let from = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(SharedString::from(Self::format_value(display.from)))
            });
            let to = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(SharedString::from(Self::format_value(display.to)))
            });
            let subscriptions = [
                (from.clone(), AnimationEndpoint::From),
                (to.clone(), AnimationEndpoint::To),
            ]
            .into_iter()
            .map(|(input, endpoint)| {
                let binding = AnimationInputBinding {
                    path: field.target.key.clone(),
                    endpoint,
                };
                cx.subscribe_in(&input, window, move |this, input, event, window, cx| {
                    this.handle_animation_input_change(&binding, input, event, window, cx);
                })
            })
            .collect();
            self.controls
                .animation_input_subscriptions
                .insert(field.target.key.clone(), subscriptions);
            self.controls
                .animation_inputs
                .insert(field.target.key, (from, to));
        }
        self.ensure_animation_color_inputs(item, window, cx);
    }

    pub(super) fn ensure_animation_color_inputs(
        &mut self,
        item: &TimelineItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut fields = Self::color_fields(item)
            .into_iter()
            .chain(item.effects.iter().flat_map(Self::effect_color_fields))
            .collect::<Vec<_>>();
        if let Some(scene_id) = item.scene_id()
            && let Some(scene) = self.editor.read(cx).scene(scene_id)
        {
            fields.extend(Self::scene_color_fields(scene_id, &scene.arguments));
        }
        let active_keys = fields
            .iter()
            .filter(|field| {
                item.animation(
                    field.target.effect_id,
                    &field.target.parameter_id,
                    field.target.value_path.array_element(),
                )
                .and_then(|animation| animation.endpoints(field.target.animation_address().channel))
                .is_some()
            })
            .flat_map(|field| {
                [AnimationEndpoint::From, AnimationEndpoint::To]
                    .into_iter()
                    .map(|endpoint| (field.target.key.clone(), endpoint))
            })
            .collect::<HashSet<_>>();
        self.controls
            .animation_color_pickers
            .retain(|key, _| active_keys.contains(key));
        self.controls
            .animation_color_subscriptions
            .retain(|key, _| active_keys.contains(key));
        for field in fields {
            let Some(animation) = item.animation(
                field.target.effect_id,
                &field.target.parameter_id,
                field.target.value_path.array_element(),
            ) else {
                continue;
            };
            let Some((from, to)) = animation.endpoints(field.target.animation_address().channel)
            else {
                continue;
            };
            for (endpoint, value) in [
                (AnimationEndpoint::From, from.clone()),
                (AnimationEndpoint::To, to.clone()),
            ] {
                let ParameterValue::Color(color) = value else {
                    continue;
                };
                let key = (field.target.key.clone(), endpoint);
                if let Some(picker) = self.controls.animation_color_pickers.get(&key) {
                    let color = Self::color_to_hsla(color);
                    if picker.read(cx).value() != Some(color) {
                        picker.update(cx, |picker, cx| picker.set_value(color, window, cx));
                    }
                    continue;
                }
                let color = Self::color_to_hsla(color);
                let picker = cx.new(|cx| ColorPickerState::new(window, cx).default_value(color));
                let binding = ParameterBinding {
                    item_id: item.id,
                    target: field.target.clone(),
                };
                let subscription =
                    cx.subscribe_in(&picker, window, move |this, _, event, _, cx| {
                        this.handle_animation_color_change(&binding, endpoint, event, cx);
                    });
                self.controls
                    .animation_color_subscriptions
                    .insert(key.clone(), subscription);
                self.controls.animation_color_pickers.insert(key, picker);
            }
        }
    }

    pub(super) fn handle_animation_input_change(
        &mut self,
        binding: &AnimationInputBinding,
        input: &Entity<InputState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let Some(field) = self.field_for_path(&binding.path, cx) else {
            return;
        };
        let input_value = input.read(cx).value().to_string();
        let display = self
            .editor
            .read(cx)
            .selected_item()
            .and_then(|item| Self::number_animation(&item, &field));
        let Some(display) = display else {
            return;
        };
        let current_endpoint = match binding.endpoint {
            AnimationEndpoint::From => display.from,
            AnimationEndpoint::To => display.to,
        };
        if input_value == Self::format_value(current_endpoint) {
            return;
        }
        if !self.controls.animation_inputs.contains_key(&binding.path) {
            return;
        }
        let parsed = input_value.parse::<f64>();
        let Ok(value) = parsed else {
            return;
        };
        let value = Self::normalize_field_value(&field, value);
        let (from, to) = match binding.endpoint {
            AnimationEndpoint::From => (value, display.to),
            AnimationEndpoint::To => (display.from, value),
        };
        self.editor.update_if_changed(cx, |editor| {
            editor.update_selected_parameter_animation_numeric_range(
                field.target.effect_id,
                &display.source_parameter_id,
                display.source_address,
                from / display.value_scale,
                to / display.value_scale,
            )
        });
    }

    pub(super) fn select_number_animation(&mut self, field: &NumberField, cx: &mut Context<Self>) {
        let selected_items = self.editor.read(cx).selected_items();
        let target = match selected_items.as_slice() {
            [item] => Self::number_animation(item, field).map(|display| AnimationTarget {
                item_id: item.id,
                effect_id: field.target.effect_id,
                parameter_id: display.source_parameter_id,
                address: display.source_address,
                property: field.target.key.clone(),
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
        field: &NumberField,
        enabled: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self.editor.read(cx).selected_item() else {
            return;
        };
        let mut target = field.target.clone();
        if !enabled {
            let Some(display) = Self::number_animation(&item, field) else {
                return;
            };
            target.parameter_id = display.source_parameter_id;
            target.value_path = SceneBindingValuePath::from_elements(
                display.source_address.array_index,
                display.source_address.channel.coordinate(),
            );
        }
        self.set_animation_enabled(&target, enabled, _window, cx);
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
}
