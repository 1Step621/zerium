use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::domain::{
    animation::BezierHandle,
    plugin::PluginRegistry,
    property::{PropertyElementId, PropertyValue},
};

use super::{
    document::{ResizeEdge, TimelineDocument},
    evaluation::{
        evaluated_scene_argument_values, evaluated_visible_document_items_at_time,
        visibility_filtered_document_items,
    },
    history::EditHistory,
    ids::{EffectInstanceId, ItemId, LayerId, ProjectId, SceneId},
    item::TimelineItem,
    project::TimelineProject,
    scene::{SceneDefinition, materialize_scene_instance_properties},
    selection::SelectionState,
    settings::{ProjectResolution, ProjectSettingsError},
    time::{Frame, FrameRate, TimelineTime},
    view::TimelineSnapshot,
    visibility::PreviewVisibility,
};

const HISTORY_LIMIT: usize = 100;
const HISTORY_COALESCE_INTERVAL: Duration = Duration::from_millis(750);

fn new_project_id() -> ProjectId {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let high = (timestamp >> 64) as u64;
    let low = timestamp as u64
        ^ u64::from(std::process::id()).rotate_left(32)
        ^ COUNTER.fetch_add(1, Ordering::Relaxed).rotate_left(17);
    ProjectId::from_parts(high, low).expect("generated project identity must be non-zero")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum HistoryKey {
    ItemCreation(ItemId),
    Property(Vec<(ItemId, Option<EffectInstanceId>)>, String),
    AspectRatioLock(Vec<(ItemId, Option<EffectInstanceId>)>),
    AnimationStopValue(
        ItemId,
        Option<EffectInstanceId>,
        String,
        Option<PropertyElementId>,
        Option<usize>,
        Frame,
    ),
    AnimationPairStopValue(
        ItemId,
        Option<EffectInstanceId>,
        String,
        Option<PropertyElementId>,
        Frame,
    ),
    AnimationHandle(
        ItemId,
        Option<EffectInstanceId>,
        String,
        Option<PropertyElementId>,
        Option<usize>,
        usize,
        BezierHandle,
    ),
    AnimationStopPosition(
        ItemId,
        Option<EffectInstanceId>,
        String,
        Option<PropertyElementId>,
        Option<usize>,
        usize,
    ),
    ItemResize(ItemId, ResizeEdge),
    ItemsResize(Vec<ItemId>, ResizeEdge),
    ItemMove(ItemId),
    ItemsMove(Vec<ItemId>),
    SceneName(SceneId),
    SceneArgumentLabel(SceneId, String),
    SceneArgumentSettings(SceneId, String),
}

#[derive(Clone)]
pub(super) struct HistorySnapshot {
    project: Arc<TimelineProject>,
    scene_path: Vec<SceneId>,
    playhead: Frame,
    selection: SelectionState,
    project_revision: u64,
}

pub(super) type ScopedHistoryKey = (Option<SceneId>, HistoryKey);

/// Coordinates timeline documents, editor session state, history, and commands.
///
/// Persistent item/layer invariants belong to `TimelineDocument`; this type owns
/// cross-document concerns such as scene navigation, revisions, and undo/redo.
pub(crate) struct TimelineEditor {
    pub(super) plugins: Arc<PluginRegistry>,
    project: Arc<TimelineProject>,
    pub(super) scene_path: Vec<SceneId>,
    pub(super) next_scene_id: Option<u64>,
    pub(super) next_effect_id: Option<u64>,
    pub(super) playhead: Frame,
    pub(super) playback_time: Option<TimelineTime>,
    pub(super) selection: SelectionState,
    /// None edits the selected item's source; Some edits an effect on that item.
    pub(super) active_edit_target: Option<(ItemId, EffectInstanceId)>,
    pub(super) visibility: PreviewVisibility,
    render_revision: u64,
    project_revision: u64,
    next_project_revision: u64,
    pub(super) history: EditHistory<HistorySnapshot, ScopedHistoryKey>,
}

impl TimelineEditor {
    pub(super) fn project(&self) -> &TimelineProject {
        &self.project
    }

    pub(super) fn project_mut(&mut self) -> &mut TimelineProject {
        Arc::make_mut(&mut self.project)
    }

    pub(super) fn commit_project_state(&mut self, project: TimelineProject) {
        self.project = Arc::new(project);
    }

    pub(crate) fn new(frame_rate: FrameRate, plugins: Arc<PluginRegistry>) -> Self {
        Self::from_document(
            TimelineDocument::new(frame_rate),
            ProjectResolution::DEFAULT,
            plugins,
        )
    }

    pub(crate) fn from_document(
        document: TimelineDocument,
        resolution: ProjectResolution,
        plugins: Arc<PluginRegistry>,
    ) -> Self {
        let next_effect_id = Self::next_effect_id(&document, std::iter::empty());
        Self {
            plugins,
            project: Arc::new(TimelineProject {
                id: new_project_id(),
                document,
                scenes: HashMap::new(),
                resolution,
            }),
            scene_path: Vec::new(),
            next_scene_id: Some(1),
            next_effect_id,
            playhead: Frame::new(0),
            playback_time: None,
            selection: SelectionState::default(),
            active_edit_target: None,
            visibility: PreviewVisibility::default(),
            render_revision: 0,
            project_revision: 0,
            next_project_revision: 1,
            history: EditHistory::new(HISTORY_LIMIT),
        }
    }

    pub(super) fn advance_render_revision(&mut self) {
        self.render_revision = self.render_revision.saturating_add(1);
    }

    fn next_effect_id<'a>(
        document: &'a TimelineDocument,
        scenes: impl Iterator<Item = &'a SceneDefinition>,
    ) -> Option<u64> {
        document
            .items()
            .chain(scenes.flat_map(SceneDefinition::items))
            .flat_map(|item| &item.effects)
            .map(|effect| effect.id.get())
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .filter(|id| *id != u64::MAX)
    }

    fn advance_project_revision(&mut self) {
        self.project_revision = self.next_project_revision;
        self.next_project_revision = self.next_project_revision.saturating_add(1);
        self.advance_render_revision();
    }

    pub(crate) fn project_revision(&self) -> u64 {
        self.project_revision
    }

    pub(crate) fn plugin_registry(&self) -> &PluginRegistry {
        &self.plugins
    }

    pub(crate) fn plugin_registry_arc(&self) -> Arc<PluginRegistry> {
        self.plugins.clone()
    }

    pub(crate) fn snapshot(&self) -> TimelineSnapshot {
        TimelineSnapshot {
            project: self.project.clone(),
            playhead: self.playhead,
            project_revision: self.project_revision,
        }
    }

    pub(super) fn history_snapshot(&self) -> HistorySnapshot {
        HistorySnapshot {
            project: self.project.clone(),
            scene_path: self.scene_path.clone(),
            playhead: self.playhead,
            selection: self.selection.clone(),
            project_revision: self.project_revision,
        }
    }

    pub(super) fn history_snapshot_for_edit(
        &self,
        key: Option<&HistoryKey>,
    ) -> Option<HistorySnapshot> {
        let scoped_key = key.map(|key| (self.active_scene_id(), key.clone()));
        self.history
            .begins_group(scoped_key.as_ref(), HISTORY_COALESCE_INTERVAL)
            .then(|| self.history_snapshot())
    }

    pub(super) fn finish_project_edit(
        &mut self,
        before: Option<HistorySnapshot>,
        key: Option<HistoryKey>,
    ) {
        let scoped_key = key.map(|key| (self.active_scene_id(), key));
        self.history.record(before, scoped_key);
        self.advance_project_revision();
    }

    pub(super) fn finish_project_edit_if_changed(
        &mut self,
        changed: bool,
        before: Option<HistorySnapshot>,
        key: Option<HistoryKey>,
    ) -> bool {
        if changed {
            self.finish_project_edit(before, key);
        }
        changed
    }

    pub(super) fn restore_history_snapshot(&mut self, snapshot: HistorySnapshot) {
        let location_changed = self.scene_path != snapshot.scene_path;
        let frame_rate_changed = self.frame_rate() != snapshot.project.document.frame_rate();
        self.project = snapshot.project;
        self.scene_path = snapshot
            .scene_path
            .into_iter()
            .take_while(|id| self.project().scenes.contains_key(id))
            .collect();
        if location_changed || frame_rate_changed {
            self.playhead = snapshot.playhead;
        }
        if location_changed {
            self.visibility.clear();
        }
        self.selection = snapshot.selection;
        self.project_revision = snapshot.project_revision;
        self.playback_time = None;
        self.advance_render_revision();
    }

    pub(crate) fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub(crate) fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub(crate) fn replace_document(
        &mut self,
        document: TimelineDocument,
        resolution: ProjectResolution,
        playhead: Frame,
    ) {
        {
            let project = self.project_mut();
            project.id = new_project_id();
            project.document = document;
            project.resolution = resolution;
            project.scenes.clear();
        }
        self.scene_path.clear();
        self.next_scene_id = Some(1);
        self.next_effect_id = Self::next_effect_id(&self.project().document, std::iter::empty());
        self.playhead = playhead;
        self.playback_time = None;
        self.selection.clear();
        self.active_edit_target = None;
        self.visibility.clear();
        self.project_revision = 0;
        self.next_project_revision = 1;
        self.history.clear();
        self.advance_render_revision();
    }

    pub(crate) fn replace_project(
        &mut self,
        project_id: ProjectId,
        document: TimelineDocument,
        scenes: HashMap<SceneId, SceneDefinition>,
        resolution: ProjectResolution,
        playhead: Frame,
    ) {
        self.replace_document(document, resolution, playhead);
        self.project_mut().id = project_id;
        self.next_scene_id = scenes
            .keys()
            .map(|id| id.get())
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .filter(|id| *id != u64::MAX);
        self.project_mut().scenes = scenes;
        self.next_effect_id =
            Self::next_effect_id(&self.project().document, self.project().scenes.values());
    }

    pub(crate) fn render_revision(&self) -> u64 {
        self.render_revision
    }

    pub(crate) fn frame_rate(&self) -> FrameRate {
        self.project().document.frame_rate()
    }

    pub(crate) fn resolution(&self) -> ProjectResolution {
        self.project().resolution
    }

    pub(crate) fn update_project_settings(
        &mut self,
        resolution: ProjectResolution,
        frame_rate: FrameRate,
    ) -> Result<bool, ProjectSettingsError> {
        if self.project().resolution == resolution && self.frame_rate() == frame_rate {
            return Ok(false);
        }
        let document = self.project().document.retimed(frame_rate)?;
        let scenes = self
            .project()
            .scenes
            .iter()
            .map(|(id, scene)| {
                let mut scene = scene.clone();
                let document = scene.document().retimed(frame_rate)?;
                *scene.document_mut() = document;
                Ok((*id, scene))
            })
            .collect::<Result<HashMap<_, _>, ProjectSettingsError>>()?;
        let playhead = super::document::retime_frame(self.playhead, self.frame_rate(), frame_rate)?;
        let before = self.history_snapshot();
        {
            let project = self.project_mut();
            project.document = document;
            project.scenes = scenes;
            project.resolution = resolution;
        }
        self.playhead = playhead;
        self.playback_time = None;
        self.finish_project_edit(Some(before), None);
        Ok(true)
    }

    pub(super) fn active_document(&self) -> &TimelineDocument {
        self.scene_path
            .last()
            .and_then(|id| self.project().scenes.get(id))
            .map_or(&self.project().document, SceneDefinition::document)
    }

    pub(super) fn active_document_mut(&mut self) -> &mut TimelineDocument {
        let active = self.scene_path.last().copied();
        match active {
            Some(id) => self
                .project_mut()
                .scenes
                .get_mut(&id)
                .expect("active scene must remain available")
                .document_mut(),
            None => &mut self.project_mut().document,
        }
    }

    pub(crate) fn active_scene_id(&self) -> Option<SceneId> {
        self.scene_path.last().copied()
    }

    pub(crate) fn scenes(&self) -> impl Iterator<Item = &SceneDefinition> {
        let mut scenes = self.project().scenes.values().collect::<Vec<_>>();
        scenes.sort_by_key(|scene| scene.id.get());
        scenes.into_iter()
    }

    pub(crate) fn scene(&self, id: SceneId) -> Option<&SceneDefinition> {
        self.project().scenes.get(&id)
    }

    pub(crate) fn evaluated_scene_argument_values_at(
        &self,
        item: &TimelineItem,
        time: TimelineTime,
    ) -> Option<HashMap<String, PropertyValue>> {
        let scene = self.scene(item.scene_id()?)?;
        Some(evaluated_scene_argument_values(scene, item, time))
    }

    pub(crate) fn playhead(&self) -> Frame {
        self.playhead
    }

    pub(crate) fn playback_time_seconds(&self) -> Option<f64> {
        self.playback_time
            .map(|time| time.seconds(self.frame_rate()))
    }

    pub(crate) fn playhead_seconds(&self) -> f64 {
        self.frame_rate().frame_to_seconds(self.playhead)
    }

    pub(crate) fn selected_item_ids(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.selection.current.iter().copied()
    }

    pub(crate) fn is_item_selected(&self, id: ItemId) -> bool {
        self.selection.current.contains(&id)
    }

    pub(crate) fn selection_remembers_item(&self, id: ItemId) -> bool {
        self.selection.remembered.contains(&id)
    }

    pub(crate) fn selected_items(&self) -> Vec<TimelineItem> {
        let mut ids = self.selection.sorted_current();
        if let Some(primary) = self.selection.primary
            && let Some(index) = ids.iter().position(|id| *id == primary)
        {
            ids.swap(0, index);
        }
        ids.into_iter()
            .filter_map(|id| self.active_document().item(id))
            .map(|item| self.materialized_item(item))
            .collect()
    }

    pub(super) fn materialized_item(&self, item: &TimelineItem) -> TimelineItem {
        let mut item = item.clone();
        if let Some(properties) =
            materialize_scene_instance_properties(&item, &self.project().scenes)
        {
            item.properties = properties;
        }
        item
    }

    pub(crate) fn selected_item(&self) -> Option<TimelineItem> {
        self.selection
            .primary
            .and_then(|id| self.active_document().item(id))
            .map(|item| self.materialized_item(item))
    }

    pub(crate) fn active_edit_effect(&self) -> Option<EffectInstanceId> {
        let (item_id, effect_id) = self.active_edit_target?;
        if self.selection.current.len() != 1 || self.selection.primary != Some(item_id) {
            return None;
        }
        self.active_document()
            .item(item_id)?
            .effects
            .iter()
            .any(|effect| effect.id == effect_id)
            .then_some(effect_id)
    }

    pub(crate) fn set_active_edit_effect(&mut self, effect_id: Option<EffectInstanceId>) -> bool {
        let next = effect_id.and_then(|effect_id| {
            let item_id = self.selection.primary?;
            if self.selection.current.len() != 1 {
                return None;
            }
            self.active_document()
                .item(item_id)?
                .effects
                .iter()
                .any(|effect| effect.id == effect_id)
                .then_some((item_id, effect_id))
        });
        if self.active_edit_target == next {
            return false;
        }
        self.active_edit_target = next;
        true
    }

    pub(crate) fn item(&self, id: ItemId) -> Option<&TimelineItem> {
        self.active_document().item(id)
    }

    pub(crate) fn item_label(&self, id: ItemId) -> Option<String> {
        let item = self.active_document().item(id)?;
        match item.scene_id() {
            Some(scene_id) => self
                .project()
                .scenes
                .get(&scene_id)
                .map(|scene| scene.name.clone()),
            None => item.intrinsic_label(),
        }
    }

    pub(crate) fn item_layer(&self, id: ItemId) -> Option<LayerId> {
        self.active_document().item_layer(id)
    }

    pub(crate) fn items_on_layer(&self, layer: LayerId) -> Vec<TimelineItem> {
        self.active_document().items_on_layer(layer)
    }

    pub(crate) fn active_items_at(&self, frame: Frame) -> Vec<(LayerId, TimelineItem)> {
        evaluated_visible_document_items_at_time(
            self.active_document(),
            &self.project().scenes,
            &self.visibility,
            TimelineTime::from_frame(frame),
        )
        .into_iter()
        .map(|(layer, mut item)| {
            self.visibility.retain_visible_effects(&mut item);
            (layer, item)
        })
        .collect()
    }

    pub(crate) fn active_items_at_time(&self, time: TimelineTime) -> Vec<(LayerId, TimelineItem)> {
        evaluated_visible_document_items_at_time(
            self.active_document(),
            &self.project().scenes,
            &self.visibility,
            time,
        )
        .into_iter()
        .map(|(layer, mut item)| {
            self.visibility.retain_visible_effects(&mut item);
            (layer, item)
        })
        .collect()
    }

    pub(crate) fn visible_items(&self) -> Vec<TimelineItem> {
        visibility_filtered_document_items(
            self.active_document(),
            &self.project().scenes,
            &self.visibility,
        )
        .into_iter()
        .map(|mut item| {
            self.visibility.retain_visible_effects(&mut item);
            item
        })
        .collect()
    }

    pub(crate) fn hidden_layer_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.visibility.hidden_layers()
    }

    pub(crate) fn hidden_item_ids(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.visibility.hidden_items()
    }

    pub(crate) fn selected_items_hidden_state(&self) -> Option<bool> {
        self.visibility
            .selected_items_hidden_state(&self.selection.current)
    }

    pub(crate) fn is_effect_hidden(&self, effect_id: EffectInstanceId) -> bool {
        self.visibility.is_effect_hidden(effect_id)
    }

    pub(crate) fn can_move_selected_effect(
        &self,
        primary_effect_id: EffectInstanceId,
        offset: i32,
    ) -> bool {
        let Some(primary_item_id) = self.selection.primary else {
            return false;
        };
        let Some(primary_item) = self.active_document().item(primary_item_id) else {
            return false;
        };
        let Some(source_index) = primary_item
            .effects
            .iter()
            .position(|effect| effect.id == primary_effect_id)
        else {
            return false;
        };
        let Some(target_index) = source_index.checked_add_signed(offset as isize) else {
            return false;
        };
        self.selected_effect_instances(primary_effect_id)
            .is_some_and(|effects| {
                !effects.is_empty()
                    && effects.iter().all(|(item_id, effect_id)| {
                        self.active_document().item(*item_id).is_some_and(|item| {
                            target_index < item.effects.len()
                                && item.effects.get(source_index).map(|effect| effect.id)
                                    == Some(*effect_id)
                        })
                    })
            })
    }

    pub(crate) fn item_time_ranges(&self) -> impl Iterator<Item = (ItemId, Frame, Frame)> + '_ {
        self.active_document()
            .items()
            .map(|item| (item.id, item.start, item.end_exclusive()))
    }

    pub(crate) fn item_layouts(
        &self,
    ) -> impl Iterator<Item = (ItemId, LayerId, Frame, Frame)> + '_ {
        self.active_document().item_layouts()
    }

    pub(crate) fn end_frame_exclusive(&self) -> Frame {
        self.active_document()
            .items()
            .map(TimelineItem::end_exclusive)
            .max()
            .unwrap_or(Frame::new(0))
    }
}
