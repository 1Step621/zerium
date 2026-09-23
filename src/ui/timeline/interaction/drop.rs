use super::*;

impl Timeline {
    pub(in crate::ui::timeline) fn update_explorer_drop_target(
        &mut self,
        layer_index: usize,
        event: &DragMoveEvent<ExplorerFileDrag>,
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
        if !event.event.modifiers.alt {
            start = self.snap_frame(start, None, &[], cx);
        }
        let target = ExplorerDropTarget {
            layer: LayerId::new(layer_index as u64),
            start,
        };
        if self.explorer_drop_target != Some(target) {
            self.explorer_drop_target = Some(target);
            cx.notify();
        }
    }

    pub(in crate::ui::timeline) fn clear_explorer_drop_target(
        &mut self,
        layer_index: usize,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        if !hovered
            && self
                .explorer_drop_target
                .is_some_and(|target| target.layer == LayerId::new(layer_index as u64))
        {
            self.explorer_drop_target = None;
            cx.notify();
        }
    }

    pub(in crate::ui::timeline) fn drop_explorer_items(
        &mut self,
        layer_index: usize,
        drag: &ExplorerFileDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let layer = LayerId::new(layer_index as u64);
        let raw_pointer = self.pointer_frame(f32::from(window.mouse_position().x), window, cx);
        let start = if window.modifiers().alt {
            raw_pointer
        } else {
            self.explorer_drop_target
                .filter(|target| target.layer == layer)
                .map(|target| target.start)
                .unwrap_or_else(|| self.snap_frame(raw_pointer, None, &[], cx))
        };
        self.explorer_drop_target = None;
        window.focus(&self.focus_handle, cx);
        self.file_drop_error = None;

        let imports = drag
            .files()
            .iter()
            .map(|file| {
                (
                    file.path().to_path_buf(),
                    file.plugin_id().to_owned(),
                    file.item_id().to_owned(),
                    file.input_id().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        let editor = self.editor.clone();
        let media_readers = self.media_readers.clone();
        let session = self.session.clone();
        let operation =
            session.update(cx, |session, cx| session.begin(ProjectActivity::Import, cx));
        cx.spawn(async move |timeline, cx| {
            let results = cx
                .background_spawn(async move {
                    imports
                        .into_iter()
                        .map(|(path, plugin_id, item_id, input_id)| {
                            let name = path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| path.display().to_string());
                            let result =
                                media_readers.probe_for_item(path, &plugin_id, &item_id, &input_id);
                            (name, plugin_id, item_id, result)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            if !session.update(cx, |session, _| session.operation_is_current(operation)) {
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
                    let item_id =
                        match editor.add_item(layer, next_start, &plugin_id, &schema_item_id) {
                            Ok(item_id) => item_id,
                            Err(error) => {
                                errors.push(format!("{name}: {error}"));
                                continue;
                            }
                        };
                    if let Err(error) = editor.set_item_asset(item_id, imported) {
                        editor.remove_item(item_id);
                        errors.push(format!("{name}: {error}"));
                        continue;
                    }
                    next_start = editor
                        .item(item_id)
                        .map(|item| item.end_exclusive())
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
                Some(SharedString::from(format!(
                    "{}件の読み込みに失敗しました: {}",
                    errors.len(),
                    errors.join(" / ")
                )))
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
                session.finish(operation, cx);
            });
        })
        .detach();
    }
}
