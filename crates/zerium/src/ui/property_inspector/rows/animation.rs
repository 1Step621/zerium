use ::ui::{
    Icon, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
};
use gpui::{Div, SharedString, div, prelude::*, px};

use crate::ui::property_inspector::control::{Control, ControlMeta};
use crate::ui::property_inspector::rows::RenderCtx;

#[derive(Clone, Copy)]
pub(in crate::ui::property_inspector) enum AnimationLabelWidth {
    Content,
    Fill,
}

fn animation_address_is_focused(common: &ControlMeta, ctx: &RenderCtx) -> bool {
    ctx.animation_address.as_ref() == Some(&common.target)
}

fn focused_animation_label(
    label: impl Into<SharedString>,
    common: &ControlMeta,
    ctx: &RenderCtx,
    width: AnimationLabelWidth,
    id: SharedString,
) -> gpui::AnyElement {
    let target = common.target.clone();
    let focused = animation_address_is_focused(common, ctx);
    let inspector = ctx.inspector.clone();
    animation_label_base(label.into(), width, focused, ctx)
        .id(id)
        .when(common.animation_enabled, |this| {
            this.cursor_pointer().on_click(move |_, _, cx| {
                inspector.update(cx, |inspector, cx| {
                    inspector.select_animation(&target, cx);
                });
            })
        })
        .into_any_element()
}

pub(in crate::ui::property_inspector) fn animation_label_base(
    label: SharedString,
    width: AnimationLabelWidth,
    focused: bool,
    ctx: &RenderCtx,
) -> Div {
    let tooltip = label.clone();
    let text = div()
        .min_w_0()
        .flex_1()
        .text_sm()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .when(focused, |this| {
            this.text_color(ctx.colors.accent_foreground)
        })
        .child(label.replace(['\n', '\r'], " "));
    let label = div()
        .min_h(px(24.))
        .min_w_0()
        .flex()
        .items_center()
        .tooltip(move |window, cx| ::ui::tooltip::Tooltip::new(tooltip.clone()).build(window, cx))
        .child(text);
    match width {
        AnimationLabelWidth::Content => label.max_w(px(90.)),
        AnimationLabelWidth::Fill => label.w_0().flex_1(),
    }
}

pub(in crate::ui::property_inspector) fn animation_property_label(
    label: impl Into<SharedString>,
    common: &ControlMeta,
    ctx: &RenderCtx,
) -> gpui::AnyElement {
    focused_animation_label(
        label,
        common,
        ctx,
        AnimationLabelWidth::Content,
        format!("property-label-{:?}", common.id).into(),
    )
}

pub(in crate::ui::property_inspector) fn animation_scalar_label(
    common: &ControlMeta,
    ctx: &RenderCtx,
) -> Option<gpui::AnyElement> {
    common.scalar_label.clone().map(|label| {
        focused_animation_label(
            label,
            common,
            ctx,
            AnimationLabelWidth::Content,
            format!("scalar-label-{:?}", common.id).into(),
        )
    })
}

pub(in crate::ui::property_inspector) fn animation_container_label(
    label: impl Into<SharedString>,
    children: &[Control],
    ctx: &RenderCtx,
    width: AnimationLabelWidth,
    id: SharedString,
) -> gpui::AnyElement {
    if let [child] = children
        && let Some(common) = child.common()
    {
        return focused_animation_label(label, common, ctx, width, id);
    }
    let focused = children
        .iter()
        .any(|child| control_contains_focused_animation(child, ctx));
    animation_label_base(label.into(), width, focused, ctx)
        .id(id)
        .into_any_element()
}

pub(in crate::ui::property_inspector) fn control_contains_focused_animation(
    control: &Control,
    ctx: &RenderCtx,
) -> bool {
    control
        .common()
        .is_some_and(|common| animation_address_is_focused(common, ctx))
        || control
            .children()
            .any(|child| control_contains_focused_animation(child, ctx))
}

pub(in crate::ui::property_inspector) fn keyframe_base(
    id: SharedString,
    selected: bool,
    tooltip: String,
) -> Button {
    Button::new(id)
        .icon(Icon::new(IconName::Keyframe))
        .small()
        .compact()
        .ghost()
        .selected(selected)
        .tooltip(tooltip)
}

pub(in crate::ui::property_inspector) fn animation_stop_inputs(
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
