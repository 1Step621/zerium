//! Shared JSON persistence for application state and user configuration.

use std::{fs, io, path::PathBuf};

use directories::ProjectDirs;
use gpui::{App, BorrowAppContext as _, Global};
use serde::{Serialize, de::DeserializeOwned};

use crate::engine::atomic_file::AtomicFileTransaction;

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
