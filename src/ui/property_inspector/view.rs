use super::*;

impl PropertyInspector {
    fn render_context<'a>(&'a self, cx: &mut Context<Self>) -> InspectorRenderContext<'a> {
        let active_scene_name_input = self
            .editor
            .read(cx)
            .active_scene_id()
            .and_then(|scene_id| self.controls.scene_name_inputs.get(&scene_id).cloned());
        InspectorRenderContext {
            colors: cx.theme().colors,
            inspector: cx.entity(),
            editor: &self.editor,
            focus_handle: &self.focus_handle,
            inputs: &self.controls.inputs,
            animation_inputs: &self.controls.animation_inputs,
            color_pickers: &self.controls.color_pickers,
            animation_color_pickers: &self.controls.animation_color_pickers,
            font_names: &self.font_names,
            active_scene_name_input,
        }
    }

    pub(super) fn selected_view(&self, cx: &mut Context<Self>) -> Option<InspectorSelectionView> {
        let selected_items = self.editor.read(cx).selected_items();
        let item = selected_items.first()?.clone();
        let item_label = self
            .editor
            .read(cx)
            .item_label(item.id)
            .unwrap_or_else(|| "不明なアイテム".to_owned());
        let multiple = selected_items.len() > 1;
        let hidden_state = self.editor.read(cx).selected_items_hidden_state();
        let schema = Self::selected_schema(&item);
        let scene_arguments = self.active_scene_argument_options(cx);
        let editing_scene = self.editor.read(cx).active_scene_id().is_some() && !multiple;
        let effects = if multiple {
            Self::common_effects(&selected_items)
        } else {
            item.effects.clone()
        };
        let has_visual = schema
            .is_some_and(|schema| schema.visual().is_some() && (!multiple || !effects.is_empty()))
            || (item.scene_id().is_some() && !multiple);
        let file_inputs = (!multiple)
            .then_some(schema)
            .flatten()
            .map(|schema| {
                schema
                    .files()
                    .iter()
                    .map(|input| (input.clone(), item.media(input.id()).cloned()))
                    .collect()
            })
            .unwrap_or_default();
        let kind_label = if multiple {
            "複数".to_owned()
        } else if item.scene_id().is_some() {
            "シーン".to_owned()
        } else {
            schema
                .map(|schema| schema.label().to_owned())
                .unwrap_or_default()
        };
        let aspect_ratio_lock =
            Self::aspect_ratio_lock_state(&item, &selected_items, &scene_arguments, editing_scene);
        let mut property_controls = self.selected_property_controls(&item, &selected_items, cx);
        if multiple {
            property_controls
                .iter_mut()
                .for_each(PropertyControl::disable_animation);
        }
        let hidden_effects = {
            let editor = self.editor.read(cx);
            item.effects
                .iter()
                .filter(|effect| editor.is_effect_hidden(effect.id))
                .map(|effect| effect.id)
                .collect()
        };
        let available_effects = plugins()
            .effects()
            .map(|(plugin_id, effect)| {
                SearchPickerEntry::from_plugin_schema(
                    plugin_id,
                    effect,
                    (plugin_id.to_owned(), effect.id().to_owned()),
                )
            })
            .collect();

        Some(InspectorSelectionView {
            item,
            item_label,
            selected_count: selected_items.len(),
            aspect_ratio_lock,
            property_controls,
            effects,
            scene_arguments,
            file_inputs,
            available_effects,
            hidden_effects,
            multiple,
            editing_scene,
            has_visual,
            items_hidden: hidden_state == Some(true),
            item_visibility_mixed: hidden_state.is_none() && multiple,
            kind_label,
        })
    }

    fn active_scene_argument_options(&self, cx: &Context<Self>) -> Vec<SceneArgumentOption> {
        let editor = self.editor.read(cx);
        let Some(scene_id) = editor.active_scene_id() else {
            return Vec::new();
        };
        let Some(scene) = editor.scene(scene_id) else {
            return Vec::new();
        };

        scene
            .arguments
            .iter()
            .map(|argument| {
                let label = if argument.schema.label().is_empty() {
                    argument.schema.id().to_owned()
                } else {
                    argument.schema.label().to_owned()
                };
                let referenced_by_derived = scene.arguments.iter().any(|other| {
                    other.schema.id() != argument.schema.id()
                        && other.derived_expression_references(argument.schema.id())
                });
                SceneArgumentOption {
                    scene_id,
                    id: argument.schema.id().to_owned(),
                    label,
                    schema: argument.schema.parameter().clone(),
                    binding_count: argument.bindings.len(),
                    bindings: argument.bindings.clone(),
                    derived: argument.is_derived(),
                    referenced_by_derived,
                }
            })
            .collect()
    }

    fn selected_property_controls(
        &self,
        item: &TimelineItem,
        selected_items: &[TimelineItem],
        cx: &Context<Self>,
    ) -> Vec<PropertyControl> {
        let mut controls = if let Some(scene_id) = item.scene_id() {
            self.editor
                .read(cx)
                .scene(scene_id)
                .map(|scene| Self::scene_property_controls(scene_id, &scene.arguments, item))
                .unwrap_or_default()
        } else {
            Self::property_controls(item)
        };
        if item.scene_id().is_none() {
            controls.retain(|control| {
                Self::parameter_is_common(selected_items, control.parameter_id())
            });
        }

        for field in controls.iter_mut().filter_map(|control| match control {
            PropertyControl::Bool(field) => Some(field),
            _ => None,
        }) {
            field.mixed = selected_items.iter().skip(1).any(|selected| {
                selected
                    .parameters
                    .get(&field.target.parameter_id)
                    .and_then(|value| value.scalar_at(field.target.value_path.tuple_element()))
                    != Some(&ParameterValue::Bool(field.value))
            });
        }
        controls
    }

    pub(super) fn aspect_ratio_lock_state(
        item: &TimelineItem,
        selected_items: &[TimelineItem],
        scene_arguments: &[SceneArgumentOption],
        editing_scene: bool,
    ) -> Option<AspectRatioLockState> {
        if item.scene_id().is_some() {
            return None;
        }
        let size = item.schema()?.size_parameter()?;
        if !Self::parameter_is_common(selected_items, size.id())
            || selected_items.iter().any(|selected| {
                selected
                    .schema()
                    .is_none_or(|schema| !schema.supports_aspect_ratio_lock())
            })
        {
            return None;
        }
        let size_is_bound = scene_arguments.iter().any(|argument| {
            argument.bindings.iter().any(|binding| {
                binding.item_id() == item.id
                    && binding.owner() == SceneBindingOwner::Item
                    && binding.parameter_id() == size.id()
                    && binding.value_path() == SceneBindingValuePath::Whole
            })
        });
        Some(AspectRatioLockState {
            value: item.aspect_ratio_locked,
            mixed: selected_items
                .iter()
                .skip(1)
                .any(|selected| selected.aspect_ratio_locked != item.aspect_ratio_locked),
            multiple: selected_items.len() > 1,
            disabled_by_scene_size_argument: editing_scene && size_is_bound,
        })
    }

    pub(super) fn selected_view_element(
        &self,
        mut view: InspectorSelectionView,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let render = self.render_context(cx);
        let header = Self::selection_header(&view, &render);
        let scene_settings = view
            .editing_scene
            .then(|| self.scene_settings_element(&view.scene_arguments, &render));
        let controls = std::mem::take(&mut view.property_controls)
            .into_iter()
            .filter_map(|control| Self::property_control_element(control, &view, &render))
            .collect::<Vec<_>>();
        let files = std::mem::take(&mut view.file_inputs)
            .into_iter()
            .map(|file| self.file_input_element(file, &render))
            .collect::<Vec<_>>();
        let effects = view
            .has_visual
            .then(|| self.effects_element(&view, &render, cx));

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .id("property-inspector-scroll")
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .gap_3()
                    .p_3()
                    .when_some(scene_settings, |this, section| this.child(section))
                    .when(view.editing_scene, |this| {
                        this.child(
                            div()
                                .w_full()
                                .h(px(1.))
                                .flex_none()
                                .bg(render.colors.border),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(render.colors.muted_foreground)
                                .child("アイテム設定"),
                        )
                    })
                    .child(Self::kind_row(view.kind_label, render.colors))
                    .children(controls)
                    .children(files)
                    .when_some(self.file_error.clone(), |this, error| {
                        this.child(
                            div()
                                .w_full()
                                .text_sm()
                                .text_color(render.colors.danger)
                                .child(error),
                        )
                    })
                    .when_some(effects, |this, section| this.child(section)),
            )
            .into_any_element()
    }

    fn selection_header(view: &InspectorSelectionView, render: &InspectorRenderContext<'_>) -> Div {
        let editor = render.editor.clone();
        pane_header(render.colors)
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(if view.multiple {
                        format!("{}個のアイテム", view.selected_count)
                    } else {
                        view.item_label.clone()
                    }),
            )
            .child(
                Button::new("toggle-selected-item-visibility")
                    .small()
                    .compact()
                    .ghost()
                    .icon(if view.items_hidden {
                        IconName::EyeOff
                    } else if view.item_visibility_mixed {
                        IconName::EyeClosed
                    } else {
                        IconName::Eye
                    })
                    .tooltip(if view.items_hidden {
                        "選択アイテムを表示"
                    } else if view.item_visibility_mixed {
                        "表示状態が混在しています。すべて非表示"
                    } else {
                        "選択アイテムを非表示"
                    })
                    .on_click(move |_, _, cx| {
                        editor.update(cx, |editor, cx| {
                            if editor.toggle_selected_items_visibility() {
                                cx.notify();
                            }
                        });
                    }),
            )
    }

    fn kind_row(label: String, colors: ThemeColor) -> Div {
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::parameter_label_column("種類"))
            .child(
                div()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child(label),
            )
    }

    pub(super) fn property_control_element(
        control: PropertyControl,
        view: &InspectorSelectionView,
        render: &InspectorRenderContext<'_>,
    ) -> Option<gpui::AnyElement> {
        match control {
            PropertyControl::Number(fields) => {
                let field = fields.first()?.clone();
                let animation_enabled = Self::number_animation(&view.item, &field).is_some();
                let animation_scalars = fields
                    .iter()
                    .map(|field| field.target.animation_enabled(&view.item))
                    .collect::<Vec<_>>();
                let aspect_ratio_lock = (field.target.effect_id.is_none() && field.is_size)
                    .then_some(view.aspect_ratio_lock)
                    .flatten();
                let scene_bindings = fields
                    .iter()
                    .map(|field| {
                        let coordinate_animation_enabled =
                            Self::number_animation(&view.item, field).is_some();
                        Self::scene_binding_for_property(
                            view.editing_scene,
                            coordinate_animation_enabled,
                            view.item.id,
                            field,
                            &view.scene_arguments,
                        )
                    })
                    .collect();
                Self::property_group_row(
                    fields,
                    render.inputs,
                    render.animation_inputs,
                    animation_enabled,
                    &animation_scalars,
                    aspect_ratio_lock,
                    render.colors.muted_foreground,
                    scene_bindings,
                    render.editor,
                    &render.inspector,
                    render.focus_handle,
                )
            }
            PropertyControl::Array(field) => Some(Self::array_field_element(
                &view.item,
                field,
                render.inputs,
                render.animation_inputs,
                render.color_pickers,
                render.animation_color_pickers,
                render.editor,
                &render.inspector,
                render.focus_handle,
                render.colors.border,
                !view.multiple,
                view.editing_scene,
                &view.scene_arguments,
                render.font_names,
            )),
            PropertyControl::String(field) => Self::string_field_element(
                field,
                view.item.id,
                view.editing_scene,
                &view.scene_arguments,
                &render.inspector,
                render.inputs,
            )
            .map(IntoElement::into_any_element),
            PropertyControl::Choice(field) => {
                let binding = Self::scene_field_binding(
                    view.editing_scene,
                    false,
                    field.scene_bindable,
                    SceneBindingTarget::new(
                        view.item.id,
                        SceneBindingOwner::from_effect(field.target.effect_id),
                        field.target.parameter_id.clone(),
                        field.target.value_path,
                    ),
                    &field.ty,
                    &view.scene_arguments,
                );
                Some(Self::choice_field_element(
                    field,
                    render.editor,
                    binding,
                    &render.inspector,
                ))
            }
            PropertyControl::Bool(field) => {
                let target = SceneBindingTarget::new(
                    view.item.id,
                    SceneBindingOwner::from_effect(field.target.effect_id),
                    field.target.parameter_id.clone(),
                    field.target.value_path,
                );
                let binding = Self::scene_field_binding(
                    view.editing_scene,
                    false,
                    field.scene_bindable,
                    target,
                    &ParameterType::Value(ParameterValueType::Scalar(ScalarParameterType::Bool)),
                    &view.scene_arguments,
                );
                Some(Self::bool_field_element(
                    field,
                    render.editor,
                    binding,
                    &render.inspector,
                ))
            }
            PropertyControl::Color(field) => {
                let picker = render.color_pickers.get(&field.target.key)?;
                let animation_enabled = field.target.animation_enabled(&view.item);
                let target = SceneBindingTarget::new(
                    view.item.id,
                    SceneBindingOwner::from_effect(field.target.effect_id),
                    field.target.parameter_id.clone(),
                    field.target.value_path,
                );
                let binding = Self::scene_field_binding(
                    view.editing_scene,
                    animation_enabled,
                    field.scene_bindable,
                    target,
                    &ParameterType::Value(ParameterValueType::Scalar(ScalarParameterType::Color)),
                    &view.scene_arguments,
                );
                Some(Self::color_field_element(
                    field,
                    picker,
                    render.animation_color_pickers,
                    animation_enabled,
                    binding,
                    &render.inspector,
                ))
            }
        }
    }

    fn file_input_element(
        &self,
        (input, media): (FileCapability, Option<MediaAsset>),
        render: &InspectorRenderContext<'_>,
    ) -> gpui::AnyElement {
        let input_id = input.id().to_owned();
        let choose_input_id = input_id.clone();
        let inspector = render.inspector.clone();
        let button_label = if self.loading_file {
            "読み込み中…"
        } else if media.is_some() {
            "ファイルを変更"
        } else {
            "ファイルを選択"
        };
        let details = media.as_ref().map(Self::media_details);

        div()
            .w_full()
            .flex()
            .items_start()
            .gap_3()
            .child(Self::parameter_label_column(input.label().to_owned()))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        Button::new(SharedString::from(format!("select-item-file-{input_id}")))
                            .small()
                            .label(button_label)
                            .disabled(self.loading_file)
                            .on_click(move |event, window, cx| {
                                inspector.update(cx, |inspector, cx| {
                                    inspector.choose_file(
                                        choose_input_id.clone(),
                                        event,
                                        window,
                                        cx,
                                    );
                                });
                            }),
                    )
                    .when_some(details, |this, details| {
                        this.child(
                            div()
                                .whitespace_normal()
                                .text_sm()
                                .text_color(render.colors.muted_foreground)
                                .child(details),
                        )
                    }),
            )
            .into_any_element()
    }

    fn media_details(media: &MediaAsset) -> String {
        let duration = media.duration.as_secs_f64();
        let minutes = (duration / 60.).floor() as u64;
        let seconds = duration - minutes as f64 * 60.;
        let format = match &media.kind {
            MediaKind::Video {
                width,
                height,
                frame_rate,
                has_audio,
                ..
            } => format!(
                "{width} × {height}・{:.3} fps{}",
                frame_rate.frames_per_second(),
                if *has_audio { "・音声あり" } else { "" }
            ),
            MediaKind::Audio {
                channels,
                sample_rate,
            } => match (channels, sample_rate) {
                (Some(channels), Some(sample_rate)) => format!("{channels} ch・{sample_rate} Hz"),
                (Some(channels), None) => format!("{channels} ch"),
                (None, Some(sample_rate)) => format!("{sample_rate} Hz"),
                (None, None) => "音声ストリーム".to_owned(),
            },
            MediaKind::Image { width, height } => format!("{width} × {height}・画像"),
        };
        format!(
            "{minutes:02}:{seconds:06.3}・{format}\n{}",
            media.path.display()
        )
    }
}
