//! Project reading dependencies and refresh state, independent of file input UI.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};

use zerium_core::{
    media::{
        FileMediaMetadata, ImportedFile, MediaDependency, MediaMetadataCache, MediaRevision,
        ProbedFile,
    },
    plugin::PluginRegistry,
    timeline::{ProjectId, TimelineSnapshot, TimelineView},
};

use super::{MediaError, MediaReaderRegistry};

/// Default observation for readers backed by a single regular file.
pub(in crate::engine) fn read_file_revision(path: &Path) -> std::io::Result<MediaRevision> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Not a regular file",
        ));
    }
    Ok(MediaRevision::File {
        size: metadata.len(),
        modified: metadata.modified()?,
    })
}

#[derive(Default)]
pub(crate) struct MediaReadings {
    sources: HashSet<MediaDependency>,
    /// Suppress repeated failures until the source changes or is explicitly retried.
    failures: HashMap<MediaDependency, Option<MediaRevision>>,
    retries: HashSet<PathBuf>,
}

#[derive(Default)]
pub(crate) struct MediaRefresh {
    pub(crate) files: Vec<ProbedFile>,
    pub(crate) invalidated: Vec<MediaDependency>,
    pub(crate) errors: Vec<(MediaDependency, MediaError)>,
}

#[derive(Default)]
pub(crate) struct MediaMetadataUpdater {
    revision: Option<(ProjectId, u64)>,
    readings: MediaReadings,
}

impl MediaMetadataUpdater {
    pub(crate) fn retry(&mut self, path: &Path) {
        self.readings.retries.insert(path.to_owned());
    }

    pub(crate) fn refresh(
        &mut self,
        snapshot: &TimelineSnapshot,
        readers: &MediaReaderRegistry,
    ) -> MediaRefresh {
        let revision = (
            snapshot.project_id(),
            snapshot.media_dependencies_revision(),
        );
        if self.revision != Some(revision) {
            if self
                .revision
                .is_some_and(|previous| previous.0 != revision.0)
            {
                self.readings = MediaReadings::default();
            }
            self.readings.sources = snapshot.project_media_dependencies();
            self.readings
                .failures
                .retain(|source, _| self.readings.sources.contains(source));
            self.revision = Some(revision);
        }
        refresh_files(&mut self.readings, snapshot.media_cache(), readers)
    }
}

/// Discover every interpretation before committing an imported source. Import
/// errors have the same semantics regardless of the property's picker UI.
pub(crate) fn prepare_import(
    path: impl AsRef<Path>,
    plugins: &PluginRegistry,
    plugin_id: &str,
    item_id: &str,
    property_id: &str,
    readers: &MediaReaderRegistry,
) -> Result<ImportedFile, MediaError> {
    let path = path.as_ref();
    let item = plugins.item(plugin_id, item_id).ok_or_else(|| {
        MediaError::invalid_input(format!("Item '{plugin_id}:{item_id}' is not registered"))
    })?;
    if item.file_property(property_id).is_none() {
        return Err(MediaError::invalid_input(format!(
            "File property '{property_id}' was not found"
        )));
    }
    let mut readings = dependency_readings(
        item.media_sources()
            .filter(|source| source.file == property_id)
            .map(|source| MediaDependency {
                path: path.to_owned(),
                reader_id: source.reader.to_owned(),
                target: source.input.target(),
            }),
    );
    let refresh = refresh_files(&mut readings, &MediaMetadataCache::default(), readers);
    if let Some((_, error)) = refresh.errors.into_iter().next() {
        return Err(error);
    }
    let file = refresh
        .files
        .into_iter()
        .next()
        .unwrap_or_else(|| ProbedFile {
            path: path.to_owned(),
            media: Vec::new(),
        });
    Ok(ImportedFile {
        plugin_id: plugin_id.to_owned(),
        source_id: item_id.to_owned(),
        property_id: property_id.to_owned(),
        file,
    })
}

pub(crate) fn refresh_files(
    readings: &mut MediaReadings,
    cache: &MediaMetadataCache,
    readers: &MediaReaderRegistry,
) -> MediaRefresh {
    let retries = std::mem::take(&mut readings.retries);
    let mut files: BTreeMap<PathBuf, ProbedFile> = BTreeMap::new();
    let mut refresh = MediaRefresh::default();
    for source in &readings.sources {
        let forced = retries.contains(&source.path);
        let previous = cache.reading(source);
        let reader = readers.reader_for(&source.reader_id);
        let revision = reader
            .as_ref()
            .map_err(|error| (*error).clone())
            .and_then(|reader| {
                reader.revision(&source.path).map_err(|error| {
                    MediaError::external(format!("'{}': {error}", source.path.display()))
                })
            });
        let observed = revision.as_ref().ok().copied();
        if !forced
            && (readings.failures.get(source) == Some(&observed)
                || observed.is_some()
                    && previous.is_some_and(|reading| Some(reading.revision) == observed))
        {
            continue;
        }
        let result = revision.and_then(|revision| {
            let metadata = readers.probe(&source.path, &source.reader_id, source.target)?;
            let after = reader
                .as_ref()
                .expect("revision requires an available reader")
                .revision(&source.path)
                .ok();
            if after != Some(revision) {
                return Err(MediaError::external(format!(
                    "'{}' changed while it was being read",
                    source.path.display()
                )));
            }
            Ok(FileMediaMetadata {
                reader: source.reader_id.clone(),
                target: source.target,
                revision,
                metadata,
            })
        });
        match result {
            Ok(reading) => {
                readings.failures.remove(source);
                if previous != Some(&reading) {
                    files
                        .entry(source.path.clone())
                        .or_insert_with(|| ProbedFile {
                            path: source.path.clone(),
                            media: Vec::new(),
                        })
                        .media
                        .push(reading);
                }
            }
            Err(error) => {
                readings.failures.insert(source.clone(), observed);
                // An observed replacement that cannot be interpreted must not
                // keep displaying decoded pixels from the previous revision.
                // Missing files retain their observation until they reappear.
                if observed.is_some()
                    && previous.is_some()
                    && reader
                        .as_ref()
                        .is_ok_and(|reader| reader.revision(&source.path).is_ok())
                {
                    refresh.invalidated.push(source.clone());
                }
                refresh.errors.push((source.clone(), error));
            }
        }
    }
    refresh.files = files.into_values().collect();
    refresh
}

pub(crate) fn dependency_readings(
    dependencies: impl IntoIterator<Item = MediaDependency>,
) -> MediaReadings {
    MediaReadings {
        sources: dependencies.into_iter().collect(),
        ..Default::default()
    }
}
