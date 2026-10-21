use crate::ui::property_inspector::rows::{
    animation_property_label, animation_scalar_label, animation_stop_inputs, keyframe_base,
    scene_binding_button,
};
use ::ui::{
    Sizable as _,
    button::Button,
    color_picker::{ColorPicker, ColorPickerState},
};
use gpui::{Entity, MouseButton, SharedString, div, prelude::*, px};

use zerium_core::timeline::PropertyAddress;

use crate::ui::property_inspector::control::ValueControl;
use crate::ui::property_inspector::rows::{PropertyRow, RenderCtx};
use crate::ui::property_inspector::{PropertyInspector, SceneFieldBinding};
use rust_i18n::t;

pub(in crate::ui::property_inspector) fn color_full_row(
    color: &ValueControl<[f32; 4]>,
    picker: &Entity<ColorPickerState>,
    animation_enabled: bool,
    binding: Option<SceneFieldBinding>,
    ctx: &RenderCtx,
) -> PropertyRow {
    let common = &color.common;
    let is_bound = binding
        .as_ref()
        .is_some_and(|binding| binding.connected.is_some());
    let binding_button = binding.map(|binding| {
        scene_binding_button(
            binding,
            &ctx.inspector,
            SharedString::from(format!("bind-scene-argument-{:?}", common.target)),
        )
    });
    let animation_button = (common.animatable && !is_bound)
        .then(|| color_animation_toggle(&common.target, animation_enabled, &ctx.inspector));
    let value = color_editor(color, picker, ctx);
    let select_inspector = ctx.inspector.clone();
    let select_target = common.target.clone();
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

pub(in crate::ui::property_inspector) fn color_compact_row(
    color: &ValueControl<[f32; 4]>,
    picker: &Entity<ColorPickerState>,
    animation_enabled: bool,
    binding: Option<SceneFieldBinding>,
    ctx: &RenderCtx,
) -> (PropertyRow, bool) {
    let common = &color.common;
    let is_bound = binding
        .as_ref()
        .is_some_and(|binding| binding.connected.is_some());
    let binding_button = binding.map(|binding| {
        scene_binding_button(
            binding,
            &ctx.inspector,
            SharedString::from(format!("bind-scene-argument-{:?}", common.target)),
        )
    });
    let animation_button = (common.animatable && !is_bound)
        .then(|| color_animation_toggle(&common.target, animation_enabled, &ctx.inspector));
    let value = color_editor(color, picker, ctx);
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

fn color_editor(
    color: &ValueControl<[f32; 4]>,
    picker: &Entity<ColorPickerState>,
    ctx: &RenderCtx,
) -> gpui::AnyElement {
    let common = &color.common;
    if common.read_only {
        let color = crate::ui::input::color_to_hsla(color.value);
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
            let picker = ctx.store.inputs.color(&stop.id)?;
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
        return animation_stop_inputs(start, end, ctx.colors.muted_foreground).into_any_element();
    }
    ColorPicker::new(picker).small().w_full().into_any_element()
}

fn color_animation_toggle(
    target: &PropertyAddress,
    enabled: bool,
    inspector: &Entity<PropertyInspector>,
) -> Button {
    let inspector = inspector.clone();
    let target = target.clone();
    keyframe_base(
        SharedString::from(format!("toggle-animation-{:?}", target)),
        enabled,
        if enabled {
            t!("rows.unanimate").to_string()
        } else {
            t!("rows.animate_color").to_string()
        },
    )
    .on_click(move |_, window, cx| {
        inspector.update(cx, |inspector, cx| {
            inspector.set_animation_enabled(&target, !enabled, window, cx);
        });
    })
}
