use rust_i18n::t;

use super::*;

impl PropertyInspector {
    pub(super) fn file_row(
        common: &LeafControl,
        render: &RenderCtx<'_>,
        compact: bool,
    ) -> PropertyRow {
        let address = common.target.clone();
        let disabled = render.selecting_file
            || common.read_only
            || common
                .binding
                .as_ref()
                .is_some_and(|binding| binding.connected.is_some());

        let label = if compact {
            Self::animation_scalar_label(common, render)
        } else {
            Some(Self::animation_property_label(
                common.label.clone(),
                common,
                render,
            ))
        };
        PropertyRow::new(
            label,
            div()
                .min_w_0()
                .flex()
                .items_start()
                .gap_1()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(crate::ui::file_input::file_picker(
                            &render.file_input,
                            SharedString::from(format!("file-{:?}", common.id)),
                            crate::ui::file_input::FileTarget::Property(address),
                            common.value.file(),
                            render.selecting_file,
                            disabled,
                        )),
                )
                .when_some(common.binding.clone(), |row, binding| {
                    row.child(Self::scene_binding_button(
                        binding,
                        &render.inspector,
                        SharedString::from(format!("file-binding-{:?}", common.id)),
                    ))
                }),
        )
    }

    pub(super) fn number_full_row(
        common: &LeafControl,
        spec: &NumericInputSpec,
        input: &Entity<InputState>,
        animation_enabled: bool,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> PropertyRow {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let select_inspector = ctx.inspector.clone();
        let select_target = common.target.clone();
        let animation_button = (common.animatable && !is_bound).then(|| {
            Self::number_animation_toggle(
                &common.target,
                animation_enabled,
                if animation_enabled {
                    t!("rows.unanimate").to_string()
                } else {
                    t!("rows.animate_axis").to_string()
                },
                &ctx.inspector,
            )
        });
        let publish_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{:?}", common.target)),
            )
        });
        let value_input = Self::number_editor(common, spec, input, common.read_only, ctx);
        PropertyRow::new(
            Self::animation_property_label(common.label.clone(), common, ctx),
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .when(!is_bound, |this| {
                    this.child(
                        div()
                            .id(SharedString::from(format!("{:?}", common.target)))
                            .min_w_0()
                            .flex()
                            .flex_1()
                            .when(!common.read_only, |this| {
                                this.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    select_inspector.update(cx, |inspector, cx| {
                                        inspector.select_animation(&select_target, cx);
                                    });
                                })
                            })
                            .child(value_input),
                    )
                })
                .when_some(animation_button, |this, button| this.child(button))
                .when_some(publish_button, |this, button| this.child(button)),
        )
    }

    pub(super) fn number_compact_row(
        common: &LeafControl,
        spec: &NumericInputSpec,
        input: &Entity<InputState>,
        ctx: &RenderCtx,
    ) -> (PropertyRow, bool) {
        let coordinate_animation_enabled = common.animation_enabled;
        let binding = common.binding.clone();
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{:?}", common.target)),
            )
        });
        let disabled = common.read_only;
        let animation_visible = coordinate_animation_enabled;
        let animation_button = (common.animatable && !is_bound).then(|| {
            Self::coordinate_animation_toggle(
                &common.target,
                coordinate_animation_enabled,
                common.read_only,
                animation_visible,
                &ctx.inspector,
            )
        });
        let value_input = Self::number_editor(common, spec, input, disabled, ctx);
        let select_inspector = ctx.inspector.clone();
        let select_target = common.target.clone();
        let row = PropertyRow::new(
            Self::animation_scalar_label(common, ctx),
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .when(!is_bound, |this| {
                    this.child(div().min_w_0().flex().flex_1().child(value_input))
                })
                .when_some(animation_button, |this, button| this.child(button))
                .when_some(binding_button, |this, button| this.child(button))
                .when(disabled, |this| {
                    this.text_color(ctx.colors.muted_foreground)
                })
                .when(!common.read_only, |this| {
                    this.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        select_inspector.update(cx, |inspector, cx| {
                            inspector.select_animation(&select_target, cx);
                        });
                    })
                }),
        );
        (row, is_bound)
    }

    pub(super) fn text_full_row(
        common: &LeafControl,
        multiline: bool,
        input: &Entity<InputState>,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> PropertyRow {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{:?}", common.target)),
            )
        });
        PropertyRow::new(
            Self::animation_property_label(common.label.clone(), common, ctx),
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .when(!is_bound, |this| {
                    this.child(Self::text_editor(input, multiline, common.read_only))
                })
                .when_some(binding_button, |this, button| this.child(button)),
        )
    }

    pub(super) fn text_compact_row(
        common: &LeafControl,
        multiline: bool,
        input: &Entity<InputState>,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> (PropertyRow, bool) {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{:?}", common.target)),
            )
        });
        let row = PropertyRow::new(
            Self::animation_scalar_label(common, ctx),
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .when(!is_bound, |this| {
                    this.child(Self::text_editor(input, multiline, common.read_only))
                })
                .when_some(binding_button, |this, button| this.child(button)),
        );
        (row, is_bound)
    }

    pub(super) fn toggle_full_row(
        common: &LeafControl,
        value: bool,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> PropertyRow {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{:?}", common.target)),
            )
        });
        let switch = Self::bool_switch(&common.target, value, common.read_only, &ctx.inspector);
        PropertyRow::new(
            Self::animation_property_label(common.label.clone(), common, ctx),
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .when(!is_bound, |this| this.child(switch))
                .when_some(binding_button, |this, button| this.child(button)),
        )
    }

    pub(super) fn toggle_compact_row(
        common: &LeafControl,
        value: bool,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> (PropertyRow, bool) {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{:?}", common.target)),
            )
        });
        let switch = Self::bool_switch(&common.target, value, common.read_only, &ctx.inspector);
        let row = PropertyRow::new(
            Self::animation_scalar_label(common, ctx),
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .when(!is_bound, |this| this.child(switch))
                .when_some(binding_button, |this, button| this.child(button)),
        );
        (row, is_bound)
    }

    pub(super) fn dropdown_full_row(
        common: &LeafControl,
        current: u32,
        options: &[(String, u32)],
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> PropertyRow {
        let bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-{:?}", common.target)),
            )
        });
        PropertyRow::new(
            Self::animation_property_label(common.label.clone(), common, ctx),
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .when(!bound, |row| {
                    row.child(Self::choice_dropdown(
                        &common.target,
                        current,
                        options,
                        common.read_only,
                        &ctx.inspector,
                    ))
                })
                .when_some(binding_button, |row, button| row.child(button)),
        )
    }

    pub(super) fn dropdown_compact_row(
        common: &LeafControl,
        current: u32,
        options: &[(String, u32)],
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> (PropertyRow, bool) {
        let bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-{:?}", common.target)),
            )
        });
        let row = PropertyRow::new(
            Self::animation_scalar_label(common, ctx),
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .when(!bound, |this| {
                    this.child(Self::choice_dropdown(
                        &common.target,
                        current,
                        options,
                        common.read_only,
                        &ctx.inspector,
                    ))
                })
                .when_some(binding_button, |this, button| this.child(button)),
        );
        (row, bound)
    }

    pub(super) fn color_full_row(
        common: &LeafControl,
        picker: &Entity<ColorPickerState>,
        animation_enabled: bool,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> PropertyRow {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{:?}", common.target)),
            )
        });
        let animation_button = (common.animatable && !is_bound).then(|| {
            Self::color_animation_toggle(&common.target, animation_enabled, &ctx.inspector)
        });
        let value = Self::color_editor(common, picker, ctx);
        let select_inspector = ctx.inspector.clone();
        let select_target = common.target.clone();
        PropertyRow::new(
            Self::animation_property_label(common.label.clone(), common, ctx),
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .when(!is_bound, |this| {
                    this.child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .when(!common.read_only, |this| {
                                this.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    select_inspector.update(cx, |inspector, cx| {
                                        inspector.select_animation(&select_target, cx);
                                    });
                                })
                            })
                            .child(value),
                    )
                })
                .when_some(animation_button, |this, button| this.child(button))
                .when_some(binding_button, |this, button| this.child(button)),
        )
    }

    pub(super) fn color_compact_row(
        common: &LeafControl,
        picker: &Entity<ColorPickerState>,
        animation_enabled: bool,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> (PropertyRow, bool) {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{:?}", common.target)),
            )
        });
        let animation_button = (common.animatable && !is_bound).then(|| {
            Self::color_animation_toggle(&common.target, animation_enabled, &ctx.inspector)
        });
        let value = Self::color_editor(common, picker, ctx);
        let select_inspector = ctx.inspector.clone();
        let select_target = common.target.clone();
        let row = PropertyRow::new(
            Self::animation_scalar_label(common, ctx),
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .when(!is_bound, |this| {
                    this.child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .when(!common.read_only, |this| {
                                this.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    select_inspector.update(cx, |inspector, cx| {
                                        inspector.select_animation(&select_target, cx);
                                    });
                                })
                            })
                            .child(value),
                    )
                })
                .when_some(animation_button, |this, button| this.child(button))
                .when_some(binding_button, |this, button| this.child(button)),
        );
        (row, is_bound)
    }

    /// Full labeled row for a top-level scalar: item/effect properties,
    /// scalar rows, and scene argument values.
    pub(in crate::ui::property_inspector) fn scalar_full_row(
        control: &Control,
        ctx: &RenderCtx,
    ) -> Option<PropertyRow> {
        let mut row = match control {
            Control::Number(number) => {
                let common = &number.common;
                let input = ctx.store.text(&common.id)?;
                Some(Self::number_full_row(
                    common,
                    &number.spec,
                    &input,
                    common.animation_enabled,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Text(text) => {
                let common = &text.common;
                let input = ctx.store.text(&common.id)?;
                Some(Self::text_full_row(
                    common,
                    text.multiline,
                    &input,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Bool(boolean) => {
                let common = &boolean;
                let PropertyValue::Bool(value) = common.value else {
                    return None;
                };
                Some(Self::toggle_full_row(
                    common,
                    value,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Choice(choice) => {
                let common = &choice.common;
                let PropertyValue::Enum(current) = common.value else {
                    return None;
                };
                Some(Self::dropdown_full_row(
                    common,
                    current,
                    &choice.options,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Color(color) => {
                let common = &color;
                let picker = ctx.store.color(&common.id)?;
                Some(Self::color_full_row(
                    common,
                    &picker,
                    common.animation_enabled,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::File(file) => Some(Self::file_row(file, ctx, false)),
            Control::Group { .. } => None,
        }?;
        let common = control.common()?;
        if common.target.effect_id.is_some()
            || !ctx.scene_overrides.contains(&common.target.property_id)
        {
            return Some(row);
        }
        let inspector = ctx.inspector.clone();
        let address = common.target.clone();
        let disabled = common.read_only
            || common
                .binding
                .as_ref()
                .is_some_and(|binding| binding.connected.is_some());
        row.content = div()
            .w_full()
            .flex()
            .items_start()
            .gap_1()
            .child(div().flex_1().min_w_0().child(row.content))
            .child(
                Button::new(SharedString::from(format!("reset-{:?}", common.target)))
                    .small()
                    .compact()
                    .ghost()
                    .icon(IconName::Undo)
                    .tooltip(t!("inspector.use_default").to_string())
                    .disabled(disabled)
                    .on_click(move |_, _, cx| {
                        inspector.update(cx, |inspector, cx| {
                            inspector.reset_property(address.clone(), cx)
                        });
                    }),
            );
        Some(row)
    }

    /// Compact scalar row for tuple children and array elements.
    /// Returns each row with whether its scene argument is connected.
    pub(super) fn scalar_compact_row(
        control: &Control,
        ctx: &RenderCtx,
    ) -> Option<(PropertyRow, bool)> {
        match control {
            Control::Number(number) => {
                let common = &number.common;
                let input = ctx.store.text(&common.id)?;
                Some(Self::number_compact_row(common, &number.spec, &input, ctx))
            }
            Control::Text(text) => {
                let common = &text.common;
                let input = ctx.store.text(&common.id)?;
                Some(Self::text_compact_row(
                    common,
                    text.multiline,
                    &input,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Bool(boolean) => {
                let common = &boolean;
                let PropertyValue::Bool(value) = common.value else {
                    return None;
                };
                Some(Self::toggle_compact_row(
                    common,
                    value,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Choice(choice) => {
                let common = &choice.common;
                let PropertyValue::Enum(current) = common.value else {
                    return None;
                };
                Some(Self::dropdown_compact_row(
                    common,
                    current,
                    &choice.options,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Color(color) => {
                let common = &color;
                let picker = ctx.store.color(&common.id)?;
                Some(Self::color_compact_row(
                    common,
                    &picker,
                    common.animation_enabled,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::File(file) => Some((
                Self::file_row(file, ctx, true),
                file.binding
                    .as_ref()
                    .is_some_and(|binding| binding.connected.is_some()),
            )),
            Control::Group { .. } => None,
        }
    }

    /// Grouped tuple rendering: one property label with per-scalar rows.
    /// A lone child renders as a plain full row, matching single scalars.
    pub(in crate::ui::property_inspector) fn group_box(
        id: ControlId,
        label: String,
        children: &[Control],
        extensions: &[EditorControl],
        ctx: &RenderCtx,
    ) -> PropertyRow {
        if extensions.is_empty()
            && let [child] = children
            && let Some(row) = Self::scalar_full_row(child, ctx)
        {
            return row;
        }
        let rows = children
            .iter()
            .filter_map(|child| Self::scalar_compact_row(child, ctx).map(|(row, _)| row))
            .collect::<Vec<_>>();
        PropertyRow::new(
            div()
                .self_start()
                .child(Self::animation_container_label(
                    label,
                    children,
                    ctx,
                    AnimationLabelWidth::Content,
                    format!("group-label-{id:?}").into(),
                ))
                .into_any_element(),
            Self::property_grid()
                .gap_x_2()
                .children(extensions.iter().map(|extension| {
                    div()
                        .col_span_full()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_end()
                        .child(Self::editor_control(extension, ctx))
                }))
                .children(rows.into_iter().flat_map(PropertyRow::into_cells)),
        )
    }
}
