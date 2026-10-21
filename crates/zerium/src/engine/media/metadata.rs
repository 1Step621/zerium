//! Project reading dependencies and refresh state, independent of file input UI.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use zerium_core::{
    media::{
        FileMediaMetadata, FileRevision, ImportedFile, MediaDependency, MediaMetadataCache,
        MediaTarget, ProbedFile,
    },
    plugin::PluginRegistry,
    timeline::{ProjectId, TimelineSnapshot, TimelineView},
};

use super::{MediaError, MediaReaderRegistry};

/// Filesystem observation belongs to the media runtime; FileRevision itself is
/// a value shared with the project cache and persistence.
pub(in crate::engine) fn read_file_revision(path: &Path) -> std::io::Result<FileRevision> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Not a regular file",
        ));
    }
    Ok(FileRevision {
        size: metadata.len(),
        modified: metadata.modified()?,
    })
}

type Reading = (String, MediaTarget);

type Readings = HashMap<PathBuf, FileReadings>;

#[derive(Default)]
pub(crate) struct FileReadings {
    sources: HashSet<Reading>,
    revision: Option<FileRevision>,
    failed: HashSet<Reading>,
}

impl FileReadings {
    pub(super) fn new(sources: impl IntoIterator<Item = Reading>) -> Self {
        Self {
            sources: sources.into_iter().collect(),
            ..Default::default()
        }
    }
}

#[derive(Default)]
pub(crate) struct MediaMetadataUpdater {
    revision: Option<(ProjectId, u64)>,
    readings: Readings,
}

impl MediaMetadataUpdater {
    pub(crate) fn retry(&mut self, path: &Path) {
        if let Some(file) = self.readings.get_mut(path) {
            file.failed.clear();
        }
    }

    pub(crate) fn refresh(
        &mut self,
        snapshot: &TimelineSnapshot,
        readers: &MediaReaderRegistry,
    ) -> (Vec<ProbedFile>, Vec<MediaError>) {
        let revision = (
            snapshot.project_id(),
            snapshot.media_dependencies_revision(),
        );
        if self.revision != Some(revision) {
            let mut readings = dependency_readings(snapshot.project_media_dependencies());
            for (path, file) in &mut readings {
                if let Some(previous) = self.readings.remove(path) {
                    file.revision = previous.revision;
                    file.failed = previous.failed;
                    file.failed.retain(|reading| file.sources.contains(reading));
                }
            }
            self.readings = readings;
            self.revision = Some(revision);
        }
        refresh_files(&mut self.readings, snapshot.media_cache(), readers)
    }
}

/// Initialize placement from available readings. The background refresher
/// retries and reports missing readings after the file property is committed.
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
    let mut sources = FileReadings::new(
        item.media_sources()
            .filter(|source| source.file == property_id)
            .map(|source| (source.reader.to_owned(), source.input.target())),
    );
    let (file, _) = refresh_file(path, &mut sources, &MediaMetadataCache::default(), readers);
    let file = file.unwrap_or_else(|| ProbedFile {
        path: path.to_owned(),
        revision: None,
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
    readings: &mut Readings,
    cache: &MediaMetadataCache,
    readers: &MediaReaderRegistry,
) -> (Vec<ProbedFile>, Vec<MediaError>) {
    let mut files = Vec::new();
    let mut errors = Vec::new();
    for (path, sources) in readings {
        let (file, failures) = refresh_file(path, sources, cache, readers);
        files.extend(file);
        errors.extend(failures);
    }
    (files, errors)
}

pub(super) fn refresh_file(
    path: &Path,
    sources: &mut FileReadings,
    cache: &MediaMetadataCache,
    readers: &MediaReaderRegistry,
) -> (Option<ProbedFile>, Vec<MediaError>) {
    let previous = cache.file(path);
    let revision = read_file_revision(path);
    let observed = revision.as_ref().ok().copied();
    if sources.revision != observed {
        sources.revision = observed;
        sources.failed.clear();
    }
    let pending = sources
        .sources
        .difference(&sources.failed)
        .cloned()
        .collect::<Vec<_>>();
    let mut errors = Vec::new();
    let revision = match revision {
        Ok(revision) => revision,
        Err(error) => {
            if !pending.is_empty() {
                errors.push(MediaError::external(format!(
                    "'{}': {error}",
                    path.display()
                )));
            }
            sources.failed.extend(pending);
            return (None, errors);
        }
    };
    let mut file = previous
        .filter(|file| file.revision == Some(revision))
        .cloned()
        .unwrap_or_else(|| ProbedFile {
            path: path.to_owned(),
            revision: Some(revision),
            media: Vec::new(),
        });
    let mut probed = false;
    for (reader, target) in pending {
        if file.reading(&reader, target).is_some() {
            continue;
        }
        probed = true;
        match readers.probe(path, &reader, target) {
            Ok(metadata) => {
                file.media.push(FileMediaMetadata {
                    reader,
                    target,
                    metadata,
                });
            }
            Err(error) => {
                sources.failed.insert((reader, target));
                errors.push(error);
            }
        }
    }
    let after = if probed {
        read_file_revision(path).ok()
    } else {
        Some(revision)
    };
    if after != Some(revision) {
        // Publish no results from a file that changed during the batch. Existing
        // cache entries remain available if it disappeared while being read.
        file.revision = after.or(Some(revision));
        file.media.clear();
        if after.is_none()
            && let Some(previous) = previous.filter(|file| file.revision == Some(revision))
        {
            file = previous.clone();
        }
        errors.push(MediaError::external(format!(
            "'{}' changed while it was being read",
            path.display()
        )));
    }
    ((previous != Some(&file)).then_some(file), errors)
}

pub(crate) fn dependency_readings(
    dependencies: impl IntoIterator<Item = MediaDependency>,
) -> Readings {
    let mut readings = Readings::new();
    for dependency in dependencies {
        readings
            .entry(dependency.path)
            .or_default()
            .sources
            .insert((dependency.reader_id, dependency.target));
    }
    readings
}
