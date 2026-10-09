use std::{collections::HashMap, sync::Arc};

use crate::{
    plugin::PluginRegistry,
    property::{PropertySchema, PropertyValue},
};

use super::{
    document::TimelineDocument,
    evaluation::{evaluated_items_at_time, visible_items},
    history::{EditHistory, HistorySnapshot, ScopedHistoryKey},
    ids::{EffectInstanceId, ItemId, LayerId, ProjectId, SceneId},
    item::TimelineItem,
    project::TimelineProject,
    properties::{SceneArguments, resolve_item, resolve_property},
    property_address::{property_schemas, resolve_property_schema},
    scene::SceneDefinition,
    selection::{EditScope, SelectionState},
    settings::{BeatGuide, ProjectResolution, ProjectSettingsError},
    time::{Frame, FrameRate, TimelineTime},
    view::TimelineSnapshot,
    visibility::PreviewVisibility,
};

const HISTORY_LIMIT: usize = 100;

/// Coordinates timeline documents, editor session state, history, and commands.
///
/// Persistent item/layer invariants belong to `TimelineDocument`; this type owns
/// cross-document concerns such as scene navigation, revisions, and undo/redo.
pub struct TimelineEditor {
    pub(super) plugins: Arc<PluginRegistry>,
    project: Arc<TimelineProject>,
    media_cache: Arc<crate::media::MediaMetadataCache>,
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

    pub fn new(frame_rate: FrameRate, plugins: Arc<PluginRegistry>) -> Self {
        Self::from_document(
            TimelineDocument::new(frame_rate),
            ProjectResolution::DEFAULT,
            plugins,
        )
    }

    pub fn from_document(
        document: TimelineDocument,
        resolution: ProjectResolution,
        plugins: Arc<PluginRegistry>,
    ) -> Self {
        let next_effect_id = Self::next_effect_id(&document, std::iter::empty());
        Self {
            plugins,
            project: Arc::new(TimelineProject {
                id: ProjectId::generate(),
                document,
                scenes: HashMap::new(),
                resolution,
                beat_guide: BeatGuide::default(),
            }),
            media_cache: Arc::default(),
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

    pub(super) fn advance_project_revision(&mut self) {
        self.project_revision = self.next_project_revision;
        self.next_project_revision = self.next_project_revision.saturating_add(1);
        self.advance_render_revision();
    }

    pub fn project_revision(&self) -> u64 {
        self.project_revision
    }

    pub fn plugin_registry(&self) -> &PluginRegistry {
        &self.plugins
    }

    pub fn plugin_registry_arc(&self) -> Arc<PluginRegistry> {
        self.plugins.clone()
    }

    pub fn snapshot(&self) -> TimelineSnapshot {
        TimelineSnapshot {
            project: self.project.clone(),
            media_cache: self.media_cache.clone(),
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

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn replace_document(
        &mut self,
        document: TimelineDocument,
        resolution: ProjectResolution,
        playhead: Frame,
    ) {
        self.replace_project(
            ProjectId::generate(),
            document,
            HashMap::new(),
            resolution,
            BeatGuide::default(),
            playhead,
        );
    }

    pub fn replace_project(
        &mut self,
        project_id: ProjectId,
        document: TimelineDocument,
        scenes: HashMap<SceneId, SceneDefinition>,
        resolution: ProjectResolution,
        beat_guide: BeatGuide,
        playhead: Frame,
    ) {
        self.next_scene_id = scenes
            .keys()
            .map(|id| id.get())
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .filter(|id| *id != u64::MAX);
        self.next_effect_id = Self::next_effect_id(&document, scenes.values());
        self.project = Arc::new(TimelineProject {
            id: project_id,
            document,
            scenes,
            resolution,
            beat_guide,
        });
        self.media_cache = Arc::default();
        self.scene_path.clear();
        self.playhead = playhead;
        self.playback_time = None;
        self.selection.clear();
        self.active_edit_target = None;
        self.visibility.clear();
        self.history.clear();
        // Reopening the same project must invalidate outstanding edit targets.
        self.advance_project_revision();
    }

    pub fn media_cache(&self) -> &Arc<crate::media::MediaMetadataCache> {
        &self.media_cache
    }

    pub fn cache_media_file(&mut self, file: &crate::media::ProbedFile) -> bool {
        let changed = Arc::make_mut(&mut self.media_cache).record(file);
        if changed {
            self.advance_render_revision();
        }
        changed
    }

    pub fn import_media_cache(&mut self, cache: &crate::media::MediaMetadataCache) {
        let mut changed = false;
        for file in cache.files() {
            changed |= Arc::make_mut(&mut self.media_cache).record_missing(file);
        }
        if changed {
            self.advance_render_revision();
        }
    }

    pub(crate) fn replace_media_cache(&mut self, cache: crate::media::MediaMetadataCache) {
        self.media_cache = Arc::new(cache);
        self.advance_render_revision();
    }

    pub fn render_revision(&self) -> u64 {
        self.render_revision
    }

    pub fn frame_rate(&self) -> FrameRate {
        self.project().document.frame_rate()
    }

    pub fn resolution(&self) -> ProjectResolution {
        self.project().resolution
    }

    pub fn beat_guide(&self) -> BeatGuide {
        self.project().beat_guide
    }

    pub fn update_project_settings(
        &mut self,
        resolution: ProjectResolution,
        frame_rate: FrameRate,
        beat_guide: BeatGuide,
    ) -> Result<bool, ProjectSettingsError> {
        if self.resolution() == resolution
            && self.frame_rate() == frame_rate
            && self.beat_guide() == beat_guide
        {
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
        Ok(self.edit_project_if_changed(None, |editor| {
            {
                let project = editor.project_mut();
                project.document = document;
                project.scenes = scenes;
                project.resolution = resolution;
                project.beat_guide = beat_guide;
            }
            editor.playhead = playhead;
            editor.playback_time = None;
            true
        }))
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

    pub fn active_scene_id(&self) -> Option<SceneId> {
        self.scene_path.last().copied()
    }

    pub fn scenes(&self) -> impl Iterator<Item = &SceneDefinition> {
        let mut scenes = self.project().scenes.values().collect::<Vec<_>>();
        scenes.sort_by_key(|scene| scene.id.get());
        scenes.into_iter()
    }

    pub fn scene(&self, id: SceneId) -> Option<&SceneDefinition> {
        self.project().scenes.get(&id)
    }

    pub fn property_schemas<'a>(
        &'a self,
        item: &'a TimelineItem,
        effect_id: Option<EffectInstanceId>,
    ) -> impl Iterator<Item = &'a PropertySchema> {
        property_schemas(&self.project().scenes, item, effect_id)
    }

    pub fn property_schema(
        &self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
    ) -> Option<&PropertySchema> {
        resolve_property_schema(
            &self.project().scenes,
            self.item(item_id)?,
            effect_id,
            property_id,
        )
    }

    /// Resolve defaults, arguments, and animations in the current editing document.
    pub fn evaluated_item_at(&self, item: &TimelineItem, time: TimelineTime) -> TimelineItem {
        resolve_item(
            &self.project().scenes,
            self.active_scene_arguments(),
            item,
            Some(time),
        )
    }

    /// Authored value after resolving defaults and the active scene's arguments.
    pub fn property_value(&self, address: &super::PropertyAddress) -> Option<PropertyValue> {
        self.resolve_property_value(address, None)
    }

    pub fn evaluated_property_value(
        &self,
        address: &super::PropertyAddress,
        time: TimelineTime,
    ) -> Option<PropertyValue> {
        self.resolve_property_value(address, Some(time))
    }

    /// Match an owner and array position using authored values, including inherited defaults.
    pub fn corresponding_property_address(
        &self,
        address: &super::PropertyAddress,
        item_id: ItemId,
    ) -> Option<super::PropertyAddress> {
        let scenes = &self.project().scenes;
        let arguments = self.active_scene_arguments();
        let source = resolve_item(scenes, arguments, self.item(address.item_id)?, None);
        let item = resolve_item(scenes, arguments, self.item(item_id)?, None);
        address.on_item(&source, &item)
    }

    fn resolve_property_value(
        &self,
        address: &super::PropertyAddress,
        time: Option<TimelineTime>,
    ) -> Option<PropertyValue> {
        let item = self.item(address.item_id)?;
        let property = address.schema(self)?;
        let value = resolve_property(
            self.active_scene_arguments(),
            item,
            address.effect_id,
            property,
            time,
        )?;
        if address.element_id.is_none() && address.scalar_index.is_none() {
            return Some(value);
        }
        let element = value.element(address.element_id)?;
        match address.scalar_index {
            Some(index) => element.scalar_at(Some(index)),
            None => Some(element),
        }
        .cloned()
    }

    fn active_scene_arguments(&self) -> Option<SceneArguments<'_>> {
        let scene = self.scene(self.active_scene_id()?)?;
        Some(SceneArguments::new(&self.project().scenes, scene, None))
    }

    pub fn playhead(&self) -> Frame {
        self.playhead
    }

    pub fn playback_time_seconds(&self) -> Option<f64> {
        self.playback_time
            .map(|time| time.seconds(self.frame_rate()))
    }

    pub fn playhead_seconds(&self) -> f64 {
        self.frame_rate().frame_to_seconds(self.playhead)
    }

    pub fn selected_item_ids(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.selection.current.iter().copied()
    }

    pub fn is_item_selected(&self, id: ItemId) -> bool {
        self.selection.current.contains(&id)
    }

    pub fn selection_remembers_item(&self, id: ItemId) -> bool {
        self.selection.remembered.contains(&id)
    }

    pub fn selected_items(&self) -> Vec<TimelineItem> {
        self.items_in_scope(EditScope::Selection)
    }

    /// Items with defaults and scene arguments, before animation evaluation.
    pub fn items_in_scope(&self, scope: EditScope) -> Vec<TimelineItem> {
        self.resolve_items_in_scope(scope, None)
    }

    /// Stored items, without defaults, scene arguments or animation evaluation.
    pub fn source_items_in_scope(&self, scope: EditScope) -> impl Iterator<Item = &TimelineItem> {
        scope
            .item_ids(self)
            .into_iter()
            .filter_map(|id| self.item(id))
    }

    pub fn evaluated_items_in_scope(
        &self,
        scope: EditScope,
        time: TimelineTime,
    ) -> Vec<TimelineItem> {
        self.resolve_items_in_scope(scope, Some(time))
    }

    fn resolve_items_in_scope(
        &self,
        scope: EditScope,
        time: Option<TimelineTime>,
    ) -> Vec<TimelineItem> {
        self.source_items_in_scope(scope)
            .map(|item| {
                resolve_item(
                    &self.project().scenes,
                    self.active_scene_arguments(),
                    item,
                    time,
                )
            })
            .collect()
    }

    pub(crate) fn resolve_active_scene_arguments<'a>(
        &self,
        items: impl IntoIterator<Item = &'a mut TimelineItem>,
    ) {
        if let Some(scene) = self.active_scene_id().and_then(|id| self.scene(id)) {
            scene.apply_arguments(&self.project().scenes, None, items);
        }
    }

    /// A selection supplies a placement layer only when all its items agree.
    pub fn selected_items_layer(&self) -> Option<LayerId> {
        let mut layers = self
            .selected_item_ids()
            .filter_map(|id| self.item_layer(id));
        let layer = layers.next()?;
        layers.all(|candidate| candidate == layer).then_some(layer)
    }

    pub fn active_edit_effect(&self) -> Option<EffectInstanceId> {
        let (item_id, effect_id) = self.active_edit_target?;
        if self.selection.current.len() != 1 || !self.selection.current.contains(&item_id) {
            return None;
        }
        self.item(item_id)?.effect(effect_id)?;
        Some(effect_id)
    }

    pub fn set_active_edit_effect(&mut self, effect_id: Option<EffectInstanceId>) -> bool {
        let next = effect_id.and_then(|effect_id| {
            if self.selection.current.len() != 1 {
                return None;
            }
            let item_id = *self.selection.current.iter().next()?;
            self.item(item_id)?.effect(effect_id)?;
            Some((item_id, effect_id))
        });
        if self.active_edit_target == next {
            return false;
        }
        self.active_edit_target = next;
        true
    }

    pub fn item(&self, id: ItemId) -> Option<&TimelineItem> {
        self.active_document().item(id)
    }

    pub fn item_label(&self, id: ItemId) -> Option<String> {
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

    pub fn item_layer(&self, id: ItemId) -> Option<LayerId> {
        self.active_document().item_layer(id)
    }

    pub fn items_on_layer(&self, layer: LayerId) -> Vec<TimelineItem> {
        self.active_document().items_on_layer(layer)
    }

    pub fn active_items_at(&self, frame: Frame) -> Vec<(LayerId, TimelineItem)> {
        self.active_items_at_time(TimelineTime::from_frame(frame))
    }

    pub(super) fn active_source_items(
        &self,
        time: Option<TimelineTime>,
    ) -> Vec<(LayerId, TimelineItem)> {
        let mut items = time.map_or_else(
            || self.active_document().source_items(),
            |time| self.active_document().active_source_items_at_time(time),
        );
        self.resolve_active_scene_arguments(items.iter_mut().map(|(_, item)| item));
        items
    }

    pub fn active_items_at_time(&self, time: TimelineTime) -> Vec<(LayerId, TimelineItem)> {
        evaluated_items_at_time(
            self.active_source_items(Some(time)),
            &self.project().scenes,
            time,
            Some(&self.visibility),
        )
    }

    pub fn visible_items(&self) -> Vec<TimelineItem> {
        visible_items(
            self.active_source_items(None),
            &self.project().scenes,
            Some(&self.visibility),
        )
        .into_iter()
        .map(|mut item| {
            self.visibility.retain_visible_effects(&mut item);
            item
        })
        .collect()
    }

    pub fn hidden_layer_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.visibility.hidden_layers()
    }

    pub fn hidden_item_ids(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.visibility.hidden_items()
    }

    pub fn selected_items_hidden_state(&self) -> Option<bool> {
        self.items_hidden_state(EditScope::Selection)
    }

    pub fn items_hidden_state(&self, scope: EditScope) -> Option<bool> {
        self.visibility.items_hidden_state(scope.item_ids(self))
    }

    pub fn is_effect_hidden(&self, effect_id: EffectInstanceId) -> bool {
        self.visibility.is_effect_hidden(effect_id)
    }

    pub fn can_move_effect(
        &self,
        scope: EditScope,
        effect_id: EffectInstanceId,
        offset: i32,
    ) -> bool {
        self.effect_move_target(scope, effect_id, offset).is_some()
    }

    pub fn item_time_ranges(&self) -> impl Iterator<Item = (ItemId, Frame, Frame)> + '_ {
        self.active_document()
            .items()
            .map(|item| (item.id, item.start, item.end_exclusive()))
    }

    pub fn item_layouts(&self) -> impl Iterator<Item = (ItemId, LayerId, Frame, Frame)> + '_ {
        self.active_document().item_layouts()
    }

    pub fn end_frame_exclusive(&self) -> Frame {
        self.active_document()
            .items()
            .map(TimelineItem::end_exclusive)
            .max()
            .unwrap_or(Frame::new(0))
    }
}
