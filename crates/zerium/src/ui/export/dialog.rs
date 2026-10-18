use rust_i18n::t;

use std::path::{Path, PathBuf};

use ::ui::{
    ContextModal as _, Disableable as _, IconName, Sizable as _, StyledExt as _,
    button::Button,
    modal::{Modal, ModalButtonProps},
};

use gpui::{App, Context, SharedString, Window, div, prelude::*, px};

use crate::project_session::ProjectActivity;

use super::{ExportController, ExportState};

impl ExportController {
    pub(crate) fn open_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let editor = self.editor.read(cx);
        let frame_rate = editor.frame_rate();
        let resolution = editor.resolution();
        let frame_count = editor.end_frame_exclusive();
        let duration_label = format_duration(frame_rate.frame_to_seconds(frame_count));
        let frame_rate_label = format!("{:.2} fps", frame_rate.frames_per_second());
        let controller = cx.entity();
        let session_id = self.session.read(cx).id();
        window.open_modal(cx, move |modal: Modal, _, cx| {
            let export = controller.read(cx);
            let output = export.output_path(cx).display().to_string();
            let busy = export.is_busy();
            let destination_controller = controller.clone();
            let confirm_controller = controller.clone();
            modal
                .title(
                    div()
                        .font_family(crate::ui::theme::FONT_FAMILY)
                        .font_normal()
                        .child(t!("export.title").to_string()),
                )
                .width(px(560.))
                .confirm()
                .button_props(
                    ModalButtonProps::default()
                        .ok_text(t!("export.start").to_string())
                        .cancel_text(t!("common.cancel").to_string()),
                )
                .on_ok(move |_, window, cx| {
                    confirm_controller.update(cx, |controller, cx| {
                        if controller.is_busy()
                            || !controller.session.read(cx).is_current(session_id)
                        {
                            return false;
                        }
                        controller.request_export(window, cx);
                        true
                    })
                })
                .child(summary_row(
                    t!("export.destination").to_string(),
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().min_w_0().flex_1().truncate().child(output.clone()))
                        .child(
                            Button::new("export-destination")
                                .small()
                                .icon(IconName::Folder)
                                .tooltip(format!("{}\n{output}", t!("export.choose_destination")))
                                .disabled(busy)
                                .on_click(move |_, _, cx| {
                                    destination_controller.update(cx, |controller, cx| {
                                        if controller.session.read(cx).is_current(session_id) {
                                            controller.choose_output(cx);
                                        }
                                    });
                                }),
                        ),
                ))
                .child(summary_row(
                    t!("export.format").to_string(),
                    "MP4 / H.264 + AAC",
                ))
                .child(summary_row(
                    t!("export.resolution").to_string(),
                    format!("{} × {}", resolution.width(), resolution.height()),
                ))
                .child(summary_row(
                    t!("export.frame_rate").to_string(),
                    frame_rate_label.clone(),
                ))
                .child(summary_row(
                    t!("export.duration").to_string(),
                    duration_label.clone(),
                ))
        });
    }

    fn output_path(&self, cx: &App) -> PathBuf {
        if let Some(output) = &self.output {
            return output.clone();
        }
        if let Some(path) = self.session.read(cx).path() {
            return path.with_extension("mp4");
        }
        let initial_directory = directories::UserDirs::new()
            .map(|directories| {
                directories
                    .video_dir()
                    .unwrap_or(directories.home_dir())
                    .to_path_buf()
            })
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        initial_directory.join("output.mp4")
    }

    fn choose_output(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let output = self.output_path(cx);
        let receiver = cx.prompt_for_new_path(
            output.parent().unwrap_or(Path::new(".")),
            output.file_name().and_then(|name| name.to_str()),
        );
        let session = self.session.clone();
        let operation = session.update(cx, |session, cx| {
            let operation = session.begin(ProjectActivity::SelectFile);
            cx.notify();
            operation
        });
        let task = cx.spawn(async move |controller, cx| {
            let result = receiver
                .await
                .map_err(|error| t!("export.destination_picker_failed", error = error).to_string())
                .and_then(|result| {
                    result.map_err(|error| {
                        t!("export.select_destination_failed", error = error).to_string()
                    })
                });
            let _ = controller.update(cx, |controller, cx| {
                if !matches!(&controller.state, ExportState::ChoosingDestination { operation: active, .. } if *active == operation) {
                    return;
                }
                let current = controller.session.update(cx, |session, cx| {
                    let current = session.finish(operation);
                    if current {
                        cx.notify();
                    }
                    current
                });
                if !current {
                    return;
                }
                controller.state = ExportState::Idle;
                match result {
                    Ok(Some(mut output)) => {
                        if !output
                            .extension()
                            .and_then(|extension| extension.to_str())
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"))
                        {
                            output.set_extension("mp4");
                        }
                        controller.output = Some(output);
                    }
                    Ok(None) => {}
                    Err(error) => controller.notifications.update(cx, |notifications, cx| {
                        notifications.push(error, cx);
                    }),
                }
                cx.notify();
            });
        });
        self.state = ExportState::ChoosingDestination {
            operation,
            _task: task,
        };
        cx.notify();
    }

    fn request_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let output = self.output_path(cx);
        if !output.exists() {
            self.start_export(output, cx);
            return;
        }
        let controller = cx.entity();
        let session_id = self.session.read(cx).id();
        // Open after the export settings modal closes.
        window.defer(cx, move |window, cx| {
            window.open_modal(cx, move |modal: Modal, _, _| {
                let controller = controller.clone();
                let destination = output.clone();
                modal
                    .title(
                        div()
                            .font_family(crate::ui::theme::FONT_FAMILY)
                            .font_normal()
                            .child(t!("export.overwrite_title").to_string()),
                    )
                    .confirm()
                    .button_props(
                        ModalButtonProps::default()
                            .ok_text(t!("export.overwrite").to_string())
                            .cancel_text(t!("common.cancel").to_string()),
                    )
                    .on_ok(move |_, _, cx| {
                        controller.update(cx, |controller, cx| {
                            if controller.session.read(cx).is_current(session_id) {
                                controller.start_export(destination.clone(), cx);
                            }
                        });
                        true
                    })
                    .child(t!("export.overwrite_confirmation").to_string())
                    .child(div().text_sm().child(output.display().to_string()))
            });
        });
    }
}

fn summary_row(label: impl Into<SharedString>, value: impl IntoElement) -> gpui::Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .w(px(120.))
                .flex_none()
                .text_color(gpui::rgb(0x888888))
                .child(label.into()),
        )
        .child(div().min_w_0().flex_1().child(value))
}

fn format_duration(seconds: f64) -> String {
    let total_seconds = seconds.max(0.).ceil() as u64;
    let hours = total_seconds / 3_600;
    let minutes = total_seconds % 3_600 / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}
