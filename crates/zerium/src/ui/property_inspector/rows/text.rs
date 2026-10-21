use crate::ui::property_inspector::rows::{
    animation_property_label, animation_scalar_label, scene_binding_button,
};
use ::ui::{
    Sizable as _,
    input::{Input, InputState},
};
use gpui::{Entity, SharedString, div, prelude::*, px};

use crate::ui::property_inspector::SceneFieldBinding;
use crate::ui::property_inspector::control::ControlMeta;
use crate::ui::property_inspector::rows::{PropertyRow, RenderCtx};

pub(in crate::ui::property_inspector) fn text_full_row(
    common: &ControlMeta,
    multiline: bool,
    input: &Entity<InputState>,
    binding: Option<SceneFieldBinding>,
    ctx: &RenderCtx,
) -> PropertyRow {
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
    PropertyRow::new(
        animation_property_label(common.label.clone(), common, ctx),
        div()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .when(!is_bound, |this| {
                this.child(text_editor(input, multiline, common.read_only))
            })
            .when_some(binding_button, |this, button| this.child(button)),
    )
}

pub(in crate::ui::property_inspector) fn text_compact_row(
    common: &ControlMeta,
    multiline: bool,
    input: &Entity<InputState>,
    binding: Option<SceneFieldBinding>,
    ctx: &RenderCtx,
) -> (PropertyRow, bool) {
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
    let row = PropertyRow::new(
        animation_scalar_label(common, ctx),
        div()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .when(!is_bound, |this| {
                this.child(text_editor(input, multiline, common.read_only))
            })
            .when_some(binding_button, |this, button| this.child(button)),
    );
    (row, is_bound)
}

fn text_editor(input: &Entity<InputState>, multiline: bool, read_only: bool) -> impl IntoElement {
    Input::new(input)
        .small()
        .w_full()
        .disabled(read_only)
        .when(multiline, |input| input.h(px(72.)))
}
