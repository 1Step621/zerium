use crate::ui::{
    components::input::number::value::NumericInputSpec,
    panels::property_inspector::{
        ControlId, PropertyInspector,
        control::{ControlMeta, NumberControl},
        render::{
            animation::{
                animation_property_label, animation_scalar_label, animation_stop_inputs,
                keyframe_base,
            },
            binding::control_binding_button,
        },
    },
};

use ::ui::{
    Disableable as _, Sizable as _,
    button::Button,
    input::{InputState, NumberInput},
};
use gpui::{Entity, MouseButton, SharedString, div, prelude::*};

use zerium_core::timeline::PropertyAddress;

use super::{PropertyRow, RenderCtx};
use rust_i18n::t;

pub(in crate::ui::panels::property_inspector) fn number_full_row(
    common: &ControlMeta,
    spec: &NumericInputSpec,
    input: &Entity<InputState>,
    ctx: &RenderCtx,
) -> PropertyRow {
    let is_bound = common.is_bound();
    let select_inspector = ctx.inspector.clone();
    let select_target = common.target.clone();
    let animation_button = (common.animatable && !is_bound).then(|| {
        number_animation_toggle(
            &common.target,
            common.animation_enabled,
            if common.animation_enabled {
                t!("rows.unanimate").to_string()
            } else {
                t!("rows.animate_axis").to_string()
            },
            &ctx.inspector,
        )
    });
    let publish_button = control_binding_button(common, ctx);
    let value_input = number_editor(common, spec, input, common.read_only, ctx);
    PropertyRow::new(
        animation_property_label(common.label.clone(), common, ctx),
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

pub(in crate::ui::panels::property_inspector) fn number_compact_row(
    common: &ControlMeta,
    spec: &NumericInputSpec,
    input: &Entity<InputState>,
    ctx: &RenderCtx,
) -> (PropertyRow, bool) {
    let coordinate_animation_enabled = common.animation_enabled;
    let is_bound = common.is_bound();
    let binding_button = control_binding_button(common, ctx);
    let disabled = common.read_only;
    let animation_visible = coordinate_animation_enabled;
    let animation_button = (common.animatable && !is_bound).then(|| {
        coordinate_animation_toggle(
            &common.target,
            coordinate_animation_enabled,
            common.read_only,
            animation_visible,
            &ctx.inspector,
        )
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
    let mut stop_inputs = common
        .animation_stops
        .iter()
        .filter_map(|stop| {
            let input = ctx.store.inputs.text(&stop.id)?;
            Some(draggable_number_input(
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
        return animation_stop_inputs(start, end, ctx.colors.muted_foreground).into_any_element();
    }
    draggable_number_input(&common.target, spec, input, &common.id(), disabled, ctx)
}

pub(in crate::ui::panels::property_inspector) fn element_number(
    number: &NumberControl,
    ctx: &RenderCtx,
) -> Option<(gpui::AnyElement, bool)> {
    let common = &number.common;
    let spec = &number.spec;
    let input = ctx.store.inputs.text(&common.id())?;
    let animation_enabled = common.animation_enabled;
    let component_is_bound = common.is_bound();
    let component_binding_button = control_binding_button(common, ctx);
    let value_input = number_editor(common, spec, &input, common.read_only, ctx);
    let animation_button = (common.animatable && !component_is_bound).then(|| {
        number_animation_toggle(
            &common.target,
            animation_enabled,
            if animation_enabled {
                t!("rows.unanimate_axis").to_string()
            } else {
                t!("rows.animate_element").to_string()
            },
            &ctx.inspector,
        )
    });
    let select_inspector = ctx.inspector.clone();
    let select_target = common.target.clone();
    let row = div()
        .min_w_0()
        .w_full()
        .flex()
        .items_center()
        .gap_1()
        .when(!component_is_bound, |this| {
            this.child(
                div()
                    .id(SharedString::from(format!("{:?}", common.target)))
                    .min_w_0()
                    .flex()
                    .flex_1()
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        select_inspector.update(cx, |inspector, cx| {
                            inspector.select_animation(&select_target, cx);
                        });
                    })
                    .child(value_input),
            )
        })
        .when_some(animation_button, |this, button| this.child(button))
        .when_some(component_binding_button, |this, button| this.child(button))
        .into_any_element();
    Some((row, component_is_bound))
}

fn number_animation_toggle(
    target: &PropertyAddress,
    enabled: bool,
    tooltip: String,
    inspector: &Entity<PropertyInspector>,
) -> Button {
    let inspector = inspector.clone();
    let target = target.clone();
    keyframe_base(
        SharedString::from(format!("toggle-animation-{:?}", target)),
        enabled,
        tooltip,
    )
    .on_click(move |_, window, cx| {
        inspector.update(cx, |inspector, cx| {
            inspector.set_animation_enabled(&target, !enabled, window, cx);
        });
    })
}

fn coordinate_animation_toggle(
    target: &PropertyAddress,
    enabled: bool,
    disabled: bool,
    visible: bool,
    inspector: &Entity<PropertyInspector>,
) -> Button {
    let inspector = inspector.clone();
    let target = target.clone();
    keyframe_base(
        SharedString::from(format!("toggle-animation-{:?}", target)),
        visible,
        if disabled {
            t!("rows.aspect_auto").to_string()
        } else if enabled {
            t!("rows.unanimate_axis").to_string()
        } else {
            t!("rows.animate_axis").to_string()
        },
    )
    .tab_stop(!disabled)
    .when(!disabled, |button| {
        button.on_click(move |_, window, cx| {
            cx.stop_propagation();
            inspector.update(cx, |inspector, cx| {
                inspector.set_animation_enabled(&target, !enabled, window, cx);
            });
        })
    })
}
