//! Derived reader results, separate from file property values and edit history.

use std::{
    collections::{BTreeMap, HashSet, btree_map::Entry},
    path::{Path, PathBuf},
    time::SystemTime,
};

use super::{FileMediaMetadata, MediaAsset, MediaDependency, MediaTarget};

/// Identifies the on-disk contents used by readers and decoded media caches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaRevision {
    File { size: u64, modified: SystemTime },
    Directory { fingerprint: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbedFile {
    pub path: PathBuf,
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

    /// Returns a reader's probe result, including successful probes with no stream.
    pub fn reading(&self, reader: &str, target: MediaTarget) -> Option<&FileMediaMetadata> {
        self.media
            .iter()
            .find(|reading| reading.reader == reader && reading.target == target)
    }

    pub fn asset(&self, reader: &str, target: MediaTarget) -> Option<MediaAsset> {
        let reading = self.reading(reader, target)?;
        let metadata = reading.metadata.as_ref()?;
        Some(MediaAsset {
            path: self.path.clone(),
            revision: reading.revision,
            reader_id: reader.to_owned(),
            duration: metadata.duration,
            kind: metadata.kind.clone(),
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaMetadataCache(BTreeMap<PathBuf, ProbedFile>);

impl MediaMetadataCache {
    pub fn file(&self, path: &Path) -> Option<&ProbedFile> {
        self.0.get(path)
    }

    pub fn asset(&self, path: &Path, reader: &str, target: MediaTarget) -> Option<MediaAsset> {
        self.0.get(path)?.asset(reader, target)
    }

    pub fn reading(&self, source: &MediaDependency) -> Option<&FileMediaMetadata> {
        self.file(&source.path)?
            .reading(&source.reader_id, source.target)
    }

    pub fn invalidate(&mut self, source: &MediaDependency) -> bool {
        let Some(file) = self.0.get_mut(&source.path) else {
            return false;
        };
        let count = file.media.len();
        file.media.retain(|reading| {
            reading.reader != source.reader_id || reading.target != source.target
        });
        let changed = file.media.len() != count;
        if file.media.is_empty() {
            self.0.remove(&source.path);
        }
        changed
    }

    pub fn record(&mut self, file: &ProbedFile) -> bool {
        if !file.is_valid() {
            return false;
        }
        let current = match self.0.entry(file.path.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(file.clone());
                return true;
            }
            Entry::Occupied(entry) => entry.into_mut(),
        };
        let mut changed = false;
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
}
