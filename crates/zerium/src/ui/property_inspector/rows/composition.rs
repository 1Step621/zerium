//! Compose scalar and grouped properties from their value-specific rows.
use ::ui::{
    Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    switch::Switch,
};
use gpui::{Div, SharedString, div, prelude::*};
use rust_i18n::t;

use crate::ui::property_inspector::control::{Control, EditorControl};
use crate::ui::property_inspector::rows::{PropertyRow, RenderCtx, animation::AnimationLabelWidth};
use crate::ui::property_inspector::{ControlId, PropertyInspector};

impl PropertyInspector {
    /// Full labeled row for a top-level scalar: item/effect properties,
    /// scalar rows, and scene argument values.
    pub(in crate::ui::property_inspector) fn scalar_full_row(
        control: &Control,
        ctx: &RenderCtx,
    ) -> Option<PropertyRow> {
        let mut row = match control {
            Control::Number(number) => {
                let common = &number.common;
                let input = ctx.store.text(&common.id)?;
                let mut row = Self::number_full_row(
                    common,
                    &number.spec,
                    &input,
                    common.animation_enabled,
                    common.binding.clone(),
                    ctx,
                );
                if let Some(value) = &number.applied_value {
                    row.content = div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(row.content)
                        .child(
                            div()
                                .text_sm()
                                .text_color(ctx.colors.muted_foreground)
                                .child(t!("args.applied_value", value = value).to_string()),
                        );
                }
                Some(row)
            }
            Control::Text(text) => {
                let common = &text.common;
                let input = ctx.store.text(&common.id)?;
                Some(Self::text_full_row(
                    common,
                    text.multiline,
                    &input,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Bool(boolean) => {
                let common = &boolean.common;
                let value = boolean.value;
                Some(Self::toggle_full_row(
                    common,
                    value,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Choice(choice) => {
                let common = &choice.common;
                let current = choice.value;
                Some(Self::dropdown_full_row(
                    common,
                    current,
                    &choice.options,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Color(color) => {
                let common = &color.common;
                let picker = ctx.store.color(&common.id)?;
                Some(Self::color_full_row(
                    color,
                    &picker,
                    common.animation_enabled,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::File(file) => Some(Self::file_row(file, ctx, false)),
            Control::Tuple(_) | Control::Array(_) | Control::Effect(_) => None,
        }?;
        let common = control.common()?;
        if common.target.effect_id.is_some()
            || !ctx.scene_overrides.contains(&common.target.property_id)
        {
            return Some(row);
        }
        let inspector = ctx.inspector.clone();
        let address = common.target.clone();
        let disabled = common.read_only
            || common
                .binding
                .as_ref()
                .is_some_and(|binding| binding.connected.is_some());
        row.content = div()
            .w_full()
            .flex()
            .items_start()
            .gap_1()
            .child(div().flex_1().min_w_0().child(row.content))
            .child(
                Button::new(SharedString::from(format!("reset-{:?}", common.target)))
                    .small()
                    .compact()
                    .ghost()
                    .icon(IconName::Undo)
                    .tooltip(t!("inspector.use_default").to_string())
                    .disabled(disabled)
                    .on_click(move |_, _, cx| {
                        inspector.update(cx, |inspector, cx| {
                            inspector.reset_property(address.clone(), cx)
                        });
                    }),
            );
        Some(row)
    }

    /// Compact scalar row for tuple children and array elements.
    /// Returns each row with whether its scene argument is connected.
    pub(super) fn scalar_compact_row(
        control: &Control,
        ctx: &RenderCtx,
    ) -> Option<(PropertyRow, bool)> {
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
                let common = &boolean.common;
                let value = boolean.value;
                Some(Self::toggle_compact_row(
                    common,
                    value,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Choice(choice) => {
                let common = &choice.common;
                let current = choice.value;
                Some(Self::dropdown_compact_row(
                    common,
                    current,
                    &choice.options,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Color(color) => {
                let common = &color.common;
                let picker = ctx.store.color(&common.id)?;
                Some(Self::color_compact_row(
                    color,
                    &picker,
                    common.animation_enabled,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::File(file) => Some((
                Self::file_row(file, ctx, true),
                file.common
                    .binding
                    .as_ref()
                    .is_some_and(|binding| binding.connected.is_some()),
            )),
            Control::Tuple(_) | Control::Array(_) | Control::Effect(_) => None,
        }
    }

    /// Grouped tuple rendering: one property label with per-scalar rows.
    /// A lone child renders as a plain full row, matching single scalars.
    pub(in crate::ui::property_inspector) fn group_box(
        id: ControlId,
        label: String,
        children: &[Control],
        extensions: &[EditorControl],
        ctx: &RenderCtx,
    ) -> PropertyRow {
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
        PropertyRow::new(
            div()
                .self_start()
                .child(Self::animation_container_label(
                    label,
                    children,
                    ctx,
                    AnimationLabelWidth::Content,
                    format!("group-label-{id:?}").into(),
                ))
                .into_any_element(),
            Self::property_grid()
                .gap_x_2()
                .children(extensions.iter().map(|extension| {
                    div()
                        .col_span_full()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_end()
                        .child(Self::editor_control(extension, ctx))
                }))
                .children(rows.into_iter().flat_map(PropertyRow::into_cells)),
        )
    }

    fn editor_control(control: &EditorControl, ctx: &RenderCtx) -> Div {
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
}
