use rust_i18n::t;

use super::*;

impl PropertyInspector {
    pub(super) fn file_row(
        common: &LeafControl,
        render: &RenderCtx<'_>,
        compact: bool,
    ) -> gpui::AnyElement {
        let effect_id = common.target.effect_id;
        let property_id = common.target.property_id.clone();
        let address = common.target.address(render.item_id);
        let disabled = render.selecting_file
            || common.read_only
            || common
                .binding
                .as_ref()
                .is_some_and(|binding| binding.connected.is_some());
        let has_override =
            effect_id.is_none() && render.scene_file_overrides.contains(&property_id);
        let inspector = render.inspector.clone();

        div()
            .w_full()
            .flex()
            .items_start()
            .gap_3()
            .when(!compact, |this| {
                this.child(Self::property_label_column(common.label.clone()))
            })
            .when_some(
                compact
                    .then(|| Self::animation_scalar_label(common, render))
                    .flatten(),
                |this, label| this.child(label),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(crate::ui::file_input::file_picker(
                        &render.file_input,
                        SharedString::from(format!("file-{:?}", common.id)),
                        crate::ui::file_input::FileTarget::Property(address.clone()),
                        common.value.file(),
                        common.mixed,
                        render.selecting_file,
                        disabled,
                    ))
                    .when(has_override, |this| {
                        this.child(
                            Button::new(SharedString::from(format!("reset-file-{:?}", common.id)))
                                .small()
                                .compact()
                                .ghost()
                                .label(t!("inspector.use_default").to_string())
                                .disabled(disabled)
                                .on_click(move |_, _, cx| {
                                    inspector.update(cx, |inspector, cx| {
                                        inspector.reset_property(address.clone(), cx)
                                    })
                                }),
                        )
                    }),
            )
            .when_some(common.binding.clone(), |row, binding| {
                row.child(Self::scene_binding_button(
                    binding,
                    &render.inspector,
                    SharedString::from(format!("file-binding-{:?}", common.id)),
                ))
            })
            .into_any_element()
    }

    pub(super) fn number_full_row(
        common: &LeafControl,
        spec: &NumericInputSpec,
        input: &Entity<InputState>,
        animation_enabled: bool,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> Div {
        let mut label = common.label.clone();
        if let Some(scalar_label) = common.scalar_label.clone() {
            label = format!("{label} {scalar_label}");
        }
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
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
            )
        });
        let value_input = Self::number_editor(common, spec, input, common.read_only, ctx);
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::animation_property_label(label, common, ctx))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(!is_bound, |this| {
                        this.child(
                            div()
                                .id(SharedString::from(common.target.key.to_string()))
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
    ) -> (gpui::AnyElement, bool) {
        let coordinate_animation_enabled = common.animation_enabled;
        let binding = common.binding.clone();
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
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
        let row = div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when(disabled, |this| {
                this.text_color(ctx.colors.muted_foreground)
            })
            .when(!common.read_only, |this| {
                this.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    select_inspector.update(cx, |inspector, cx| {
                        inspector.select_animation(&select_target, cx);
                    });
                })
            })
            .when_some(Self::animation_scalar_label(common, ctx), |this, label| {
                this.child(label)
            })
            .when(!is_bound, |this| {
                this.child(div().min_w_0().flex().flex_1().child(value_input))
            })
            .when_some(animation_button, |this, button| this.child(button))
            .when_some(binding_button, |this, button| this.child(button))
            .into_any_element();
        (row, is_bound)
    }

    pub(super) fn text_full_row(
        common: &LeafControl,
        multiline: bool,
        input: &Entity<InputState>,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> Div {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
            )
        });
        Self::labeled_row(
            Self::animation_property_label(common.label.clone(), common, ctx),
            div()
                .min_w_0()
                .flex_1()
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
    ) -> (gpui::AnyElement, bool) {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
            )
        });
        let row = Self::compact_row(
            Self::animation_scalar_label(common, ctx),
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .gap_1()
                .when(!is_bound, |this| {
                    this.child(Self::text_editor(input, multiline, common.read_only))
                })
                .when_some(binding_button, |this, button| this.child(button)),
        )
        .into_any_element();
        (row, is_bound)
    }

    pub(super) fn toggle_full_row(
        common: &LeafControl,
        value: bool,
        mixed: bool,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> gpui::AnyElement {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
            )
        });
        let (switch, mixed) = Self::bool_switch(
            &common.target,
            value,
            mixed,
            common.read_only,
            &ctx.inspector,
        );
        Self::labeled_row(
            Self::animation_property_label(common.label.clone(), common, ctx),
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .gap_2()
                .when(!is_bound && mixed, |this| {
                    this.child(div().text_xs().child(t!("rows.mixed").to_string()))
                })
                .when(!is_bound, |this| this.child(switch))
                .when_some(binding_button, |this, button| this.child(button)),
        )
        .into_any_element()
    }

    pub(super) fn toggle_compact_row(
        common: &LeafControl,
        value: bool,
        mixed: bool,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> (gpui::AnyElement, bool) {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
            )
        });
        let (switch, mixed) = Self::bool_switch(
            &common.target,
            value,
            mixed,
            common.read_only,
            &ctx.inspector,
        );
        let row = div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when_some(Self::animation_scalar_label(common, ctx), |this, label| {
                this.child(label)
            })
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(!is_bound && mixed, |this| {
                        this.child(div().text_xs().child(t!("rows.mixed").to_string()))
                    })
                    .when(!is_bound, |this| this.child(switch))
                    .when_some(binding_button, |this, button| this.child(button)),
            )
            .into_any_element();
        (row, is_bound)
    }

    pub(super) fn dropdown_full_row(
        common: &LeafControl,
        current: u32,
        options: &[(String, u32)],
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> gpui::AnyElement {
        let bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-{}", common.target.key)),
            )
        });
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::animation_property_label(
                common.label.clone(),
                common,
                ctx,
            ))
            .when(!bound, |row| {
                row.child(Self::choice_dropdown(
                    &common.target,
                    current,
                    options,
                    common.read_only,
                    &ctx.inspector,
                ))
            })
            .when_some(binding_button, |row, button| row.child(button))
            .into_any_element()
    }

    pub(super) fn dropdown_compact_row(
        common: &LeafControl,
        current: u32,
        options: &[(String, u32)],
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> (gpui::AnyElement, bool) {
        let bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-{}", common.target.key)),
            )
        });
        let row = div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when_some(Self::animation_scalar_label(common, ctx), |this, label| {
                this.child(label)
            })
            .child(
                div()
                    .min_w_0()
                    .flex_1()
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
            )
            .into_any_element();
        (row, bound)
    }

    pub(super) fn color_full_row(
        common: &LeafControl,
        picker: &Entity<ColorPickerState>,
        animation_enabled: bool,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> gpui::AnyElement {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
            )
        });
        let animation_button = (common.animatable && !is_bound).then(|| {
            Self::color_animation_toggle(&common.target, animation_enabled, &ctx.inspector)
        });
        let value = Self::color_editor(common, picker, ctx);
        let select_inspector = ctx.inspector.clone();
        let select_target = common.target.clone();
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::animation_property_label(
                common.label.clone(),
                common,
                ctx,
            ))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
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
            .into_any_element()
    }

    pub(super) fn color_compact_row(
        common: &LeafControl,
        picker: &Entity<ColorPickerState>,
        animation_enabled: bool,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> (gpui::AnyElement, bool) {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
            )
        });
        let animation_button = (common.animatable && !is_bound).then(|| {
            Self::color_animation_toggle(&common.target, animation_enabled, &ctx.inspector)
        });
        let value = Self::color_editor(common, picker, ctx);
        let select_inspector = ctx.inspector.clone();
        let select_target = common.target.clone();
        let row = div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when_some(Self::animation_scalar_label(common, ctx), |this, label| {
                this.child(label)
            })
            .child(
                div()
                    .min_w_0()
                    .flex_1()
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
            .into_any_element();
        (row, is_bound)
    }

    /// Full labeled row for a top-level scalar: item/effect properties,
    /// scalar rows, and scene argument values.
    pub(in crate::ui::property_inspector) fn scalar_full_row(
        control: &Control,
        ctx: &RenderCtx,
    ) -> Option<gpui::AnyElement> {
        match control {
            Control::Number(number) => {
                let common = &number.common;
                let input = ctx.store.text(&common.id)?;
                Some(
                    Self::number_full_row(
                        common,
                        &number.spec,
                        &input,
                        common.animation_enabled,
                        common.binding.clone(),
                        ctx,
                    )
                    .into_any_element(),
                )
            }
            Control::Text(text) => {
                let common = &text.common;
                let input = ctx.store.text(&common.id)?;
                Some(
                    Self::text_full_row(
                        common,
                        text.multiline,
                        &input,
                        common.binding.clone(),
                        ctx,
                    )
                    .into_any_element(),
                )
            }
            Control::Bool(boolean) => {
                let common = &boolean;
                let PropertyValue::Bool(value) = common.value else {
                    return None;
                };
                Some(Self::toggle_full_row(
                    common,
                    value,
                    common.mixed,
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
        }
    }

    /// Compact scalar row for tuple children and array elements.
    /// Returns each row with whether its scene argument is connected.
    pub(super) fn scalar_compact_row(
        control: &Control,
        ctx: &RenderCtx,
    ) -> Option<(gpui::AnyElement, bool)> {
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
                    common.mixed,
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
        label: String,
        children: &[Control],
        extensions: &[EditorControl],
        ctx: &RenderCtx,
    ) -> gpui::AnyElement {
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
        div()
            .w_full()
            .flex()
            .items_start()
            .gap_3()
            .child(Self::animation_container_label(
                label,
                children,
                ctx,
                AnimationLabelWidth::Fixed(Self::PROPERTY_LABEL_WIDTH),
                "group-label",
            ))
            .child(
                div()
                    .w_0()
                    .min_w_0()
                    .flex()
                    .flex_1()
                    .flex_col()
                    .gap_1()
                    .children(extensions.iter().map(|extension| {
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_end()
                            .child(Self::editor_control(extension, ctx))
                    }))
                    .children(rows),
            )
            .into_any_element()
    }
}
