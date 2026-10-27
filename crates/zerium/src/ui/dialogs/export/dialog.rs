use rust_i18n::t;
use zerium_core::timeline::{AudioCodec, ExportColorRange, ExportContainer, TimelineView as _};

use std::path::{Path, PathBuf};

use ::ui::{
    ContextModal as _, Disableable as _, IconName, Sizable as _, StyledExt as _,
    button::Button,
    modal::{Modal, ModalButtonProps},
};

use gpui::{App, Context, Entity, Window, div, prelude::*};

use crate::{
    app::project::ProjectActivity,
    ui::components::form::{form_dialog, form_fields, form_row},
};

use super::{ExportController, ExportState};

impl ExportController {
    pub(crate) fn open_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let editor = self.editor.read(cx);
        let frame_rate = editor.frame_rate();
        let resolution = editor.resolution();
        let frame_count = editor.snapshot().end_frame_exclusive();
        let duration_label = format_duration(frame_rate.frame_to_seconds(frame_count));
        let frame_rate_label = format!("{:.2} fps", frame_rate.frames_per_second());
        let settings = editor.export_settings().clone();
        let output = self.output_path(cx);
        let destination = cx.new(|_| output);
        let video_bitrate_label = settings
            .video_bitrate_kbps
            .map(|bitrate| format!("{bitrate} kbps"))
            .unwrap_or_else(|| t!("export.automatic").to_string());
        let audio_label = if settings.exports_audio() {
            let channels = if settings.audio_channels == 1 {
                t!("export.mono")
            } else {
                t!("export.stereo")
            };
            let mut label = format!(
                "{} / {} kHz / {}",
                settings.audio_codec,
                settings.audio_sample_rate as f32 / 1000.,
                channels
            );
            if settings.audio_codec != AudioCodec::Flac {
                label.push_str(&format!(" / {} kbps", settings.audio_bitrate_kbps));
            }
            label
        } else {
            t!("export.no_audio").to_string()
        };
        let color_range_label = match settings.color.range {
            ExportColorRange::Limited => t!("export.range_limited").to_string(),
            ExportColorRange::Full => t!("export.range_full").to_string(),
        };
        let is_gif = settings.container == ExportContainer::Gif;
        let format_label = if is_gif {
            settings.container.to_string()
        } else {
            format!("{} / {}", settings.container, settings.video_codec)
        };
        let mut format_details = Vec::new();
        if is_gif {
            format_details.push((
                t!("export.loop").to_string(),
                if settings.gif_loop {
                    t!("common.on")
                } else {
                    t!("common.off")
                }
                .to_string(),
            ));
        } else {
            format_details.extend([
                (
                    t!("export.color_space").to_string(),
                    settings.color.space.to_string(),
                ),
                (
                    t!("export.bit_depth").to_string(),
                    format!("{} bit", settings.color.depth.bits()),
                ),
                (
                    t!("export.color_range").to_string(),
                    color_range_label.clone(),
                ),
                (
                    t!("export.video_bitrate").to_string(),
                    video_bitrate_label.clone(),
                ),
                (t!("export.audio").to_string(), audio_label.clone()),
            ]);
        }
        let controller = cx.entity();
        let session_id = self.session.read(cx).id();
        window.open_modal(cx, move |modal: Modal, window, cx| {
            let export = controller.read(cx);
            let output = destination.read(cx).display().to_string();
            let busy = export.is_busy();
            let destination_controller = controller.clone();
            let confirm_controller = controller.clone();
            let confirm_destination = destination.clone();
            let destination_draft = destination.clone();
            form_dialog(modal, t!("export.title").to_string(), window)
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
                        let output = confirm_destination.read(cx).clone();
                        let result = controller.editor.update(cx, |editor, cx| {
                            let result = editor.update_export_destination(output);
                            if let Ok(true) = result {
                                cx.notify();
                            }
                            result
                        });
                        if let Err(error) = result {
                            controller.notifications.update(cx, |notifications, cx| {
                                notifications.push(error.to_string(), cx);
                            });
                            return false;
                        }
                        controller.request_export(window, cx);
                        true
                    })
                })
                .child(
                    form_fields()
                        .gap_1()
                        .child(form_row(
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
                                        .tooltip(format!(
                                            "{}\n{output}",
                                            t!("export.choose_destination")
                                        ))
                                        .disabled(busy)
                                        .on_click(move |_, _, cx| {
                                            destination_controller.update(cx, |controller, cx| {
                                                if controller
                                                    .session
                                                    .read(cx)
                                                    .is_current(session_id)
                                                {
                                                    controller.choose_output(
                                                        destination_draft.clone(),
                                                        cx,
                                                    );
                                                }
                                            });
                                        }),
                                ),
                        ))
                        .children(
                            [
                                (t!("export.format").to_string(), format_label.clone()),
                                (
                                    t!("export.resolution").to_string(),
                                    format!("{} × {}", resolution.width(), resolution.height()),
                                ),
                                (
                                    t!("export.frame_rate").to_string(),
                                    frame_rate_label.clone(),
                                ),
                                (t!("export.duration").to_string(), duration_label.clone()),
                            ]
                            .into_iter()
                            .chain(format_details.iter().cloned())
                            .map(|(label, value)| form_row(label, value).min_h_0()),
                        ),
                )
        });
    }

    fn output_path(&self, cx: &App) -> PathBuf {
        let settings = self.editor.read(cx).export_settings();
        let extension = settings.container.extension();
        if let Some(output) = &settings.destination {
            return output.with_extension(extension);
        }
        if let Some(path) = self.session.read(cx).path() {
            return path.with_extension(extension);
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
        initial_directory.join("output").with_extension(extension)
    }

    fn choose_output(&mut self, destination: Entity<PathBuf>, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let output = destination.read(cx).clone();
        let extension = self.editor.read(cx).export_settings().container.extension();
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
                let ExportState::ChoosingDestination {
                    operation: active, ..
                } = &controller.state
                else {
                    return;
                };
                if *active != operation {
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
                        output.set_extension(extension);
                        destination.update(cx, |destination, cx| {
                            *destination = output;
                            cx.notify();
                        });
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

fn format_duration(seconds: f64) -> String {
    let total_millis = (seconds.max(0.) * 1_000.).round() as u64;
    let hours = total_millis / 3_600_000;
    let minutes = total_millis % 3_600_000 / 60_000;
    let seconds = total_millis % 60_000 / 1_000;
    let millis = total_millis % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}
