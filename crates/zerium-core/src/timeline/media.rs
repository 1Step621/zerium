//! File dependencies are resolved independently of rendering and audio controls.
use std::collections::{HashMap, HashSet};

use crate::media::{MediaDependency, MediaSource, MediaTarget};

use super::{
    EffectInstanceId, SceneDefinition, SceneId, TimelineItem, TimelineSnapshot,
    properties::{SceneBindings, resolve_property},
    property_address::resolve_property_schema,
};

struct Dependencies<'a> {
    scenes: &'a HashMap<SceneId, SceneDefinition>,
    files: HashSet<MediaDependency>,
    target: Option<MediaTarget>,
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
            if self
                .target
                .is_some_and(|target| target != source.input.target())
            {
                continue;
            }
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
}

impl super::placement::SceneVisitor for Dependencies<'_> {
    type State = ();
    type Output = ();

    fn enter(
        &mut self,
        placed: &super::placement::PlacedItem,
        _time: Option<super::TimelineTime>,
        bindings: Option<&SceneBindings<'_>>,
    ) {
        let item = &placed.source;
        if let Some(schema) = item.schema() {
            self.add_sources(item, None, schema.media_sources(), bindings);
        }
        for effect in &item.effects {
            self.add_sources(
                item,
                Some(effect.id),
                effect.schema().media_sources(),
                bindings,
            );
        }
    }

    fn exit(&mut self, _item: super::placement::PlacedItem, _: (), _: Vec<()>) -> Option<()> {
        None
    }
}

impl TimelineSnapshot {
    /// Visual sources reachable through root instances, and audio sources in
    /// their occupied intervals. Visual sampling can reach beyond an ancestor's
    /// output clip. No visual or gain values are read during file discovery.
    pub fn media_dependencies(&self) -> HashSet<MediaDependency> {
        let mut dependencies = Dependencies {
            scenes: &self.project.scenes,
            files: HashSet::new(),
            target: Some(MediaTarget::Visual),
        };
        let walker = super::placement::SceneWalker {
            frame_rate: self.project.document.frame_rate(),
            scenes: &self.project.scenes,
            visibility: None,
        };
        walker.walk(
            self.project.document.source_items(),
            super::placement::WalkMode::Definitions,
            None,
            &mut dependencies,
        );
        dependencies.target = Some(MediaTarget::Audio);
        walker.walk(
            self.project.document.source_items(),
            super::placement::WalkMode::Occupied,
            None,
            &mut dependencies,
        );
        dependencies.files
    }

    /// Discover the project's file references, including unused scene defaults
    /// and instance overrides, without evaluating unrelated scene calculations.
    pub fn project_media_dependencies(&self) -> HashSet<MediaDependency> {
        let mut dependencies = Dependencies {
            scenes: &self.project.scenes,
            files: HashSet::new(),
            target: None,
        };
        let walker = super::placement::SceneWalker {
            frame_rate: self.project.document.frame_rate(),
            scenes: &self.project.scenes,
            visibility: None,
        };
        walker.walk(
            self.project.document.source_items(),
            super::placement::WalkMode::Definitions,
            None,
            &mut dependencies,
        );
        for scene in self.project.scenes.values() {
            let bindings = SceneBindings::new(&self.project.scenes, scene, None);
            walker.walk(
                scene.document().source_items(),
                super::placement::WalkMode::Definitions,
                Some(&bindings),
                &mut dependencies,
            );
        }
        dependencies.files
    }
}
