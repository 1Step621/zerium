use crate::ui::{
    components::input::number::value::NumericInputSpec,
    panels::property_inspector::{
        ControlId,
        control::{ControlMeta, NumberControl},
        render::{
            animation::{
                animation_property_label, animation_scalar_label, animation_toggle,
                animation_value_inputs,
            },
            binding::control_binding_button,
        },
    },
};

use ::ui::{
    Disableable as _, Sizable as _,
    input::{InputState, NumberInput},
};
use gpui::{Div, Entity, MouseButton, SharedString, div, prelude::*};
use rust_i18n::t;

use zerium_core::timeline::PropertyAddress;

use super::{PropertyRow, RenderCtx};

pub(in crate::ui::panels::property_inspector) fn number_full_row(
    common: &ControlMeta,
    spec: &NumericInputSpec,
    input: &Entity<InputState>,
    ctx: &RenderCtx,
) -> PropertyRow {
    PropertyRow::new(
        animation_property_label(common.label.clone(), common, ctx),
        number_value_row(common, spec, input, ctx),
    )
}

fn number_value_row(
    common: &ControlMeta,
    spec: &NumericInputSpec,
    input: &Entity<InputState>,
    ctx: &RenderCtx,
) -> Div {
    let select_inspector = ctx.inspector.clone();
    let select_target = common.target.clone();
    div()
        .min_w_0()
        .flex()
        .items_center()
        .gap_1()
        .when(!common.is_bound(), |this| {
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
                    .child(number_editor(common, spec, input, common.read_only, ctx)),
            )
        })
        .when_some(animation_toggle(common, ctx), |this, button| {
            this.child(button)
        })
        .when_some(control_binding_button(common, ctx), |this, button| {
            this.child(button)
        })
}

pub(in crate::ui::panels::property_inspector) fn number_compact_row(
    common: &ControlMeta,
    spec: &NumericInputSpec,
    input: &Entity<InputState>,
    ctx: &RenderCtx,
) -> (PropertyRow, bool) {
    let is_bound = common.is_bound();
    let binding_button = control_binding_button(common, ctx);
    let disabled = common.read_only;
    let animation_button = animation_toggle(common, ctx).map(|button| {
        button.when(disabled, |button| {
            button.tooltip(t!("rows.aspect_auto").to_string())
        })
    });
    let value_input = number_editor(common, spec, input, disabled, ctx);
    let select_inspector = ctx.inspector.clone();
    let select_target = common.target.clone();
    let row = PropertyRow::new(
        animation_scalar_label(common, ctx),
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

fn draggable_number_input(
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
    crate::ui::components::input::number::number_input_drag(
        &ctx.inspector,
        input,
        value_input,
        disabled,
        move |this, event, cx| this.prepare_value_drag(&target, &spec, &prepare_id, event, cx),
    )
}

fn number_editor(
    common: &ControlMeta,
    spec: &NumericInputSpec,
    input: &Entity<InputState>,
    disabled: bool,
    ctx: &RenderCtx,
) -> gpui::AnyElement {
    let stop_inputs = common.animation_stops.iter().filter_map(|stop| {
        let input = ctx.store.inputs.text(&stop.id)?;
        Some(draggable_number_input(
            &common.target,
            spec,
            &input,
            &stop.id,
            disabled,
            ctx,
        ))
    });
    animation_value_inputs(stop_inputs, ctx.colors.muted_foreground).unwrap_or_else(|| {
        draggable_number_input(&common.target, spec, input, &common.id(), disabled, ctx)
    })
}

pub(in crate::ui::panels::property_inspector) fn element_number(
    number: &NumberControl,
    ctx: &RenderCtx,
) -> Option<(gpui::AnyElement, bool)> {
    let common = &number.common;
    let input = ctx.store.inputs.text(&common.id())?;
    Some((
        number_value_row(common, &number.spec, &input, ctx)
            .w_full()
            .into_any_element(),
        common.is_bound(),
    ))
}
