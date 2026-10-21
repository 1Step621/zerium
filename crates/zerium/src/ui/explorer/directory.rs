use rust_i18n::t;

use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use directories::UserDirs;
use gpui::{ClickEvent, Context, SharedString, Window, prelude::*};

use crate::project_session::ProjectSession;

use crate::ui::file_import::FileImportTarget;

use crate::ui::explorer::Explorer;

#[derive(Clone)]
pub(super) struct ExplorerEntry {
    pub(super) element_id: u64,
    pub(super) path: PathBuf,
    pub(super) name: SharedString,
    pub(super) is_directory: bool,
    pub(super) import_target: Option<FileImportTarget>,
}

impl Explorer {
    pub(in crate::ui::explorer) fn import_target(&self, path: &Path) -> Option<FileImportTarget> {
        FileImportTarget::for_path(path, &self.plugins)
    }

    pub(in crate::ui::explorer) fn load_directory(
        &mut self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.load_generation = self.load_generation.saturating_add(1);
        let generation = self.load_generation;
        let session_id = self.session.read(cx).id();
        let session = self.session.clone();
        self.error = None;
        self._load_task = cx.spawn(async move |explorer, cx| {
            let input = path.clone();
            let result = cx
                .background_spawn(async move { read_directory(&input) })
                .await;
            if !session.update(cx, |session, _| session.is_current(session_id)) {
                return;
            }
            explorer
                .update(cx, |explorer, cx| {
                    if explorer.load_generation != generation {
                        return;
                    }
                    match result {
                        Ok(raw_entries) => {
                            explorer.current_directory = path;
                            explorer.entries = raw_entries
                                .into_iter()
                                .map(|(path, name, is_directory)| {
                                    let mut hasher = DefaultHasher::new();
                                    path.hash(&mut hasher);
                                    ExplorerEntry {
                                        element_id: hasher.finish(),
                                        import_target: (!is_directory)
                                            .then(|| explorer.import_target(&path))
                                            .flatten(),
                                        path,
                                        name: name.into(),
                                        is_directory,
                                    }
                                })
                                .collect();
                            explorer.selected_paths.clear();
                            explorer.marquee = None;
                            explorer.error = None;
                        }
                        Err(error) => {
                            explorer.error = Some(error.clone().into());
                            explorer.notifications.update(cx, |notifications, cx| {
                                notifications.push(error, cx);
                            });
                        }
                    }
                    cx.notify();
                })
                .ok();
        });
    }

    pub(in crate::ui::explorer) fn open_parent(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(parent) = self.current_directory.parent().map(Path::to_path_buf) else {
            return;
        };
        self.load_directory(parent, cx);
        cx.notify();
    }

    pub(in crate::ui::explorer) fn open_home(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(home) = UserDirs::new().map(|directories| directories.home_dir().to_path_buf())
        else {
            return;
        };
        self.load_directory(home, cx);
        cx.notify();
    }

    pub(in crate::ui::explorer) fn choose_directory(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let generation = self.load_generation;
        let session_id = self.session.read(cx).id();
        let selection = rfd::AsyncFileDialog::new()
            .set_parent(window)
            .set_directory(&self.current_directory)
            .set_title(t!("explorer.choose_folder").to_string())
            .pick_folder();
        cx.spawn(async move |explorer, cx| {
            let Some(folder) = selection.await else {
                return;
            };
            explorer
                .update(cx, |explorer, cx| {
                    if explorer.load_generation != generation
                        || !explorer.session.read(cx).is_current(session_id)
                    {
                        return;
                    }
                    explorer.load_directory(folder.path().to_path_buf(), cx);
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    pub(in crate::ui::explorer) fn refresh(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.load_directory(self.current_directory.clone(), cx);
        cx.notify();
    }

    pub(in crate::ui::explorer) fn open_entry(
        &mut self,
        path: PathBuf,
        is_directory: bool,
        event: &ClickEvent,
        cx: &mut Context<Self>,
    ) {
        if is_directory && event.click_count() >= 2 {
            self.load_directory(path, cx);
            cx.notify();
        }
    }
}

pub(in crate::ui::explorer) fn default_directory(session: &ProjectSession) -> PathBuf {
    session
        .path()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .or_else(|| UserDirs::new().map(|directories| directories.home_dir().to_path_buf()))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn read_directory(path: &Path) -> Result<Vec<(PathBuf, String, bool)>, String> {
    let directory = fs::read_dir(path).map_err(|error| {
        t!("explorer.open_failed", path = path.display(), error = error).to_string()
    })?;
    let mut entries = Vec::new();
    for entry in directory {
        let entry = entry.map_err(|error| {
            t!(
                "explorer.read_entries_failed",
                path = path.display(),
                error = error
            )
            .to_string()
        })?;
        let entry_path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let is_directory = entry
            .file_type()
            .map_err(|error| {
                t!(
                    "explorer.determine_type_failed",
                    path = entry_path.display(),
                    error = error
                )
                .to_string()
            })?
            .is_dir();
        entries.push((entry_path, name, is_directory));
    }
    entries.sort_by(|left, right| {
        right
            .2
            .cmp(&left.2)
            .then_with(|| left.1.to_lowercase().cmp(&right.1.to_lowercase()))
    });
    Ok(entries)
}
