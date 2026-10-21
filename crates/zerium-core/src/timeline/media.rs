//! File dependencies are resolved independently of rendering and audio controls.
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

use crate::media::{MediaDependency, MediaSource};

use super::{
    EffectInstanceId, LayerId, SceneDefinition, SceneId, TimelineItem, TimelineSnapshot,
    properties::{SceneBindings, resolve_property},
    property_address::resolve_property_schema,
};

struct Dependencies<'a> {
    scenes: &'a HashMap<SceneId, SceneDefinition>,
    files: HashSet<MediaDependency>,
}

impl Dependencies<'_> {
    fn add_sources<'a>(
        &mut self,
        item: &TimelineItem,
        effect: Option<EffectInstanceId>,
        sources: impl Iterator<Item = MediaSource<'a>>,
        bindings: Option<&SceneBindings<'_>>,
    ) {
        for source in sources {
            let property = resolve_property_schema(self.scenes, item, effect, source.file)
                .expect("validated media file property");
            let value = resolve_property(bindings, item, effect, property, None)
                .expect("media file property belongs to this item")
                .value;
            if let Some(path) = value.file() {
                self.files.insert(MediaDependency {
                    path: path.to_owned(),
                    reader_id: source.reader.to_owned(),
                    target: source.input.target(),
                });
            }
        }
    }

    /// A clip limits export dependencies to occupied placements. Project-wide
    /// discovery omits it so unused definitions and trimmed sources stay known.
    fn collect(
        &mut self,
        items: Vec<(LayerId, TimelineItem)>,
        bindings: Option<&SceneBindings<'_>>,
        clip: Option<Range<u64>>,
    ) {
        for (_, item) in items {
            let child_clip = clip.as_ref().map(|parent| {
                let start = parent.start.saturating_add(item.start.get());
                start..parent.end.min(start.saturating_add(item.duration.get()))
            });
            if child_clip.as_ref().is_some_and(Range::is_empty) {
                continue;
            }
            if let Some(schema) = item.schema() {
                self.add_sources(&item, None, schema.media_sources(), bindings);
            }
            for effect in &item.effects {
                self.add_sources(
                    &item,
                    Some(effect.id),
                    effect.schema().media_sources(),
                    bindings,
                );
            }
            let Some(scene) = item.scene_id().and_then(|id| self.scenes.get(&id)) else {
                continue;
            };
            // Only file arguments are read by add_sources. Other arguments and
            // calculations remain lazy throughout the nested binding scopes.
            let child_bindings =
                SceneBindings::for_instance(self.scenes, scene, &item, None, bindings);
            let children = if child_clip.is_some() {
                scene.expanded_items(&item, None)
            } else {
                scene.document().source_items()
            };
            self.collect(children, Some(&child_bindings), child_clip);
        }
    }
}

impl TimelineSnapshot {
    /// Files used by occupied placements in the root export timeline, including
    /// effects attached to scene instances. No visual or gain values are read.
    pub fn media_dependencies(&self) -> HashSet<MediaDependency> {
        let mut dependencies = Dependencies {
            scenes: &self.project.scenes,
            files: HashSet::new(),
        };
        dependencies.collect(
            self.project.document.source_items(),
            None,
            Some(0..u64::MAX),
        );
        dependencies.files
    }

    /// Discover the project's file references, including unused scene defaults
    /// and instance overrides, without evaluating unrelated scene calculations.
    pub fn project_media_dependencies(&self) -> HashSet<MediaDependency> {
        let mut dependencies = Dependencies {
            scenes: &self.project.scenes,
            files: HashSet::new(),
        };
        dependencies.collect(self.project.document.source_items(), None, None);
        for scene in self.project.scenes.values() {
            let bindings = SceneBindings::new(&self.project.scenes, scene, None);
            dependencies.collect(scene.document().source_items(), Some(&bindings), None);
        }
        dependencies.files
    }
}
