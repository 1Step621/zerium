use crate::ui::panels::property_inspector::{
    control::FileControl, render::binding::control_binding_button,
};
use gpui::{SharedString, div, prelude::*};

use super::{PropertyRow, RenderCtx};

pub(in crate::ui::panels::property_inspector) fn file_row(
    file: &FileControl,
    render: &RenderCtx<'_>,
    label: Option<gpui::AnyElement>,
) -> PropertyRow {
    let common = &file.common;
    let address = common.target.clone();
    let disabled = render.selecting_file || common.read_only || common.is_bound();
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
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(crate::ui::components::input::file::file_picker(
                        &render.file_input,
                        SharedString::from(format!("file-{:?}", common.id())),
                        crate::ui::components::input::file::FileTarget::Property(address),
                        crate::ui::components::input::file::FilePickerOptions {
                            path: file.value.as_deref(),
                            directory: file.directory,
                            selecting: render.selecting_file,
                            disabled,
                        },
                    ))
                    .when_some(file.description.clone(), |column, description| {
                        column.child(
                            div()
                                .text_xs()
                                .text_color(render.colors.muted_foreground)
                                .child(description),
                        )
                    }),
            )
            .when_some(control_binding_button(common, render), |row, button| {
                row.child(button)
            }),
    )
}
