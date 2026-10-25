//! Property rows grouped by the value being edited.
use super::{
    ControlId, PropertyInspector,
    control::{AspectRatioControl, Control},
    render::animation::{
        AnimationLabelWidth, animation_container_label, animation_property_label,
        animation_scalar_label,
    },
    state::ControlStore,
};
use crate::ui::components::input::file::FileInputController;
use ::ui::{
    Disableable as _, IconName, Sizable as _, ThemeColor,
    button::{Button, ButtonVariants as _},
    switch::Switch,
};
use gpui::{Div, Entity, SharedString, div, prelude::*};
use rust_i18n::t;
use zerium_core::timeline::{ItemId, PropertyAddress, TimelineEditor};

mod array;
mod choice;
mod color;
mod file;
mod number;
mod text;

pub(super) use self::{
    array::elements_section,
    choice::{dropdown_row, toggle_row},
    color::color_row,
    file::file_row,
    number::{element_number, number_compact_row, number_full_row},
    text::text_row,
};

pub(super) struct RenderCtx<'a> {
    pub colors: ThemeColor,
    pub editor: &'a Entity<TimelineEditor>,
    pub animation_address: Option<PropertyAddress>,
    pub inspector: Entity<PropertyInspector>,
    pub store: &'a ControlStore,
    pub font_names: &'a [String],
    pub item_id: ItemId,
    pub selecting_file: bool,
    pub file_input: Entity<FileInputController>,
    pub scene_overrides: std::collections::HashSet<String>,
}

pub(super) struct PropertyRow {
    label: Option<gpui::AnyElement>,
    content: Div,
}

impl PropertyRow {
    fn new(label: impl Into<Option<gpui::AnyElement>>, content: Div) -> Self {
        Self {
            label: label.into(),
            content,
        }
    }

    pub(super) fn into_cells(self) -> Vec<gpui::AnyElement> {
        let content = self.content.min_w_0().w_full();
        match self.label {
            Some(label) => vec![label, content.into_any_element()],
            None => vec![content.col_span_full().into_any_element()],
        }
    }
}

impl IntoElement for PropertyRow {
    type Element = Div;

    fn into_element(self) -> Div {
        property_grid().children(self.into_cells())
    }
}

pub(super) fn property_grid() -> Div {
    div()
        .grid()
        .grid_template_columns([
            gpui::GridTrackSize::FitContent(gpui::relative(0.3)),
            gpui::GridTrackSize::Fraction(1.),
        ])
        .min_w_0()
        .w_full()
        .items_center()
        .gap_x_3()
        .gap_y_1()
}

/// Full labeled row for a top-level scalar: item/effect properties,
/// scalar rows, and scene argument values.
pub(super) fn scalar_full_row(control: &Control, ctx: &RenderCtx) -> Option<PropertyRow> {
    let mut row = scalar_row(control, ctx, false)?;
    let common = control.common()?;
    if common.target.effect_id.is_some()
        || !ctx.scene_overrides.contains(&common.target.property_id)
    {
        return Some(row);
    }
    let inspector = ctx.inspector.clone();
    let address = common.target.clone();
    let disabled = common.read_only || common.is_bound();
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
    Some((
        scalar_row(control, ctx, true)?,
        control.common()?.is_bound(),
    ))
}

fn scalar_row(control: &Control, ctx: &RenderCtx, compact: bool) -> Option<PropertyRow> {
    let common = control.common()?;
    if let Control::Number(number) = control {
        let input = ctx.store.inputs.text(&common.id())?;
        if compact {
            return Some(number_compact_row(common, &number.spec, &input, ctx).0);
        }
        let mut row = number_full_row(common, &number.spec, &input, ctx);
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
        return Some(row);
    }
    let label = if compact {
        animation_scalar_label(common, ctx)
    } else {
        Some(animation_property_label(common.label.clone(), common, ctx))
    };
    Some(match control {
        Control::Text(text) => {
            let input = ctx.store.inputs.text(&common.id())?;
            text_row(common, text.multiline, &input, label, ctx)
        }
        Control::Bool(boolean) => toggle_row(common, boolean.value, label, ctx),
        Control::Choice(choice) => dropdown_row(common, choice.value, &choice.options, label, ctx),
        Control::Color(color) => {
            let picker = ctx.store.inputs.color(&common.id())?;
            color_row(color, &picker, label, ctx)
        }
        Control::File(file) => file_row(file, ctx, label),
        _ => return None,
    })
}

/// Grouped tuple rendering: one property label with per-scalar rows.
/// A lone child renders as a plain full row, matching single scalars.
pub(super) fn group_box(
    id: ControlId,
    label: String,
    children: &[Control],
    aspect_ratio_lock: Option<&AspectRatioControl>,
    ctx: &RenderCtx,
) -> PropertyRow {
    if aspect_ratio_lock.is_none()
        && let [child] = children
        && let Some(row) = scalar_full_row(child, ctx)
    {
        return row;
    }
    let rows = children
        .iter()
        .filter_map(|child| scalar_compact_row(child, ctx).map(|(row, _)| row))
        .collect::<Vec<_>>();
    PropertyRow::new(
        div()
            .self_start()
            .child(animation_container_label(
                label,
                children,
                ctx,
                AnimationLabelWidth::Content,
                format!("group-label-{id:?}").into(),
            ))
            .into_any_element(),
        property_grid()
            .gap_x_2()
            .children(aspect_ratio_lock.into_iter().map(|extension| {
                div()
                    .col_span_full()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(aspect_ratio_control(extension, ctx))
            }))
            .children(rows.into_iter().flat_map(PropertyRow::into_cells)),
    )
}

fn aspect_ratio_control(control: &AspectRatioControl, ctx: &RenderCtx) -> Div {
    let AspectRatioControl {
        address,
        locked,
        read_only,
    } = control;
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
                        if editor.update_aspect_ratio_locked(item_id, effect_id, *checked) {
                            cx.notify();
                        }
                    });
                }),
        )
}
