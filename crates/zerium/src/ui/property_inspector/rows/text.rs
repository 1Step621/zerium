use crate::ui::property_inspector::rows::control_binding_button;
use ::ui::{
    Sizable as _,
    input::{Input, InputState},
};
use gpui::{Entity, div, prelude::*, px};

use crate::ui::property_inspector::control::ControlMeta;
use crate::ui::property_inspector::rows::{PropertyRow, RenderCtx};

pub(in crate::ui::property_inspector) fn text_row(
    common: &ControlMeta,
    multiline: bool,
    input: &Entity<InputState>,
    label: Option<gpui::AnyElement>,
    ctx: &RenderCtx,
) -> PropertyRow {
    let is_bound = common.is_bound();
    let binding_button = control_binding_button(common, ctx);
    PropertyRow::new(
        label,
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

fn text_editor(input: &Entity<InputState>, multiline: bool, read_only: bool) -> impl IntoElement {
    Input::new(input)
        .small()
        .w_full()
        .disabled(read_only)
        .when(multiline, |input| input.h(px(72.)))
}
