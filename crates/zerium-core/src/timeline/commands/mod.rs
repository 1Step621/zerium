//! All public mutations of [`TimelineEditor`].
//!
//! This sibling module keeps the read-oriented editor API compact. The editor
//! internals it needs are visible only inside `timeline`.

use std::collections::{HashMap, HashSet};
use thiserror::Error;

use crate::animation::{BezierHandle, ScalarAnimations, ScalarTrack, SegmentInterpolation};
use crate::media::ImportedFile;
use crate::property::{PropertyConfiguration, PropertyElementId, PropertySchema, PropertyValue};

use super::{
    document::{ResizeEdge, ResizeMode, TimelineDocument},
    editor::{HistoryKey, HistorySnapshot, TimelineEditor},
    ids::{EffectInstanceId, ItemId, LayerId, SceneId},
    item::TimelineItem,
    scene::{
        SceneArgument, SceneArgumentPreset, SceneBindingOwner, SceneBindingTarget, SceneDefinition,
        apply_scene_binding_to_item, materialize_scene_instance_properties,
        resolve_property_schema, resolve_scene_binding, unique_scene_argument_name,
    },
    settings::ProjectResolution,
    time::{Frame, FrameDuration, FrameRate, TimelineTime},
};

fn remove_bindings_for_items(scene: &mut SceneDefinition, item_ids: &HashSet<ItemId>) {
    for argument in &mut scene.arguments {
        argument
            .bindings
            .retain(|binding| !item_ids.contains(&binding.item_id()));
    }
}

fn remove_bindings_for_nested_argument(
    scenes: &mut HashMap<SceneId, SceneDefinition>,
    nested_scene_id: SceneId,
    argument_id: &str,
) {
    let parents = scenes
        .iter()
        .map(|(scene_id, scene)| {
            let item_ids = scene
                .items()
                .filter(|item| item.scene_id() == Some(nested_scene_id))
                .map(|item| item.id)
                .collect::<HashSet<_>>();
            (*scene_id, item_ids)
        })
        .collect::<Vec<_>>();
    for (scene_id, item_ids) in parents {
        let Some(scene) = scenes.get_mut(&scene_id) else {
            continue;
        };
        for argument in &mut scene.arguments {
            argument.bindings.retain(|binding| {
                !(item_ids.contains(&binding.item_id())
                    && binding.owner() == SceneBindingOwner::Item
                    && binding.property_id() == argument_id)
            });
        }
    }
}

fn remove_scene_instances(document: &mut TimelineDocument, scene_id: SceneId) -> HashSet<ItemId> {
    let instance_ids = document
        .items()
        .filter(|item| item.scene_id() == Some(scene_id))
        .map(|item| item.id)
        .collect::<HashSet<_>>();
    for item_id in &instance_ids {
        let removed = document.remove_item(*item_id);
        debug_assert!(removed, "collected scene instance must still exist");
    }
    instance_ids
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum SceneArgumentEditError {
    #[error("No scene is active")]
    NoActiveScene,
    #[error("Scene argument was not found")]
    ArgumentNotFound,
    #[error("Binding target was not found")]
    TargetNotFound,
    #[error("Binding target is already connected")]
    TargetAlreadyBound,
    #[error("Binding target is animated")]
    TargetAnimated,
    #[error("Binding target does not support scene arguments")]
    TargetNotBindable,
    #[error("Value or binding does not match the argument type")]
    IncompatibleContract,
}

/// An expected reason why an editor command could not be applied.
///
/// Commands that merely report whether they changed state still return
/// `bool`; commands that can reject valid-looking user input use this type so
/// the UI does not have to guess why they failed.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum TimelineEditError {
    #[error("Item '{plugin_id}/{item_id}' was not found")]
    PluginItemNotFound { plugin_id: String, item_id: String },
    #[error("Effect '{plugin_id}/{effect_id}' was not found")]
    PluginEffectNotFound {
        plugin_id: String,
        effect_id: String,
    },
    #[error("Scene {} was not found", .0.get())]
    SceneNotFound(SceneId),
    #[error("Scene reference cycle detected")]
    RecursiveSceneReference,
    #[error("No target item is selected")]
    NothingSelected,
    #[error("Could not allocate a new ID")]
    IdentifierExhausted,
    #[error("Cannot place item at the specified position")]
    PlacementUnavailable,
    #[error("Item {} was not found", .0.get())]
    ItemNotFound(ItemId),
    #[error("File type does not match the item input")]
    IncompatibleMedia,
    #[error("Playback speed must be between 25% and 400%")]
    InvalidPlaybackSpeed,
    #[error("Source interval must have a finite nonnegative start and a finite positive duration")]
    InvalidSourceRange,
}

mod animation;
mod effect;
mod item;
mod property;
mod scene;
mod session;
