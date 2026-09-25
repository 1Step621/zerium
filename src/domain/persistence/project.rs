//! Project file representation, capture, and checked reconstruction.
use super::ProjectError;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::domain::animation::{ScalarAnimationAddress, ScalarAnimations, ScalarTrack};
use crate::domain::media::{MediaAsset, MediaKind};
use crate::domain::plugin::PluginRegistry;
use crate::domain::property::materialized_property_values;
use crate::domain::property::{
    PropertyElementId, PropertySchema, PropertyType, PropertyValue, PropertyValues,
};
use crate::domain::timeline::{
    EffectInstance, EffectInstanceId, Frame, FrameDuration, FrameRate, ItemId, LayerId, ProjectId,
    ProjectResolution, SceneArgument, SceneBindingTarget, SceneDefinition, SceneId,
    TimelineDocument, TimelineEditor, TimelineItem, TimelineItemKind, TimelineSnapshot,
    TimelineView, resolve_scene_binding,
};

pub(crate) const PROJECT_EXTENSION: &str = "zero";
const FORMAT_VERSION: u32 = 1;

pub(crate) struct LoadedProject {
    project_id: ProjectId,
    document: TimelineDocument,
    scenes: HashMap<SceneId, SceneDefinition>,
    resolution: ProjectResolution,
    playhead: Frame,
}

impl LoadedProject {
    pub(crate) fn apply(self, editor: &mut TimelineEditor) {
        editor.replace_project(
            self.project_id,
            self.document,
            self.scenes,
            self.resolution,
            self.playhead,
        );
    }
}

pub(crate) fn encode(
    snapshot: &TimelineSnapshot,
    project_path: &Path,
) -> Result<String, ProjectError> {
    let file = ProjectFile::capture(snapshot, project_path)?;
    serde_json::to_string_pretty(&file).map_err(|error| {
        ProjectError::encode(format!("プロジェクトを変換できません: {error}"), error)
    })
}

pub(crate) fn decode(
    source: &str,
    project_path: &Path,
    plugins: &PluginRegistry,
) -> Result<LoadedProject, ProjectError> {
    let file = serde_json::from_str::<ProjectFile>(source).map_err(|error| {
        ProjectError::invalid_format(
            format!("プロジェクトファイルの形式が不正です: {error}"),
            error,
        )
    })?;
    file.into_loaded(project_path, plugins)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectFile {
    format_version: u32,
    project_high: u64,
    project_low: u64,
    resolution: [u32; 2],
    frame_rate: [u32; 2],
    playhead: u64,
    items: Vec<ProjectItem>,
    scenes: Vec<ProjectScene>,
}

impl ProjectFile {
    fn capture(snapshot: &TimelineSnapshot, project_path: &Path) -> Result<Self, ProjectError> {
        let items = capture_items(snapshot.items(), |id| snapshot.item_layer(id), project_path)?;
        let mut scenes = snapshot
            .scenes()
            .map(|scene| {
                if scene.id.project() != snapshot.project_id() {
                    return Err(ProjectError::invalid_data(
                        "別プロジェクトのシーンを保存できません",
                    ));
                }
                ProjectScene::capture(scene, project_path)
            })
            .collect::<Result<Vec<_>, _>>()?;
        scenes.sort_by_key(|scene| scene.id);
        Ok(Self {
            format_version: FORMAT_VERSION,
            project_high: snapshot.project_id().high(),
            project_low: snapshot.project_id().low(),
            resolution: [
                snapshot.resolution().width(),
                snapshot.resolution().height(),
            ],
            frame_rate: [
                snapshot.frame_rate().numerator(),
                snapshot.frame_rate().denominator(),
            ],
            playhead: snapshot.playhead().get(),
            items,
            scenes,
        })
    }

    fn into_loaded(
        self,
        project_path: &Path,
        plugins: &PluginRegistry,
    ) -> Result<LoadedProject, ProjectError> {
        if self.format_version != FORMAT_VERSION {
            return Err(ProjectError::unsupported_format(format!(
                "未対応のプロジェクト形式です (version {})",
                self.format_version
            )));
        }
        let project_id = ProjectId::from_parts(self.project_high, self.project_low)
            .ok_or_else(|| ProjectError::invalid_data("プロジェクトIDが不正です"))?;
        let frame_rate = FrameRate::new(self.frame_rate[0], self.frame_rate[1])
            .ok_or_else(|| ProjectError::invalid_data("フレームレートが不正です"))?;
        let resolution = ProjectResolution::new(self.resolution[0], self.resolution[1])
            .ok_or_else(|| ProjectError::invalid_data("解像度が不正です"))?;
        let mut scene_ids = HashSet::new();
        let mut scene_schemas = HashMap::new();
        for scene in &self.scenes {
            if scene.id == 0 || scene.id == u64::MAX || !scene_ids.insert(scene.id) {
                return Err(ProjectError::invalid_data(format!(
                    "シーンID {} が不正または重複しています",
                    scene.id
                )));
            }
            if scene.name.trim().is_empty() {
                return Err(ProjectError::invalid_data("シーン名は空にできません"));
            }
            let mut argument_ids = HashSet::new();
            for argument in &scene.arguments {
                argument
                    .schema
                    .validate("scene", &scene.name)
                    .map_err(|error| {
                        ProjectError::invalid_data(format!(
                            "シーン '{}' の引数 '{}' が不正です: {error}",
                            scene.name, argument.schema.id
                        ))
                    })?;
                if argument.schema.clone().for_scene_argument().is_none() {
                    return Err(ProjectError::invalid_data(format!(
                        "シーン '{}' の引数 '{}' は対応するスカラー型ではありません",
                        scene.name, argument.schema.id
                    )));
                }
                if !argument_ids.insert(argument.schema.id.as_str()) {
                    return Err(ProjectError::invalid_data(format!(
                        "シーン '{}' の引数 '{}' が重複しています",
                        scene.name, argument.schema.id
                    )));
                }
            }
            scene_schemas.insert(
                SceneId::new(project_id, scene.id),
                scene
                    .arguments
                    .iter()
                    .map(|argument| argument.schema.clone())
                    .collect::<Vec<_>>(),
            );
        }

        let mut effect_ids = HashSet::new();
        let items = load_items(
            self.items,
            project_path,
            &scene_schemas,
            &mut effect_ids,
            plugins,
        )?;
        validate_no_overlaps(&items)?;
        let mut scenes = HashMap::new();
        for scene in self.scenes {
            let scene_id = SceneId::new(project_id, scene.id);
            let scene_items = load_items(
                scene.items,
                project_path,
                &scene_schemas,
                &mut effect_ids,
                plugins,
            )?;
            validate_no_overlaps(&scene_items)?;
            let arguments = scene
                .arguments
                .into_iter()
                .map(ProjectSceneArgument::into_domain)
                .collect::<Result<Vec<_>, _>>()?;
            scenes.insert(
                scene_id,
                SceneDefinition::from_project(
                    scene_id,
                    scene.name,
                    arguments,
                    TimelineDocument::from_items(frame_rate, scene_items),
                ),
            );
        }
        validate_scenes(&items, &scenes)?;
        Ok(LoadedProject {
            project_id,
            document: TimelineDocument::from_items(frame_rate, items),
            scenes,
            resolution,
            playhead: Frame::new(self.playhead),
        })
    }
}

fn capture_items<'a>(
    items: impl Iterator<Item = &'a TimelineItem>,
    layer_for: impl Fn(ItemId) -> Option<LayerId>,
    project_path: &Path,
) -> Result<Vec<ProjectItem>, ProjectError> {
    let mut captured = items
        .map(|item| {
            let layer = layer_for(item.id).ok_or_else(|| {
                ProjectError::invalid_data(format!(
                    "アイテム {} のレイヤー情報がありません",
                    item.id.get()
                ))
            })?;
            Ok(ProjectItem::capture(item, layer, project_path))
        })
        .collect::<Result<Vec<_>, ProjectError>>()?;
    captured.sort_by_key(|item| (item.layer, item.start, item.id));
    Ok(captured)
}

pub(super) fn load_items(
    items: Vec<ProjectItem>,
    project_path: &Path,
    scene_schemas: &HashMap<SceneId, Vec<PropertySchema>>,
    effect_ids: &mut HashSet<u64>,
    plugins: &PluginRegistry,
) -> Result<Vec<(LayerId, TimelineItem)>, ProjectError> {
    let mut item_ids = HashSet::new();
    items
        .into_iter()
        .map(|item| {
            if item.id == 0 || item.id == u64::MAX || !item_ids.insert(item.id) {
                return Err(ProjectError::invalid_data(format!(
                    "アイテムID {} が不正または重複しています",
                    item.id
                )));
            }
            item.into_timeline(project_path, effect_ids, scene_schemas, plugins)
        })
        .collect()
}

fn capture_property_overrides(
    values: &PropertyValues,
    schema: Option<&[PropertySchema]>,
) -> BTreeMap<String, PropertyValue> {
    values
        .iter()
        .filter(|(id, value)| {
            schema
                .and_then(|schema| schema.iter().find(|property| property.id() == *id))
                .is_none_or(|property| property.default_value() != *value)
        })
        .map(|(id, value)| (id.to_owned(), value.clone()))
        .collect()
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectScene {
    id: u64,
    name: String,
    arguments: Vec<ProjectSceneArgument>,
    items: Vec<ProjectItem>,
}

impl ProjectScene {
    fn capture(scene: &SceneDefinition, project_path: &Path) -> Result<Self, ProjectError> {
        Ok(Self {
            id: scene.id.get(),
            name: scene.name.clone(),
            arguments: scene
                .arguments
                .iter()
                .map(ProjectSceneArgument::capture)
                .collect(),
            items: capture_items(scene.items(), |id| scene.item_layer(id), project_path)?,
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectSceneArgument {
    schema: PropertySchema,
    bindings: Vec<SceneBindingTarget>,
}

impl ProjectSceneArgument {
    fn capture(argument: &SceneArgument) -> Self {
        Self {
            schema: argument.schema.clone(),
            bindings: argument.bindings.clone(),
        }
    }

    fn into_domain(self) -> Result<SceneArgument, ProjectError> {
        let schema = self.schema.for_scene_argument().ok_or_else(|| {
            ProjectError::invalid_data("シーン引数は対応するスカラー型ではありません")
        })?;
        Ok(SceneArgument::new(schema, self.bindings))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectItem {
    id: u64,
    layer: u64,
    start: u64,
    duration: u64,
    kind: ProjectItemKind,
    assets: BTreeMap<String, ProjectMediaAsset>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    properties: BTreeMap<String, PropertyValue>,
    animations: Vec<ProjectScalarAnimation>,
    aspect_ratio_locked: bool,
    effects: Vec<ProjectEffect>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ProjectItemKind {
    Plugin {
        plugin_id: String,
        item_id: String,
    },
    Scene {
        project_high: u64,
        project_low: u64,
        scene_id: u64,
    },
}

impl ProjectItem {
    pub(super) fn capture(item: &TimelineItem, layer: LayerId, project_path: &Path) -> Self {
        Self {
            id: item.id.get(),
            layer: layer.get(),
            start: item.start.get(),
            duration: item.duration.get(),
            kind: match item.scene_id() {
                Some(scene_id) => ProjectItemKind::Scene {
                    project_high: scene_id.project().high(),
                    project_low: scene_id.project().low(),
                    scene_id: scene_id.get(),
                },
                None => ProjectItemKind::Plugin {
                    plugin_id: item.plugin_id().unwrap_or_default().to_owned(),
                    item_id: item.item_id().unwrap_or_default().to_owned(),
                },
            },
            assets: item
                .assets
                .iter()
                .map(|(id, asset)| (id.clone(), ProjectMediaAsset::capture(asset, project_path)))
                .collect(),
            properties: capture_property_overrides(
                &item.properties,
                item.schema().map(|schema| schema.properties()),
            ),
            animations: capture_animations(&item.animations),
            aspect_ratio_locked: item.aspect_ratio_locked,
            effects: item
                .effects
                .iter()
                .map(|effect| ProjectEffect::capture(effect, project_path))
                .collect(),
        }
    }

    fn into_timeline(
        self,
        project_path: &Path,
        effect_ids: &mut HashSet<u64>,
        scene_schemas: &HashMap<SceneId, Vec<PropertySchema>>,
        plugins: &PluginRegistry,
    ) -> Result<(LayerId, TimelineItem), ProjectError> {
        let duration = FrameDuration::new(self.duration)
            .ok_or_else(|| ProjectError::invalid_data("アイテムの長さは1フレーム以上必要です"))?;
        self.start
            .checked_add(self.duration)
            .ok_or_else(|| ProjectError::invalid_data("アイテムの時刻が大きすぎます"))?;
        let plugin_schema = match &self.kind {
            ProjectItemKind::Scene { .. } => None,
            ProjectItemKind::Plugin { plugin_id, item_id } => {
                Some(plugins.item(plugin_id, item_id).ok_or_else(|| {
                    ProjectError::invalid_data(format!(
                        "アイテム '{}:{}' を提供するプラグインがありません",
                        plugin_id, item_id
                    ))
                })?)
            }
        };
        let property_schema = match &self.kind {
            ProjectItemKind::Scene {
                project_high,
                project_low,
                scene_id,
            } => {
                let project =
                    ProjectId::from_parts(*project_high, *project_low).ok_or_else(|| {
                        ProjectError::invalid_data("参照先のプロジェクトIDが不正です")
                    })?;
                let scene = SceneId::new(project, *scene_id);
                scene_schemas.get(&scene).ok_or_else(|| {
                    ProjectError::invalid_data(format!("参照先のシーン {scene_id} がありません"))
                })?
            }
            ProjectItemKind::Plugin { .. } => plugin_schema
                .as_deref()
                .expect("plugin item has a schema")
                .properties(),
        };
        let scene_instance = matches!(&self.kind, ProjectItemKind::Scene { .. });
        let initial = if scene_instance {
            PropertyValues::default()
        } else {
            PropertyValues::from_properties(property_schema)
        };
        let properties = load_properties(
            property_schema,
            self.properties,
            if scene_instance {
                "シーンインスタンス"
            } else {
                "アイテム"
            },
            initial,
        )?;
        let animation_base = materialized_property_values(&properties, property_schema);
        let animations = load_animations(property_schema, &animation_base, self.animations)?;

        let mut assets = std::collections::HashMap::with_capacity(self.assets.len());
        for (input_id, asset) in self.assets {
            let asset = asset.into_media(project_path)?;
            if let Some(schema) = plugin_schema.as_deref() {
                let capability = schema.file(&input_id).ok_or_else(|| {
                    ProjectError::invalid_data(format!("不明なファイル入力 '{input_id}' です"))
                })?;
                if capability.reader() != asset.reader_id
                    || capability.media_type() != asset.kind.media_type()
                {
                    return Err(ProjectError::invalid_data(format!(
                        "ファイル入力 '{input_id}' の種類がプラグイン定義と一致しません"
                    )));
                }
            }
            assets.insert(input_id, asset);
        }

        let mut effects = Vec::with_capacity(self.effects.len());
        for effect in self.effects {
            if effect.id == 0 || effect.id == u64::MAX || !effect_ids.insert(effect.id) {
                return Err(ProjectError::invalid_data(format!(
                    "エフェクトID {} が不正または重複しています",
                    effect.id
                )));
            }
            effects.push(effect.into_effect(project_path, plugins)?);
        }
        if !effects.is_empty()
            && plugin_schema
                .as_ref()
                .is_some_and(|schema| schema.shader().is_none())
        {
            return Err(ProjectError::invalid_data(
                "映像を持たないアイテムにはエフェクトを設定できません",
            ));
        }

        Ok((
            LayerId::new(self.layer),
            TimelineItem {
                id: ItemId(self.id),
                start: Frame::new(self.start),
                duration,
                kind: match self.kind {
                    ProjectItemKind::Scene {
                        project_high,
                        project_low,
                        scene_id,
                    } => TimelineItemKind::Scene {
                        scene_id: SceneId::new(
                            ProjectId::from_parts(project_high, project_low)
                                .expect("scene project identity was validated"),
                            scene_id,
                        ),
                    },
                    ProjectItemKind::Plugin { plugin_id, item_id } => TimelineItemKind::Plugin {
                        plugin_id,
                        item_id,
                        schema: plugin_schema.expect("plugin items were required to have a schema"),
                    },
                },
                assets,
                properties,
                animations,
                aspect_ratio_locked: self.aspect_ratio_locked,
                effects,
            },
        ))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectEffect {
    id: u64,
    plugin_id: String,
    effect_id: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    assets: BTreeMap<String, ProjectMediaAsset>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    properties: BTreeMap<String, PropertyValue>,
    animations: Vec<ProjectScalarAnimation>,
}

impl ProjectEffect {
    fn capture(effect: &EffectInstance, project_path: &Path) -> Self {
        Self {
            id: effect.id.get(),
            plugin_id: effect.plugin_id.clone(),
            effect_id: effect.effect_id.clone(),
            assets: effect
                .assets
                .iter()
                .map(|(id, asset)| (id.clone(), ProjectMediaAsset::capture(asset, project_path)))
                .collect(),
            properties: capture_property_overrides(
                &effect.properties,
                Some(effect.schema().properties()),
            ),
            animations: capture_animations(&effect.animations),
        }
    }

    fn into_effect(
        self,
        project_path: &Path,
        plugins: &PluginRegistry,
    ) -> Result<EffectInstance, ProjectError> {
        let schema = plugins
            .effect(&self.plugin_id, &self.effect_id)
            .ok_or_else(|| {
                ProjectError::invalid_data(format!(
                    "エフェクト '{}:{}' を提供するプラグインがありません",
                    self.plugin_id, self.effect_id
                ))
            })?;
        let properties = load_properties(
            schema.properties(),
            self.properties,
            "エフェクト",
            PropertyValues::from_properties(schema.properties()),
        )?;
        let animations = load_animations(schema.properties(), &properties, self.animations)?;
        let mut assets = HashMap::new();
        for (input_id, stored) in self.assets {
            let file = schema
                .files()
                .find(|file| file.id() == input_id)
                .ok_or_else(|| {
                    ProjectError::invalid_data(format!(
                        "エフェクトの入力 '{input_id}' が見つかりません"
                    ))
                })?;
            let asset = stored.into_media(project_path)?;
            if file.reader() != asset.reader_id || file.media_type() != asset.kind.media_type() {
                return Err(ProjectError::invalid_data(format!(
                    "エフェクトの入力 '{input_id}' とメディアが一致しません"
                )));
            }
            assets.insert(input_id, asset);
        }
        Ok(EffectInstance {
            id: EffectInstanceId::new(self.id),
            plugin_id: self.plugin_id,
            effect_id: self.effect_id,
            assets,
            properties,
            animations,
            schema,
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectScalarAnimation {
    #[serde(flatten)]
    address: ScalarAnimationAddress,
    track: ScalarTrack,
}

fn capture_animations(animations: &ScalarAnimations) -> Vec<ProjectScalarAnimation> {
    animations
        .tracks()
        .map(|(address, track)| ProjectScalarAnimation {
            address: address.clone(),
            track: track.clone(),
        })
        .collect()
}

fn load_animations(
    schema: &[PropertySchema],
    properties: &PropertyValues,
    animations: Vec<ProjectScalarAnimation>,
) -> Result<ScalarAnimations, ProjectError> {
    let mut loaded = ScalarAnimations::default();
    for animation in animations {
        let property = schema
            .iter()
            .find(|property| property.id == animation.address.property_id())
            .ok_or_else(|| {
                ProjectError::invalid_data(format!(
                    "アニメーション対象 '{}' が見つかりません",
                    animation.address.property_id()
                ))
            })?;
        validate_project_track(
            &mut loaded,
            properties,
            property,
            animation.address.property_id(),
            animation.address.element_id(),
            animation.address.scalar_index(),
            animation.track,
        )?;
    }
    Ok(loaded)
}

fn validate_project_track(
    loaded: &mut ScalarAnimations,
    properties: &PropertyValues,
    property: &PropertySchema,
    property_id: &str,
    element_id: Option<PropertyElementId>,
    scalar_index: Option<usize>,
    track: ScalarTrack,
) -> Result<(), ProjectError> {
    let Some(scalar) = properties
        .property(property_id)
        .and_then(|value| value.element(element_id))
        .and_then(|value| value.scalar_at(scalar_index))
    else {
        return Err(ProjectError::invalid_data(format!(
            "'{property_id}' のアニメーション対象が不正です"
        )));
    };
    let value_type = match (element_id, property.ty()) {
        (Some(_), PropertyType::Array { element_type, .. })
        | (None, PropertyType::Value(element_type)) => Some(element_type),
        _ => None,
    };
    let valid = value_type
        .and_then(|value_type| {
            value_type
                .scalar_at(scalar_index)
                .map(|scalar_ty| (scalar, scalar_ty))
        })
        .is_some_and(|(scalar, scalar_ty)| {
            property.is_animatable(scalar_index)
                && track.is_valid_for(scalar_ty)
                && scalar_ty.allows(scalar)
                && track.stops().iter().all(|stop| {
                    property
                        .configuration_constraints(scalar_index)
                        .allows(stop.value())
                })
        });
    let address = ScalarAnimationAddress::new(property_id, element_id, scalar_index);
    if !valid || !loaded.insert(address, track) {
        return Err(ProjectError::invalid_data(format!(
            "'{property_id}' のアニメーション対象が不正です"
        )));
    }
    Ok(())
}

fn load_properties(
    schema: &[PropertySchema],
    values: BTreeMap<String, PropertyValue>,
    owner: &str,
    mut loaded: PropertyValues,
) -> Result<PropertyValues, ProjectError> {
    for (id, value) in values {
        let property = schema
            .iter()
            .find(|property| property.id == id)
            .ok_or_else(|| {
                ProjectError::invalid_data(format!("{owner}に不明なパラメータ '{id}' があります"))
            })?;
        if &value == property.default_value() {
            continue;
        }
        loaded.set(property, value).map_err(|error| {
            ProjectError::invalid_data(format!("{owner}パラメータ '{id}' が不正です: {error}"))
        })?;
    }
    Ok(loaded)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectMediaAsset {
    reader_id: String,
    path: PathBuf,
    name: String,
    duration_seconds: u64,
    duration_nanoseconds: u32,
    kind: MediaKind,
}

impl ProjectMediaAsset {
    fn capture(asset: &MediaAsset, project_path: &Path) -> Self {
        let path = make_relative(&asset.path, project_path);
        Self {
            reader_id: asset.reader_id.clone(),
            path,
            name: asset.name.clone(),
            duration_seconds: asset.duration.as_secs(),
            duration_nanoseconds: asset.duration.subsec_nanos(),
            kind: asset.kind.clone(),
        }
    }

    fn into_media(self, project_path: &Path) -> Result<MediaAsset, ProjectError> {
        if self.duration_nanoseconds >= 1_000_000_000 {
            return Err(ProjectError::invalid_data("メディアの長さが不正です"));
        }
        let asset = MediaAsset {
            reader_id: self.reader_id,
            path: resolve_path(&self.path, project_path),
            name: self.name,
            duration: Duration::new(self.duration_seconds, self.duration_nanoseconds),
            kind: self.kind,
        };
        asset.validate().map_err(|error| {
            ProjectError::invalid_data(format!("メディア情報が不正です: {error}"))
        })?;
        Ok(asset)
    }
}

pub(super) fn validate_no_overlaps(items: &[(LayerId, TimelineItem)]) -> Result<(), ProjectError> {
    let mut ranges = items
        .iter()
        .map(|(layer, item)| (layer.get(), item.start.get(), item.end_exclusive().get()))
        .collect::<Vec<_>>();
    ranges.sort_unstable();
    for pair in ranges.windows(2) {
        if pair[0].0 == pair[1].0 && pair[0].2 > pair[1].1 {
            return Err(ProjectError::invalid_data(format!(
                "レイヤー {} でアイテムが重なっています",
                pair[0].0
            )));
        }
    }
    Ok(())
}

fn validate_scenes(
    root_items: &[(LayerId, TimelineItem)],
    scenes: &HashMap<SceneId, SceneDefinition>,
) -> Result<(), ProjectError> {
    let mut names = HashSet::new();
    for scene in scenes.values() {
        if !names.insert(scene.name.as_str()) {
            return Err(ProjectError::invalid_data(format!(
                "シーン名 '{}' が重複しています",
                scene.name
            )));
        }
        let mut bound_targets = HashSet::new();
        for argument in &scene.arguments {
            for binding in &argument.bindings {
                if !bound_targets.insert(binding) {
                    return Err(ProjectError::invalid_data(format!(
                        "シーン '{}' で同じ接続先が複数回使われています",
                        scene.name
                    )));
                }
                let property_id = binding.property_id();
                let resolved = resolve_scene_binding(scenes, scene, binding).ok_or_else(|| {
                    ProjectError::invalid_data(format!(
                        "シーン '{}' の引数接続先 '{}' がありません",
                        scene.name, property_id
                    ))
                })?;
                if !resolved.schema.is_scene_bindable(None) {
                    return Err(ProjectError::invalid_data(format!(
                        "シーン '{}' の接続先 '{}' はシーン引数へ公開できません",
                        scene.name, property_id
                    )));
                }
                if resolved.schema.ty() != argument.schema.ty() {
                    return Err(ProjectError::invalid_data(format!(
                        "シーン '{}' の引数 '{}' と接続先の型が一致しません",
                        scene.name,
                        argument.schema.id()
                    )));
                }
                if resolved.animated {
                    return Err(ProjectError::invalid_data(format!(
                        "アニメーション済みの '{}' にはシーン引数を接続できません",
                        property_id
                    )));
                }
            }
        }
    }

    let validate_instances = |items: Vec<&TimelineItem>| -> Result<(), ProjectError> {
        for item in items {
            if let Some(scene_id) = item.scene_id() {
                let scene = scenes.get(&scene_id).ok_or_else(|| {
                    ProjectError::invalid_data(format!(
                        "参照先のシーン {} がありません",
                        scene_id.get()
                    ))
                })?;
                if item.duration.get() > scene.duration().get() {
                    return Err(ProjectError::invalid_data(format!(
                        "シーン '{}' のインスタンスが本来の長さを超えています",
                        scene.name
                    )));
                }
            }
        }
        Ok(())
    };
    validate_instances(root_items.iter().map(|(_, item)| item).collect())?;
    for scene in scenes.values() {
        validate_instances(scene.items().collect())?;
    }

    fn visit(
        scene_id: SceneId,
        scenes: &HashMap<SceneId, SceneDefinition>,
        visiting: &mut HashSet<SceneId>,
        visited: &mut HashSet<SceneId>,
    ) -> Result<(), ProjectError> {
        if visited.contains(&scene_id) {
            return Ok(());
        }
        if !visiting.insert(scene_id) {
            return Err(ProjectError::invalid_data("シーン参照が循環しています"));
        }
        let scene = scenes
            .get(&scene_id)
            .ok_or_else(|| ProjectError::invalid_data("参照先のシーンがありません"))?;
        for nested in scene.items().filter_map(TimelineItem::scene_id) {
            visit(nested, scenes, visiting, visited)?;
        }
        visiting.remove(&scene_id);
        visited.insert(scene_id);
        Ok(())
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for scene_id in scenes.keys().copied() {
        visit(scene_id, scenes, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn make_relative(path: &Path, project_path: &Path) -> PathBuf {
    let Some(directory) = project_path.parent() else {
        return path.to_path_buf();
    };
    path.strip_prefix(directory)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

fn resolve_path(path: &Path, project_path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    project_path
        .parent()
        .map(|directory| directory.join(path))
        .unwrap_or_else(|| path.to_path_buf())
}
