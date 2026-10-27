//! Still-frame commands capture the menu's frame before any asynchronous work.
use std::path::{Path, PathBuf};

use ::ui::{IconName, menu::PopupMenu};
use gpui::{AppContext as _, ClipboardItem, Context, Image, ImageFormat};
use rust_i18n::t;
use zerium_core::timeline::{TimelineSnapshot, TimelineView as _};

use super::Preview;
use crate::app::export::{ExportedFrame, export_frame};

#[derive(Clone, Copy)]
enum ImageAction {
    Save,
    Copy,
}

impl Preview {
    pub(super) fn image_menu(&mut self, menu: PopupMenu, cx: &mut Context<Self>) -> PopupMenu {
        self.transport
            .update(cx, |transport, cx| transport.stop_in_place(cx));
        let snapshot = self.editor.read(cx).preview_snapshot();
        let disabled =
            self.image_task.is_some() || self.render_runtime.read(cx).preview_session().is_none();
        [
            (
                ImageAction::Save,
                t!("preview.save_image"),
                IconName::Download,
            ),
            (ImageAction::Copy, t!("preview.copy_image"), IconName::Copy),
        ]
        .into_iter()
        .fold(menu, |menu, (action, label, icon)| {
            let preview = cx.entity();
            let snapshot = snapshot.clone();
            menu.menu_handler_with_icon_and_disabled(
                label.to_string(),
                icon,
                disabled,
                move |_, cx| {
                    preview.update(cx, |preview, cx| {
                        preview.export_image(snapshot.clone(), action, cx)
                    });
                },
            )
        })
    }

    fn export_image(
        &mut self,
        snapshot: TimelineSnapshot,
        action: ImageAction,
        cx: &mut Context<Self>,
    ) {
        if self.image_task.is_some() {
            return;
        }
        let Some(renderer) = self.render_runtime.read(cx).preview_session() else {
            return;
        };
        let media_readers = self.media_readers.clone();
        let initial_directory = self
            .session
            .read(cx)
            .path()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let suggested_name = format!("frame-{:06}.png", snapshot.playhead().get());
        self.image_task = Some(cx.spawn(async move |preview, cx| {
            let result: Result<Option<ExportedFrame>, String> = async {
                let output = if let ImageAction::Save = action {
                    let receiver = cx.update(|cx| {
                        cx.prompt_for_new_path(&initial_directory, Some(&suggested_name))
                    });
                    let Some(mut path) = receiver
                        .await
                        .map_err(|e| e.to_string())?
                        .map_err(|e| e.to_string())?
                    else {
                        return Ok(None);
                    };
                    path.set_extension("png");
                    Some(path)
                } else {
                    None
                };
                cx.background_spawn(async move {
                    export_frame(snapshot, renderer.fork(), media_readers, output)
                        .map(Some)
                        .map_err(|error| error.to_string())
                })
                .await
            }
            .await;
            let _ = preview.update(cx, |preview, cx| {
                preview.image_task = None;
                match result {
                    Ok(Some(exported)) => {
                        let message = match exported {
                            ExportedFrame::Saved => t!("preview.image_saved"),
                            ExportedFrame::Png(png) => {
                                cx.write_to_clipboard(ClipboardItem::new_image(
                                    &Image::from_bytes(ImageFormat::Png, png),
                                ));
                                t!("preview.image_copied")
                            }
                        };
                        preview.notifications.update(cx, |notifications, cx| {
                            notifications.push_success(message.to_string(), cx)
                        });
                    }
                    Ok(None) => {}
                    Err(error) => {
                        preview.notifications.update(cx, |notifications, cx| {
                            notifications
                                .push(t!("preview.image_failed", error = error).to_string(), cx)
                        });
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
