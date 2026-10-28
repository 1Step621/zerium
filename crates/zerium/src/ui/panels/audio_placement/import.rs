use std::path::Path;

use rust_i18n::t;
use zerium_core::{
    media::{ImportedFile, MediaTarget},
    plugin::PluginRegistry,
};

use crate::{
    app::{
        defaults::{self, DefaultsTarget},
        project::ProjectActivity,
    },
    engine::input::prepare_import,
};

use super::*;

impl AudioPlacement {
    pub(super) fn prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.import_task.is_some() || self.layer.is_none() {
            return;
        }
        self.error = None;
        let scope = (self.session_id, self.scene_id, self.layer);
        let plugins = self.editor.read(cx).plugin_registry_arc();
        let operation = self.session.update(cx, |session, cx| {
            let operation = session.begin(ProjectActivity::Import);
            cx.notify();
            operation
        });
        let selection = rfd::AsyncFileDialog::new()
            .set_parent(window)
            .set_title(t!("audio_placement.title").to_string())
            .pick_folder();
        self.import_task = Some((
            operation,
            cx.spawn_in(window, async move |panel, cx| {
                let result = if let Some(folder) = selection.await {
                    let path = folder.path().to_owned();
                    Some(
                        cx.background_spawn(async move { read_folder(&path, &plugins) })
                            .await,
                    )
                } else {
                    None
                };
                panel
                    .update_in(cx, |this, _, cx| {
                        this.finish_import(cx);
                        cx.notify();
                        if (
                            this.session.read(cx).id(),
                            this.editor.read(cx).active_scene_id(),
                            this.layer,
                        ) != scope
                        {
                            return;
                        }
                        if let Some(result) = result
                            && let Err(error) = result.and_then(|files| this.load_files(files, cx))
                        {
                            this.error =
                                Some(t!("audio_placement.read_failed", error = error).to_string());
                        }
                    })
                    .ok();
            }),
        ));
        cx.notify();
    }

    fn load_files(
        &mut self,
        files: Vec<ImportedFile>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some(layer) = self.layer else {
            return Ok(());
        };
        let editor = self.editor.read(cx);
        let start = editor
            .items_on_layer(layer)
            .iter()
            .map(|item| item.end_exclusive(editor.frame_rate()))
            .max()
            .unwrap_or(editor.playhead())
            .max(editor.playhead());
        let imports = files
            .into_iter()
            .map(|file| {
                let schema = editor
                    .plugin_registry()
                    .item(&file.plugin_id, &file.source_id)
                    .expect("imported source");
                let properties = defaults::properties(
                    cx,
                    &DefaultsTarget::Item {
                        plugin_id: file.plugin_id.clone(),
                        item_id: file.source_id.clone(),
                    },
                    schema.properties(),
                );
                (file, properties)
            })
            .collect();
        let ids = self
            .editor
            .update(cx, |editor, cx| {
                let ids = editor.import_items(layer, start, imports)?;
                if let Some(id) = ids.first() {
                    editor.select(*id);
                }
                cx.notify();
                Ok::<_, TimelineEditError>(ids)
            })
            .map_err(|error| error.to_string())?;
        self.stamp_target = ids.first().copied().map(StampTarget::Start);
        if let Some(id) = ids.first() {
            self.scroll.scroll_to_item(
                self.rows(cx)
                    .iter()
                    .position(|row| row.id == *id)
                    .unwrap_or(0),
            );
        }
        Ok(())
    }
}

fn read_folder(path: &Path, plugins: &PluginRegistry) -> Result<Vec<ImportedFile>, String> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(path).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            paths.push(entry.path());
        }
    }
    paths.sort();
    let mut files = Vec::new();
    for path in paths {
        let target = plugins.items().find_map(|(plugin, schema)| {
            let source = audio_source(schema)?;
            schema
                .file_property(source.file)?
                .configuration_ui(None)
                .matches_file(&path)
                .then(|| (plugin, schema.id(), source.file, source.reader))
        });
        let Some((plugin, item, property, reader)) = target else {
            continue;
        };
        let file = prepare_import(&path, plugins, plugin, item, property)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if file.file.asset(reader, MediaTarget::Audio).is_none() {
            return Err(t!("audio_placement.invalid_audio", name = path.display()).to_string());
        }
        files.push(file);
    }
    if files.is_empty() {
        return Err(t!("audio_placement.empty").to_string());
    }
    Ok(files)
}
