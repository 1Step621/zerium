//! All public mutations of [`TimelineEditor`].
//!
//! This sibling module keeps the read-oriented editor API compact. The editor
//! internals it needs are visible only inside `timeline`.

use crate::domain::property::PropertyValueType;
use std::collections::{HashMap, HashSet};
use thiserror::Error;

use crate::domain::animation::{
    BezierHandle, ScalarAnimationAddress, ScalarAnimations, ScalarTrack, SegmentInterpolation,
};
use crate::domain::media::ImportedMedia;
use crate::domain::property::{
    PropertyConfiguration, PropertyElementId, PropertySchema, PropertyType, PropertyValue,
    ScalarPropertyType,
};

use super::{
    document::{ResizeEdge, TimelineDocument},
    editor::{HistoryKey, HistorySnapshot, TimelineEditor},
    evaluation::evaluate_expression_arguments,
    expression,
    ids::{EffectInstanceId, ItemId, LayerId, SceneId},
    item::TimelineItem,
    scene::{
        SceneArgument, SceneArgumentPreset, SceneBindingOwner, SceneBindingTarget, SceneDefinition,
        apply_scene_binding_to_item, materialize_scene_instance_properties,
        resolve_property_schema, resolve_scene_binding, scene_argument_expressions_valid,
        set_scene_instance_override, unique_scene_argument_name,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SceneArgumentEditError {
    NoActiveScene,
    ArgumentNotFound,
    TargetNotFound,
    TargetAlreadyBound,
    TargetAnimated,
    TargetNotBindable,
    IncompatibleContract,
    ReferencedByExpression,
}

/// An expected reason why an editor command could not be applied.
///
/// Commands that merely report whether they changed state still return
/// `bool`; commands that can reject valid-looking user input use this type so
/// the UI does not have to guess why they failed.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum TimelineEditError {
    #[error("アイテム '{plugin_id}/{item_id}' がありません")]
    PluginItemNotFound { plugin_id: String, item_id: String },
    #[error("エフェクト '{plugin_id}/{effect_id}' がありません")]
    PluginEffectNotFound {
        plugin_id: String,
        effect_id: String,
    },
    #[error("シーン {} がありません", .0.get())]
    SceneNotFound(SceneId),
    #[error("シーン参照が循環します")]
    RecursiveSceneReference,
    #[error("対象のアイテムが選択されていません")]
    NothingSelected,
    #[error("新しいIDを割り当てられません")]
    IdentifierExhausted,
    #[error("指定した位置にアイテムを配置できません")]
    PlacementUnavailable,
    #[error("アイテム {} がありません", .0.get())]
    ItemNotFound(ItemId),
    #[error("ファイルの種類がアイテム入力と一致しません")]
    IncompatibleMedia,
}

mod animation;
mod effect;
mod item;
mod property;
mod scene;
mod session;
