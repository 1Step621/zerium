use rust_i18n::t;

use gpui::{Context, DragMoveEvent, ExternalPaths, SharedString, Window, prelude::*};

use crate::{
    app::project::ProjectActivity,
    engine::input::prepare_import,
    ui::{
        components::input::file::import::{FileImport, FileImportTarget},
        panels::timeline::Timeline,
    },
};
use zerium_core::timeline::{Frame, LayerId};

impl Timeline {
    pub(in crate::ui::panels::timeline) fn update_file_drop_target<T>(
        &mut self,
        layer_index: usize,
        event: &DragMoveEvent<T>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if !event.bounds.contains(&event.event.position) {
            return;
        }

        let local_x = f32::from(event.event.position.x - event.bounds.origin.x);
        let viewport_width = f32::from(event.bounds.size.width).max(1.);
        let mut start = self.viewport.frame_at_x(
            local_x,
            0.,
            viewport_width,
            self.editor.read(cx).frame_rate(),
        );
        if !window.modifiers().alt {
            start = self.snap_frame(start, &[], cx);
        }
        let target = FileDropTarget {
            layer: LayerId::new(layer_index as u64),
            start,
        };
        if self.file_drop_target != Some(target) {
            self.file_drop_target = Some(target);
            cx.notify();
        }
    }

    pub(in crate::ui::panels::timeline) fn clear_file_drop_target(
        &mut self,
        layer_index: usize,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        if !hovered
            && self
                .file_drop_target
                .is_some_and(|target| target.layer == LayerId::new(layer_index as u64))
        {
            self.file_drop_target = None;
            cx.notify();
        }
    }

    pub(in crate::ui::panels::timeline) fn drop_external_files(
        &mut self,
        layer_index: usize,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let plugins = self.editor.read(cx).plugin_registry_arc();
        let files = paths
            .paths()
            .iter()
            .filter_map(|path| {
                FileImportTarget::for_path(path, &plugins).map(|target| FileImport {
                    path: path.clone(),
                    target,
                })
            })
            .collect();
        self.drop_files(layer_index, files, window, cx);
    }

    pub(in crate::ui::panels::timeline) fn drop_files(
        &mut self,
        layer_index: usize,
        files: Vec<FileImport>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if files.is_empty() {
            self.file_drop_target = None;
            cx.notify();
            return;
        }
        let layer = LayerId::new(layer_index as u64);
        let raw_pointer = self.pointer_frame(f32::from(window.mouse_position().x), window, cx);
        let start = if window.modifiers().alt {
            raw_pointer
        } else {
            self.file_drop_target
                .filter(|target| target.layer == layer)
                .map(|target| target.start)
                .unwrap_or_else(|| self.snap_frame(raw_pointer, &[], cx))
        };
        self.file_drop_target = None;
        window.focus(&self.focus_handle, cx);
        self.file_drop_error = None;

        let imports = files
            .into_iter()
            .map(|file| {
                (
                    file.path,
                    file.target.plugin_id,
                    file.target.item_id,
                    file.target.property_id,
                )
            })
            .collect::<Vec<_>>();
        let editor = self.editor.clone();
        let scene_id = self.editor.read(cx).active_scene_id();
        let plugins = self.editor.read(cx).plugin_registry_arc();
        let session = self.session.clone();
        let operation = session.update(cx, |session, cx| {
            let operation = session.begin(ProjectActivity::Import);
            cx.notify();
            operation
        });
        cx.spawn(async move |timeline, cx| {
            let results = cx
                .background_spawn(async move {
                    imports
                        .into_iter()
                        .map(|(path, plugin_id, item_id, property_id)| {
                            let name = path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| path.display().to_string());
                            let result =
                                prepare_import(path, &plugins, &plugin_id, &item_id, &property_id);
                            (name, plugin_id, item_id, result)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            if !session.update(cx, |session, _| session.operation_is_current(operation)) {
                return;
            }
            if editor.read_with(cx, |editor, _| editor.active_scene_id()) != scene_id {
                session.update(cx, |session, cx| {
                    if session.finish(operation) {
                        cx.notify();
                    }
                });
                return;
            }
            let mut errors = Vec::new();
            let changed = editor.update(cx, |editor, cx| {
                let mut next_start = start;
                let mut added = Vec::new();
                for (name, plugin_id, schema_item_id, result) in results {
                    let imported = match result {
                        Ok(imported) => imported,
                        Err(error) => {
                            errors.push(format!("{name}: {error}"));
                            continue;
                        }
                    };
                    let target = crate::app::defaults::DefaultsTarget::Item {
                        plugin_id: plugin_id.clone(),
                        item_id: schema_item_id.clone(),
                    };
                    let properties = editor
                        .plugin_registry()
                        .item(&plugin_id, &schema_item_id)
                        .map(|schema| {
                            crate::app::defaults::properties(cx, &target, schema.properties())
                        })
                        .unwrap_or_default();
                    let item_id = match editor.add_item(
                        layer,
                        next_start,
                        &plugin_id,
                        &schema_item_id,
                        properties,
                    ) {
                        Ok(item_id) => item_id,
                        Err(error) => {
                            errors.push(format!("{name}: {error}"));
                            continue;
                        }
                    };
                    if let Err(error) = editor.import_item_file(item_id, imported) {
                        editor.remove_item(item_id);
                        errors.push(format!("{name}: {error}"));
                        continue;
                    }
                    next_start = editor
                        .item(item_id)
                        .map(|item| item.end_exclusive(editor.frame_rate()))
                        .unwrap_or(next_start);
                    added.push(item_id);
                }
                if added.len() > 1 {
                    editor.select_items(added.iter().copied());
                }
                if !added.is_empty() {
                    cx.notify();
                }
                !added.is_empty()
            });
            let error = if errors.is_empty() {
                None
            } else if errors.len() == 1 {
                Some(SharedString::from(errors.remove(0)))
            } else {
                Some(SharedString::from(
                    t!(
                        "timeline.import_failed_count",
                        count = errors.len(),
                        details = errors.join(" / ")
                    )
                    .to_string(),
                ))
            };
            if let Some(error_message) = error.clone() {
                timeline
                    .update(cx, |timeline, cx| {
                        timeline.notifications.update(cx, |notifications, cx| {
                            notifications.push(error_message, cx);
                        });
                    })
                    .ok();
            }
            timeline
                .update(cx, |timeline, cx| {
                    timeline.file_drop_error = error;
                    if changed || timeline.file_drop_error.is_some() {
                        cx.notify();
                    }
                })
                .ok();
            session.update(cx, |session, cx| {
                if session.finish(operation) {
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::timeline) struct FileDropTarget {
    pub(in crate::ui::panels::timeline) layer: LayerId,
    pub(in crate::ui::panels::timeline) start: Frame,
}
