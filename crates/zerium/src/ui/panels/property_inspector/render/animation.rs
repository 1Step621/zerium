use ::ui::{
    Icon, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    menu::context_menu::ContextMenuExt as _,
};
use gpui::{Div, SharedString, div, prelude::*, px};
use rust_i18n::t;

use crate::ui::panels::property_inspector::{
    control::{Control, ControlMeta},
    rows::RenderCtx,
};

#[derive(Clone, Copy)]
pub(in crate::ui::panels::property_inspector) enum AnimationLabelWidth {
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
    let menu_inspector = ctx.inspector.clone();
    let menu_target = common.target.clone();
    animation_label_base(label.into(), width, focused, ctx)
        .id(id)
        .when(common.animatable, |this| {
            this.context_menu(move |menu, _, cx| {
                menu_inspector.update(cx, |inspector, cx| {
                    let menu = inspector.animation_timing_menu(menu, menu_target.clone(), cx);
                    inspector.property_reset_menu(menu.separator(), menu_target.clone(), cx)
                })
            })
        })
        .when(common.animation_enabled, |this| {
            this.cursor_pointer().on_click(move |_, _, cx| {
                inspector.update(cx, |inspector, cx| {
                    inspector.select_animation(&target, cx);
                });
            })
        })
        .into_any_element()
}

pub(in crate::ui::panels::property_inspector) fn animation_label_base(
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

pub(in crate::ui::panels::property_inspector) fn animation_property_label(
    label: impl Into<SharedString>,
    common: &ControlMeta,
    ctx: &RenderCtx,
) -> gpui::AnyElement {
    focused_animation_label(
        label,
        common,
        ctx,
        AnimationLabelWidth::Content,
        format!("property-label-{:?}", common.id()).into(),
    )
}

pub(in crate::ui::panels::property_inspector) fn animation_scalar_label(
    common: &ControlMeta,
    ctx: &RenderCtx,
) -> Option<gpui::AnyElement> {
    common.scalar_label.clone().map(|label| {
        focused_animation_label(
            label,
            common,
            ctx,
            AnimationLabelWidth::Content,
            format!("scalar-label-{:?}", common.id()).into(),
        )
    })
}

pub(in crate::ui::panels::property_inspector) fn animation_container_label(
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

pub(in crate::ui::panels::property_inspector) fn control_contains_focused_animation(
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

pub(in crate::ui::panels::property_inspector) fn animation_toggle(
    common: &ControlMeta,
    ctx: &RenderCtx,
) -> Option<Button> {
    if !common.animatable || common.is_bound() {
        return None;
    }
    let target = common.target.clone();
    let inspector = ctx.inspector.clone();
    let enabled = common.animation_enabled;
    let tooltip = if enabled {
        t!("rows.unanimate")
    } else {
        t!("rows.animate")
    };
    Some(
        Button::new(SharedString::from(format!("toggle-animation-{target:?}")))
            .icon(Icon::new(IconName::Keyframe))
            .small()
            .compact()
            .ghost()
            .selected(enabled)
            .tooltip(tooltip)
            .tab_stop(!common.read_only)
            .when(!common.read_only, |button| {
                button.on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    inspector.update(cx, |inspector, cx| {
                        inspector.set_animation_enabled(&target, !enabled, window, cx);
                    });
                })
            }),
    )
}

pub(in crate::ui::panels::property_inspector) fn animation_value_inputs(
    inputs: impl IntoIterator<Item = gpui::AnyElement>,
    foreground: gpui::Hsla,
) -> Option<gpui::AnyElement> {
    let mut inputs = inputs.into_iter();
    let start = inputs.next()?;
    let Some(end) = inputs.next() else {
        return Some(start);
    };
    if inputs.next().is_some() {
        return None;
    }
    Some(
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
            .into_any_element(),
    )
}
