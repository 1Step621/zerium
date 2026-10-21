use crate::ui::property_inspector::rows::{
    animation_property_label, animation_scalar_label, scene_binding_button,
};
use gpui::{SharedString, div, prelude::*};

use crate::ui::property_inspector::control::ValueControl;
use crate::ui::property_inspector::rows::{PropertyRow, RenderCtx};

pub(in crate::ui::property_inspector) fn file_row(
    file: &ValueControl<Option<std::path::PathBuf>>,
    render: &RenderCtx<'_>,
    compact: bool,
) -> PropertyRow {
    let common = &file.common;
    let address = common.target.clone();
    let disabled = render.selecting_file
        || common.read_only
        || common
            .binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());

    let label = if compact {
        animation_scalar_label(common, render)
    } else {
        Some(animation_property_label(
            common.label.clone(),
            common,
            render,
        ))
    };
    PropertyRow::new(
        label,
        div()
            .min_w_0()
            .flex()
            .items_start()
            .gap_1()
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(crate::ui::file_input::file_picker(
                        &render.file_input,
                        SharedString::from(format!("file-{:?}", common.id)),
                        crate::ui::file_input::FileTarget::Property(address),
                        file.value.as_deref(),
                        render.selecting_file,
                        disabled,
                    )),
            )
            .when_some(common.binding.clone(), |row, binding| {
                row.child(scene_binding_button(
                    binding,
                    &render.inspector,
                    SharedString::from(format!("file-binding-{:?}", common.id)),
                ))
            }),
    )
}
