//! Frame-based audio control, shared by playback, meters, and export.
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::{Arc, OnceLock},
};

use super::{
    Frame, FrameDuration, FrameRate, ItemId, LayerId, SceneId, TimelineItem, TimelineTime,
    evaluation::{scene_runtime_seed, unique_runtime_id},
    project::TimelineProject,
    properties::{SceneBindings, resolve_property},
    visibility::PreviewVisibility,
};
use crate::{
    media::{MediaAsset, MediaPlayback},
    property::PropertySchema,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AudioClipId {
    pub item_id: ItemId,
    pub input_id: String,
}

/// Static decoder configuration. Gain is evaluated separately at authored frames.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioTimelineSource {
    pub id: AudioClipId,
    pub asset: MediaAsset,
    pub start: Frame,
    pub duration: FrameDuration,
    pub playback: MediaPlayback,
    pub preserve_pitch: bool,
}

/// A failed connection keeps its authored gain available for preview. Export
/// can report the diagnostic for precisely the stream it is consuming.
#[derive(Clone)]
pub struct EvaluatedAudioGain {
    pub value: f32,
    pub diagnostic: Option<String>,
}

#[derive(Clone)]
pub struct AudioTimelineSnapshot {
    project: Arc<TimelineProject>,
    media_cache: Arc<crate::media::MediaMetadataCache>,
    scene_id: Option<SceneId>,
    visibility: Option<PreviewVisibility>,
    plan: Arc<OnceLock<AudioPlan>>,
}

/// Both source scheduling and gain evaluation use these same expanded placements.
struct AudioPlan {
    nodes: Vec<AudioNode>,
    sources: Vec<AudioTimelineSource>,
}

struct AudioNode {
    item: TimelineItem,
    offset: u64,
    clip: Range<u64>,
    kind: AudioNodeKind,
}

enum AudioNodeKind {
    Scene {
        scene_id: SceneId,
        children: Vec<AudioNode>,
    },
    Source {
        inputs: Vec<GainInput>,
    },
}

struct GainInput {
    id: AudioClipId,
    property: PropertySchema,
}

#[derive(Clone, Copy)]
struct Placement {
    offset: u64,
    seed: u64,
    end: u64,
    root: bool,
}

struct PlanBuilder<'a> {
    snapshot: &'a AudioTimelineSnapshot,
    used_ids: HashSet<ItemId>,
    sources: Vec<AudioTimelineSource>,
}

impl PlanBuilder<'_> {
    fn expand(
        &mut self,
        items: Vec<(LayerId, TimelineItem)>,
        placement: Placement,
        bindings: Option<&SceneBindings<'_>>,
    ) -> Vec<AudioNode> {
        let mut nodes = Vec::new();
        for (layer, source) in items {
            if placement.root
                && self
                    .snapshot
                    .visibility
                    .as_ref()
                    .is_some_and(|visibility| !visibility.is_item_visible(layer, source.id))
            {
                continue;
            }
            let start = placement.offset.saturating_add(source.start.get());
            let end = placement
                .end
                .min(start.saturating_add(source.duration.get()));
            if end <= start {
                continue;
            }
            let scenes = &self.snapshot.project.scenes;
            let kind = if let Some(scene_id) = source.scene_id() {
                let Some(scene) = scenes.get(&scene_id) else {
                    continue;
                };
                let child_bindings = SceneBindings::for_instance(
                    scenes,
                    scene,
                    &source,
                    Some(TimelineTime::from_frame(source.start)),
                    bindings,
                );
                let children = self.expand(
                    scene.expanded_items(&source, None),
                    Placement {
                        offset: start,
                        seed: scene_runtime_seed(placement.seed, source.id, scene_id),
                        end,
                        root: false,
                    },
                    Some(&child_bindings),
                );
                if children.is_empty() {
                    continue;
                }
                AudioNodeKind::Scene { scene_id, children }
            } else {
                let inputs = self.source_inputs(&source, placement.seed, start..end, bindings);
                if inputs.is_empty() {
                    continue;
                }
                AudioNodeKind::Source { inputs }
            };
            nodes.push(AudioNode {
                item: source,
                offset: placement.offset,
                clip: start..end,
                kind,
            });
        }
        nodes
    }

    fn source_inputs(
        &mut self,
        source: &TimelineItem,
        seed: u64,
        clip: Range<u64>,
        bindings: Option<&SceneBindings<'_>>,
    ) -> Vec<GainInput> {
        let Some(schema) = source.schema().filter(|schema| !schema.audio().is_empty()) else {
            return Vec::new();
        };
        let runtime_id = unique_runtime_id(seed, source.id, &mut self.used_ids);
        // Source clocks and preserve-pitch are static by schema. Resolve only
        // decoder inputs; gain and visual properties do not belong here.
        let mut item = source.clone();
        let mut resolved = HashSet::new();
        let mut inputs = Vec::new();
        for input in schema.audio() {
            let playback = input.playback_properties();
            for id in [
                input.media_source().file,
                input.preserve_pitch_property(),
                playback.source_start,
                playback.source_duration,
                playback.playback_speed,
                playback.end_behavior,
            ] {
                if !resolved.insert(id) {
                    continue;
                }
                let property = schema
                    .property(id)
                    .expect("validated audio source property");
                let value = resolve_property(bindings, source, None, property, None)
                    .expect("audio property owner")
                    .value;
                item.properties
                    .set(property, value)
                    .expect("resolved audio source property");
            }
            let Some((asset, playback, preserve_pitch)) =
                item.audio_input(input.id(), &self.snapshot.media_cache)
            else {
                continue;
            };
            let id = AudioClipId {
                item_id: runtime_id,
                input_id: input.id().to_owned(),
            };
            let property = schema
                .property(input.volume_property())
                .expect("validated audio volume")
                .clone();
            inputs.push(GainInput {
                id: id.clone(),
                property,
            });
            self.sources.push(AudioTimelineSource {
                id,
                asset,
                start: Frame::new(clip.start),
                duration: FrameDuration::new_saturating(clip.end - clip.start),
                playback,
                preserve_pitch,
            });
        }
        inputs
    }
}

impl AudioTimelineSnapshot {
    pub(super) fn new(
        project: Arc<TimelineProject>,
        media_cache: Arc<crate::media::MediaMetadataCache>,
        scene_id: Option<SceneId>,
        visibility: Option<PreviewVisibility>,
    ) -> Self {
        Self {
            project,
            media_cache,
            scene_id,
            visibility,
            plan: Arc::default(),
        }
    }

    pub fn same_content(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.project, &other.project)
            && Arc::ptr_eq(&self.media_cache, &other.media_cache)
            && self.scene_id == other.scene_id
            && self.visibility == other.visibility
    }

    pub fn frame_rate(&self) -> FrameRate {
        self.project.document.frame_rate()
    }

    fn plan(&self) -> &AudioPlan {
        self.plan.get_or_init(|| {
            let scene = self.scene_id.and_then(|id| self.project.scenes.get(&id));
            let items = scene.map_or_else(
                || self.project.document.source_items(),
                |scene| scene.document().source_items(),
            );
            let bindings = scene.map(|scene| SceneBindings::new(&self.project.scenes, scene, None));
            let mut builder = PlanBuilder {
                snapshot: self,
                used_ids: HashSet::new(),
                sources: Vec::new(),
            };
            let nodes = builder.expand(
                items,
                Placement {
                    offset: 0,
                    seed: 0,
                    end: u64::MAX,
                    root: true,
                },
                bindings.as_ref(),
            );
            builder.sources.sort_unstable_by(|a, b| {
                (a.id.item_id.get(), &a.id.input_id).cmp(&(b.id.item_id.get(), &b.id.input_id))
            });
            AudioPlan {
                nodes,
                sources: builder.sources,
            }
        })
    }

    pub fn sources(&self) -> &[AudioTimelineSource] {
        &self.plan().sources
    }

    /// Evaluate only requested streams at an integer video frame. The mixer
    /// linearly interpolates these control values; scripts never run per sample.
    pub fn gains_at_frame(
        &self,
        frame: Frame,
        requested: &HashSet<AudioClipId>,
    ) -> HashMap<AudioClipId, EvaluatedAudioGain> {
        fn evaluate(
            snapshot: &AudioTimelineSnapshot,
            nodes: &[AudioNode],
            frame: Frame,
            requested: &HashSet<AudioClipId>,
            bindings: Option<&SceneBindings<'_>>,
            gains: &mut HashMap<AudioClipId, EvaluatedAudioGain>,
        ) {
            for node in nodes {
                if !node.clip.contains(&frame.get()) {
                    continue;
                }
                let time = TimelineTime::from_frame(Frame::new(frame.get() - node.offset));
                match &node.kind {
                    AudioNodeKind::Scene { scene_id, children } => {
                        let scene = &snapshot.project.scenes[scene_id];
                        let bindings = SceneBindings::for_instance(
                            &snapshot.project.scenes,
                            scene,
                            &node.item,
                            Some(time),
                            bindings,
                        );
                        evaluate(snapshot, children, frame, requested, Some(&bindings), gains);
                    }
                    AudioNodeKind::Source { inputs } => {
                        for input in inputs {
                            if !requested.contains(&input.id) {
                                continue;
                            }
                            let resolved = resolve_property(
                                bindings,
                                &node.item,
                                None,
                                &input.property,
                                Some(time),
                            )
                            .expect("audio volume belongs to this item");
                            let value = resolved
                                .value
                                .as_f32()
                                .expect("audio volume is f32")
                                .max(0.);
                            let diagnostic =
                                resolved.diagnostics.into_iter().next().map(|error| {
                                    format!("At audio frame {}: {error}", frame.get())
                                });
                            gains
                                .insert(input.id.clone(), EvaluatedAudioGain { value, diagnostic });
                        }
                    }
                }
            }
        }
        let mut gains = HashMap::new();
        if !requested.is_empty() {
            let bindings = self
                .scene_id
                .and_then(|id| self.project.scenes.get(&id))
                .map(|scene| SceneBindings::new(&self.project.scenes, scene, None));
            evaluate(
                self,
                &self.plan().nodes,
                frame,
                requested,
                bindings.as_ref(),
                &mut gains,
            );
        }
        gains
    }
}
