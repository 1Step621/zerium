//! Project reading dependencies and refresh state, independent of file input UI.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use zerium_core::{
    media::{
        FileMediaMetadata, FileRevision, MediaMetadataCache, MediaSource, MediaTarget, ProbedFile,
    },
    property::PropertyValues,
    timeline::{ProjectId, TimelineItem, TimelineSnapshot, TimelineView},
};

use super::{MediaError, MediaReaderRegistry};

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
        let revision = (snapshot.project_id(), snapshot.project_revision());
        if self.revision != Some(revision) {
            let mut readings = Readings::new();
            snapshot.visit_resolved_items(|item| add_item_readings(&mut readings, item));
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
    let revision = FileRevision::read(path);
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
        if file
            .media
            .iter()
            .any(|reading| reading.reader == reader && reading.target == target)
        {
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
        FileRevision::read(path).ok()
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

pub(crate) fn item_readings<'a>(items: impl IntoIterator<Item = &'a TimelineItem>) -> Readings {
    let mut readings = Readings::new();
    for item in items {
        add_item_readings(&mut readings, item);
    }
    readings
}

fn add_item_readings(readings: &mut Readings, item: &TimelineItem) {
    if let Some(schema) = item.schema() {
        add_readings(readings, &item.properties, schema.media_sources());
    }
    for effect in &item.effects {
        add_readings(
            readings,
            &effect.properties,
            effect.schema().media_sources(),
        );
    }
}

fn add_readings<'a>(
    readings: &mut Readings,
    properties: &PropertyValues,
    sources: impl Iterator<Item = MediaSource<'a>>,
) {
    for source in sources {
        if let Some(path) = properties
            .property(source.file)
            .and_then(|value| value.file())
        {
            readings
                .entry(path.to_owned())
                .or_default()
                .sources
                .insert((source.reader.to_owned(), source.input.target()));
        }
    }
}
