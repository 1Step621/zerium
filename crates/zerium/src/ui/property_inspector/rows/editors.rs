use rust_i18n::t;

use super::*;

impl PropertyInspector {
    pub(super) fn choice_dropdown(
        target: &PropertyAddress,
        current: u32,
        options: &[(String, u32)],
        read_only: bool,
        inspector: &Entity<Self>,
    ) -> impl IntoElement {
        let selected_label = options
            .iter()
            .find(|(_, value)| *value == current)
            .map(|(label, _)| label.clone())
            .unwrap_or_default();
        let options = options.to_vec();
        let inspector = inspector.clone();
        let target = target.clone();
        Button::new(SharedString::from(format!("{target:?}")))
            .small()
            .outline()
            .w_full()
            .disabled(read_only)
            .label(selected_label)
            .dropdown_caret(true)
            .popup_menu(move |menu, _, _| {
                options.iter().fold(menu, |menu, (label, value)| {
                    let inspector = inspector.clone();
                    let target = target.clone();
                    let value = *value;
                    menu.item(PopupMenuItem::new(label.clone()).on_click(move |_, _, cx| {
                        inspector.update(cx, |inspector, cx| {
                            let changed =
                                inspector.set_scalar(&target, PropertyValue::Enum(value), cx);
                            if changed {
                                cx.notify();
                            }
                        });
                    }))
                })
            })
    }

    pub(super) fn bool_switch(
        target: &PropertyAddress,
        value: bool,
        read_only: bool,
        inspector: &Entity<Self>,
    ) -> Switch {
        let checked = value;
        let inspector = inspector.clone();
        let target = target.clone();
        Switch::new(SharedString::from(format!("{target:?}")))
            .small()
            .checked(checked)
            .disabled(read_only)
            .tooltip(if checked {
                t!("edit.turn_off")
            } else {
                t!("edit.turn_on")
            })
            .on_click(move |checked, _, cx| {
                inspector.update(cx, |inspector, cx| {
                    let changed = inspector.set_scalar(&target, PropertyValue::Bool(*checked), cx);
                    if changed {
                        cx.notify();
                    }
                });
            })
    }

    pub(super) fn editor_control(control: &EditorControl, ctx: &RenderCtx) -> Div {
        match control {
            EditorControl::AspectRatioLock {
                address,
                locked,
                read_only,
            } => {
                let editor = ctx.editor.clone();
                let effect_id = address.effect_id;
                let item_id = address.item_id;
                let checked = *locked;
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(ctx.colors.muted_foreground)
                            .child(t!("edit.aspect_lock").to_string()),
                    )
                    .child(
                        Switch::new(SharedString::from(format!("aspect-ratio-lock-{address:?}")))
                            .small()
                            .checked(checked)
                            .disabled(*read_only)
                            .tooltip(if checked {
                                t!("edit.unlock_ratio")
                            } else {
                                t!("edit.lock_ratio")
                            })
                            .on_click(move |checked, _, cx| {
                                editor.update(cx, |editor, cx| {
                                    if editor
                                        .update_aspect_ratio_locked(item_id, effect_id, *checked)
                                    {
                                        cx.notify();
                                    }
                                });
                            }),
                    )
            }
        }
    }

    pub(super) fn draggable_number_input(
        target: &PropertyAddress,
        spec: &NumericInputSpec,
        input: &Entity<InputState>,
        input_id: &ControlId,
        disabled: bool,
        ctx: &RenderCtx,
    ) -> gpui::AnyElement {
        let value_input = NumberInput::new(input)
            .small()
            .min_w_0()
            .disabled(disabled)
            .suffix(div().text_sm().child(spec.suffix.clone()));

        let target = target.clone();
        let spec = spec.clone();
        let prepare_id = input_id.clone();
        crate::ui::number_input::number_input_drag(
            &ctx.inspector,
            input,
            value_input,
            disabled,
            move |this, event, cx| this.prepare_value_drag(&target, &spec, &prepare_id, event, cx),
        )
    }

    pub(super) fn number_editor(
        common: &LeafControl,
        spec: &NumericInputSpec,
        input: &Entity<InputState>,
        disabled: bool,
        ctx: &RenderCtx,
    ) -> gpui::AnyElement {
        let mut stop_inputs = common
            .animation_stops
            .iter()
            .filter_map(|stop| {
                let input = ctx.store.text(&stop.id)?;
                Some(Self::draggable_number_input(
                    &common.target,
                    spec,
                    &input,
                    &stop.id,
                    disabled,
                    ctx,
                ))
            })
            .collect::<Vec<_>>();
        if stop_inputs.len() == 1 {
            return stop_inputs.remove(0);
        }
        if stop_inputs.len() == 2 {
            let end = stop_inputs.pop().expect("two stop inputs");
            let start = stop_inputs.pop().expect("two stop inputs");
            return Self::animation_stop_inputs(start, end, ctx.colors.muted_foreground)
                .into_any_element();
        }
        Self::draggable_number_input(&common.target, spec, input, &common.id, disabled, ctx)
    }

    fn animation_stop_inputs(
        start: gpui::AnyElement,
        end: gpui::AnyElement,
        foreground: gpui::Hsla,
    ) -> Div {
        div()
            .min_w_0()
            .flex_1()
            .flex()
            .items_center()
            .gap_1()
            .child(div().w_0().min_w_0().flex_1().flex().child(start))
            .child(
                Icon::new(IconName::ArrowRight)
                    .xsmall()
                    .text_color(foreground),
            )
            .child(div().w_0().min_w_0().flex_1().flex().child(end))
    }

    pub(super) fn text_editor(
        input: &Entity<InputState>,
        multiline: bool,
        read_only: bool,
    ) -> impl IntoElement {
        Input::new(input)
            .small()
            .w_full()
            .disabled(read_only)
            .when(multiline, |input| input.h(px(72.)))
    }

    pub(super) fn color_editor(
        common: &LeafControl,
        picker: &Entity<ColorPickerState>,
        ctx: &RenderCtx,
    ) -> gpui::AnyElement {
        if common.read_only {
            let color = match common.value {
                PropertyValue::Color(color) => Self::color_to_hsla(color),
                _ => ctx.colors.background,
            };
            return div()
                .w_full()
                .h(px(28.))
                .flex()
                .items_center()
                .child(
                    div()
                        .size(px(24.))
                        .rounded_md()
                        .border_1()
                        .border_color(ctx.colors.border)
                        .bg(color)
                        .opacity(0.55),
                )
                .into_any_element();
        }
        let mut stop_inputs = common
            .animation_stops
            .iter()
            .filter_map(|stop| {
                let picker = ctx.store.color(&stop.id)?;
                Some(
                    ColorPicker::new(&picker)
                        .small()
                        .w_full()
                        .into_any_element(),
                )
            })
            .collect::<Vec<_>>();
        if stop_inputs.len() == 1 {
            return stop_inputs.remove(0);
        }
        if stop_inputs.len() == 2 {
            let end = stop_inputs.pop().expect("two stop inputs");
            let start = stop_inputs.pop().expect("two stop inputs");
            return Self::animation_stop_inputs(start, end, ctx.colors.muted_foreground)
                .into_any_element();
        }
        ColorPicker::new(picker).small().w_full().into_any_element()
    }
}
