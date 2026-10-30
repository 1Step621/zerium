use crate::ui::panels::{
    preview::color_input,
    property_inspector::{
        control::ValueControl,
        render::{
            animation::{animation_toggle, animation_value_inputs},
            binding::control_binding_button,
        },
    },
};
use ::ui::color_picker::ColorPickerState;
use gpui::{Entity, MouseButton, div, prelude::*, px};

use super::{PropertyRow, RenderCtx};

pub(in crate::ui::panels::property_inspector) fn color_row(
    color: &ValueControl<[f32; 4]>,
    picker: &Entity<ColorPickerState>,
    label: Option<gpui::AnyElement>,
    ctx: &RenderCtx,
) -> PropertyRow {
    let common = &color.common;
    let is_bound = common.is_bound();
    let binding_button = control_binding_button(common, ctx);
    let animation_button = animation_toggle(common, ctx);
    let value = color_editor(color, picker, ctx);
    let select_inspector = ctx.inspector.clone();
    let select_target = common.target.clone();
    PropertyRow::new(
        label,
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

fn color_editor(
    color: &ValueControl<[f32; 4]>,
    picker: &Entity<ColorPickerState>,
    ctx: &RenderCtx,
) -> gpui::AnyElement {
    let common = &color.common;
    if common.read_only {
        let color = crate::ui::components::input::color_to_hsla(color.value);
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
    let stop_inputs = common.animation_stops.iter().filter_map(|stop| {
        let picker = ctx.store.inputs.color(&stop.id)?;
        Some(color_input(&picker).into_any_element())
    });
    animation_value_inputs(stop_inputs, ctx.colors.muted_foreground)
        .unwrap_or_else(|| color_input(picker).into_any_element())
}
