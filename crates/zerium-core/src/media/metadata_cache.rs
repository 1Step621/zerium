//! Derived reader results, separate from file property values and edit history.

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use super::{FileMediaMetadata, MediaAsset, MediaTarget};

/// Identifies the on-disk contents used by readers and decoded media caches.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub struct FileRevision {
    pub size: u64,
    pub modified: SystemTime,
}

impl FileRevision {
    pub fn read(path: &Path) -> std::io::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Not a regular file",
            ));
        }
        Ok(Self {
            size: metadata.len(),
            modified: metadata.modified()?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProbedFile {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<FileRevision>,
    pub media: Vec<FileMediaMetadata>,
}

impl ProbedFile {
    pub fn is_valid(&self) -> bool {
        let mut keys = HashSet::new();
        !self.path.as_os_str().is_empty()
            && self.media.iter().all(|reading| {
                !reading.reader.is_empty()
                    && keys.insert((&reading.reader, reading.target))
                    && reading
                        .metadata
                        .as_ref()
                        .is_none_or(|metadata| metadata.accepts(reading.target))
            })
    }

    pub fn asset(&self, reader: &str, target: MediaTarget) -> Option<MediaAsset> {
        let metadata = self
            .media
            .iter()
            .find(|reading| reading.reader == reader && reading.target == target)?
            .metadata
            .as_ref()?;
        Some(MediaAsset {
            path: self.path.clone(),
            revision: self.revision,
            reader_id: reader.to_owned(),
            duration: metadata.duration,
            kind: metadata.kind.clone(),
        })
    }

    pub fn initial_duration(&self) -> Option<Duration> {
        self.media
            .iter()
            .filter_map(|reading| reading.metadata.as_ref())
            .filter(|metadata| metadata.kind.is_temporal())
            .map(|metadata| metadata.duration)
            .max()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "Vec<ProbedFile>", into = "Vec<ProbedFile>")]
pub struct MediaMetadataCache(BTreeMap<PathBuf, ProbedFile>);

impl MediaMetadataCache {
    pub fn file(&self, path: &Path) -> Option<&ProbedFile> {
        self.0.get(path)
    }

    pub fn revision(&self, path: &Path) -> Option<FileRevision> {
        self.0.get(path)?.revision
    }

    pub fn has_reading(&self, path: &Path, reader: &str, target: MediaTarget) -> bool {
        self.0.get(path).is_some_and(|file| {
            file.media
                .iter()
                .any(|reading| reading.reader == reader && reading.target == target)
        })
    }

    pub fn asset(&self, path: &Path, reader: &str, target: MediaTarget) -> Option<MediaAsset> {
        self.0.get(path)?.asset(reader, target)
    }

    pub fn record(&mut self, file: &ProbedFile) -> bool {
        if !file.is_valid() {
            return false;
        }
        let current = match self.0.entry(file.path.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(file.clone());
                return true;
            }
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
        };
        let mut changed = current.revision != file.revision;
        if changed {
            current.revision = file.revision;
            current.media.clear();
        }
        for reading in &file.media {
            if let Some(previous) = current.media.iter_mut().find(|previous| {
                previous.reader == reading.reader && previous.target == reading.target
            }) {
                if previous != reading {
                    *previous = reading.clone();
                    changed = true;
                }
            } else {
                current.media.push(reading.clone());
                changed = true;
            }
        }
        changed
    }

    pub fn files(&self) -> impl Iterator<Item = &ProbedFile> {
        self.0.values()
    }

    pub fn record_missing(&mut self, file: &ProbedFile) -> bool {
        // Clipboard metadata cannot replace a newer observation from this project.
        if self.0.contains_key(&file.path) && self.revision(&file.path) != file.revision {
            return false;
        }
        let file = ProbedFile {
            path: file.path.clone(),
            revision: file.revision,
            media: file
                .media
                .iter()
                .filter(|reading| !self.has_reading(&file.path, &reading.reader, reading.target))
                .cloned()
                .collect(),
        };
        self.record(&file)
    }

    pub(crate) fn retained_paths<'a>(&self, paths: impl Iterator<Item = &'a Path>) -> Self {
        Self(
            paths
                .filter_map(|path| {
                    self.0
                        .get(path)
                        .map(|file| (file.path.clone(), file.clone()))
                })
                .collect(),
        )
    }

    pub(crate) fn mapped_paths(&self, mut map: impl FnMut(&Path) -> PathBuf) -> Self {
        let mut result = Self::default();
        for file in self.0.values() {
            let mut file = file.clone();
            file.path = map(&file.path);
            result.record(&file);
        }
        result
    }
}

impl TryFrom<Vec<ProbedFile>> for MediaMetadataCache {
    type Error = &'static str;

    fn try_from(files: Vec<ProbedFile>) -> Result<Self, Self::Error> {
        let mut result = Self::default();
        for file in files {
            if !file.is_valid() {
                return Err("Invalid cached media metadata");
            }
            if result.0.insert(file.path.clone(), file).is_some() {
                return Err("Duplicate cached file");
            }
        }
        Ok(result)
    }
}

impl From<MediaMetadataCache> for Vec<ProbedFile> {
    fn from(cache: MediaMetadataCache) -> Self {
        cache.0.into_values().collect()
    }
}
