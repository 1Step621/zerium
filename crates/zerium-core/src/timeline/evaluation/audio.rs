//! Frame-based audio control, shared by playback, meters, and export.

use super::scene::{
    properties::{SceneBindings, resolve_property},
    unique_runtime_id,
    visibility::PreviewVisibility,
};
use crate::{
    media::{MediaAsset, MediaPlayback},
    property::PropertySchema,
    timeline::{
        CalculationDiagnostic, Frame, FrameDuration, FrameRate, ItemId, SceneId, TimelineItem,
        TimelineTime, model::project::TimelineProject,
    },
};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::{Arc, OnceLock},
};
use thiserror::Error;

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
    pub diagnostic: Option<crate::timeline::AudioCalculationDiagnostic>,
}

#[derive(Clone)]
pub struct AudioTimelineSnapshot {
    project: Arc<TimelineProject>,
    media_cache: Arc<crate::media::MediaMetadataCache>,
    scene_id: SceneId,
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

struct PlanBuilder<'a> {
    snapshot: &'a AudioTimelineSnapshot,
    used_ids: HashSet<ItemId>,
    sources: Vec<AudioTimelineSource>,
}

impl PlanBuilder<'_> {
    fn source_inputs(
        &mut self,
        source: &TimelineItem,
        seed: u64,
        clip: Range<u64>,
        bindings: &SceneBindings<'_>,
    ) -> Vec<GainInput> {
        let Some(schema) = source.schema().filter(|schema| !schema.audio().is_empty()) else {
            return Vec::new();
        };
        let runtime_id = unique_runtime_id(seed, source.id, &mut self.used_ids);
        // Source clocks and preserve-pitch are static by schema. Resolve only
        // decoder inputs; gain and visual properties do not belong here.
        let mut properties = crate::property::PropertyValues::default();
        let mut resolved = HashSet::new();
        let mut inputs = Vec::new();
        for input in schema.audio() {
            for id in [input.media_source().file, input.preserve_pitch_property()] {
                if !resolved.insert(id) {
                    continue;
                }
                let property = schema
                    .property(id)
                    .expect("validated audio source property");
                let value = resolve_property(bindings, source, None, property, None)
                    .expect("audio property owner")
                    .value;
                properties
                    .set(property, value)
                    .expect("resolved audio source property");
            }
            let Some((asset, playback, preserve_pitch)) =
                crate::timeline::evaluation::scene::item::audio_input_for(
                    schema,
                    input.id(),
                    &properties,
                    source.playback(),
                    &self.snapshot.media_cache,
                )
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

impl crate::timeline::evaluation::scene::placement::SceneVisitor for PlanBuilder<'_> {
    type State = Vec<GainInput>;
    type Output = AudioNode;

    fn enter(
        &mut self,
        item: &crate::timeline::evaluation::scene::placement::PlacedItem,
        _time: Option<TimelineTime>,
        bindings: &SceneBindings<'_>,
    ) -> Self::State {
        if item.source.scene_id().is_some() {
            Vec::new()
        } else {
            self.source_inputs(
                &item.source,
                item.seed,
                item.clip.start.get()..item.clip.end_exclusive().get(),
                bindings,
            )
        }
    }

    fn exit(
        &mut self,
        item: crate::timeline::evaluation::scene::placement::PlacedItem,
        inputs: Self::State,
        children: Vec<AudioNode>,
    ) -> Option<AudioNode> {
        let kind = if let Some(scene_id) = item.source.scene_id() {
            if children.is_empty() {
                return None;
            }
            AudioNodeKind::Scene { scene_id, children }
        } else {
            if inputs.is_empty() {
                return None;
            }
            AudioNodeKind::Source { inputs }
        };
        Some(AudioNode {
            item: item.source,
            offset: item.offset,
            clip: item.clip.start.get()..item.clip.end_exclusive().get(),
            kind,
        })
    }
}

impl AudioTimelineSnapshot {
    pub(in crate::timeline) fn new(
        project: Arc<TimelineProject>,
        media_cache: Arc<crate::media::MediaMetadataCache>,
        scene_id: SceneId,
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
        self.project.main_scene().document().frame_rate()
    }

    fn plan(&self) -> &AudioPlan {
        self.plan.get_or_init(|| {
            let scene = &self.project.scenes[&self.scene_id];
            let items = scene.document().source_items();
            let bindings = SceneBindings::new(&self.project.scenes, scene, None);
            let mut builder = PlanBuilder {
                snapshot: self,
                used_ids: HashSet::new(),
                sources: Vec::new(),
            };
            let nodes = crate::timeline::evaluation::scene::placement::SceneWalker {
                frame_rate: self.frame_rate(),
                scenes: &self.project.scenes,
                visibility: self.visibility.as_ref(),
            }
            .walk(
                items,
                crate::timeline::evaluation::scene::placement::WalkMode::AudioPlan,
                &bindings,
                &mut builder,
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
            bindings: &SceneBindings<'_>,
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
                        evaluate(snapshot, children, frame, requested, &bindings, gains);
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
                            let diagnostic = resolved.diagnostics.into_iter().next().map(|error| {
                                crate::timeline::AudioCalculationDiagnostic {
                                    frame,
                                    clip: input.id.clone(),
                                    diagnostic: error,
                                }
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
            let bindings = SceneBindings::new(
                &self.project.scenes,
                &self.project.scenes[&self.scene_id],
                None,
            );
            evaluate(
                self,
                &self.plan().nodes,
                frame,
                requested,
                &bindings,
                &mut gains,
            );
        }
        gains
    }
}

impl crate::timeline::TimelineSnapshot {
    pub fn audio_snapshot(&self) -> crate::timeline::AudioTimelineSnapshot {
        crate::timeline::AudioTimelineSnapshot::new(
            self.project.clone(),
            self.media_cache.clone(),
            self.project.main_scene,
            None,
        )
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("At audio frame {frame}: {diagnostic}", frame = frame.get())]
pub struct AudioCalculationDiagnostic {
    pub frame: Frame,
    pub clip: crate::timeline::AudioClipId,
    #[source]
    pub diagnostic: CalculationDiagnostic,
}
