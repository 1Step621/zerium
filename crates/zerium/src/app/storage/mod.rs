//! Application storage, JSON state, and atomic file transactions.

use directories::ProjectDirs;
use gpui::{App, BorrowAppContext as _, Global};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub(crate) mod config;
pub(crate) mod state;

pub(crate) struct JsonStore<T> {
    path: PathBuf,
    value: T,
}

impl<T: 'static> Global for JsonStore<T> {}

impl<T: Clone + Default + DeserializeOwned + Serialize + 'static> JsonStore<T> {
    pub(crate) fn load(filename: &str) -> io::Result<Self> {
        let directories = ProjectDirs::from("dev", "1Step621", "Zerium")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No user config directory"))?;
        let path = directories.config_dir().join(filename);
        match fs::read(&path) {
            Ok(bytes) => Ok(Self {
                path,
                value: serde_json::from_slice(&bytes)?,
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let store = Self {
                    path,
                    value: T::default(),
                };
                store.save(&store.value)?;
                Ok(store)
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn value(&self) -> &T {
        &self.value
    }

    pub(crate) fn read(cx: &App) -> io::Result<&T> {
        cx.try_global::<Self>().map(Self::value).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "Persistent store is unavailable")
        })
    }

    pub(crate) fn update<R>(cx: &mut App, change: impl FnOnce(&mut T) -> R) -> io::Result<R> {
        Self::read(cx)?;
        cx.update_global::<Self, _>(|store, _| {
            let mut next = store.value.clone();
            let result = change(&mut next);
            store.save(&next)?;
            store.value = next;
            Ok(result)
        })
    }

    fn save(&self, value: &T) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let transaction = AtomicFileTransaction::new(&self.path)?;
        transaction.write_all(&serde_json::to_vec_pretty(value)?)?;
        transaction.commit()
    }
}

static NEXT_TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);

/// A same-directory file transaction. The destination is only replaced after
/// all bytes in the temporary file have been flushed and synchronized.
pub(crate) struct AtomicFileTransaction {
    destination: PathBuf,
    temporary: PathBuf,
    committed: bool,
}

impl AtomicFileTransaction {
    pub(crate) fn new(destination: impl Into<PathBuf>) -> io::Result<Self> {
        let destination = destination.into();
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let file_name = destination
            .file_name()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "destination has no file name")
            })?
            .to_string_lossy();

        for _ in 0..128 {
            let id = NEXT_TEMPORARY_ID.fetch_add(1, Ordering::Relaxed);
            let temporary = parent.join(format!(
                ".{file_name}.zerium-{}-{id}.tmp",
                std::process::id()
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => {
                    drop(file);
                    return Ok(Self {
                        destination,
                        temporary,
                        committed: false,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }

        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve a unique temporary file",
        ))
    }

    pub(crate) fn temporary_path(&self) -> &Path {
        &self.temporary
    }

    pub(crate) fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.temporary)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()
    }

    pub(crate) fn commit(mut self) -> io::Result<()> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.temporary)?
            .sync_all()?;

        fs::rename(&self.temporary, &self.destination)?;
        sync_parent(&self.destination)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for AtomicFileTransaction {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.temporary);
        }
    }
}

#[cfg(not(windows))]
fn sync_parent(path: &Path) -> io::Result<()> {
    use std::fs::File;

    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    File::open(parent)?.sync_all()
}

#[cfg(windows)]
fn sync_parent(_path: &Path) -> io::Result<()> {
    Ok(())
}
