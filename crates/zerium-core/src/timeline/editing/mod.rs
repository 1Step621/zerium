//! Timeline editing, session state, and undoable commands.
use self::{
    history::{EditHistory, EditImpact, HistoryKey, HistorySnapshot, ScopedHistoryKey},
    selection::SelectionState,
};
use super::{
    EvaluatedItem, RenderEvaluation, TimelineView,
    evaluation::scene::{
        evaluated_items_at_time,
        properties::{SceneBindings, resolve_property},
    },
    model::{
        EffectInstanceId, ItemId, LayerId, ProjectId, SceneId,
        document::TimelineDocument,
        item::{
            TimelineItem,
            property::{property_schemas, resolve_property_schema},
        },
        project::{
            BeatGuide, ProjectResolution, ProjectSettingsError, TimelineProject, TimelineSnapshot,
        },
        scene::SceneDefinition,
        time::{Frame, FrameRate, TimelineTime},
    },
};
use crate::{
    plugin::PluginRegistry,
    property::{PropertySchema, PropertyValue},
};
use std::{collections::HashMap, sync::Arc};

pub(super) mod commands;
mod gesture;
pub(super) mod history;
mod selection;
mod session;

pub use self::{
    commands::{
        AnimationEdit, AnimationEditTarget, AnimationValuePlan, ItemMoveEdit,
        SceneVariableEditError, ValueEdit, ValueEditTarget,
    },
    gesture::{EditContext, EditSession},
};

const HISTORY_LIMIT: usize = 100;

/// A stopped cursor is frame-exact; playback keeps its continuous sample time.
/// The displayed frame is derived so these positions cannot drift apart.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PlayheadPosition {
    Stopped(Frame),
    Playing(TimelineTime),
}

impl PlayheadPosition {
    fn frame(self) -> Frame {
        match self {
            Self::Stopped(frame) => frame,
            Self::Playing(time) => time.nearest_frame(),
        }
    }

    fn playback_time(self) -> Option<TimelineTime> {
        match self {
            Self::Stopped(_) => None,
            Self::Playing(time) => Some(time),
        }
    }
}

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
    pub(super) position: PlayheadPosition,
    pub(super) selection: SelectionState,
    /// None edits the selected item's source; Some edits an effect on that item.
    pub(super) active_edit_target: Option<(ItemId, EffectInstanceId)>,
    render_revision: u64,
    project_revision: u64,
    /// Monotonic edit lifetime; unlike project_revision, Undo never restores it.
    edit_revision: u64,
    next_media_dependencies_revision: u64,
    pub(super) history: EditHistory<HistorySnapshot, ScopedHistoryKey>,
    /// Override individual command keys while applying a gesture update.
    pub(super) history_group: Option<HistoryKey>,
}

/// A prepared edit owns the pre-publication state until it is committed.
/// Failed and unchanged commands restore IDs and session side effects as well as data.
pub(super) struct ProjectEditState {
    project: Arc<TimelineProject>,
    media_cache: Arc<crate::media::MediaMetadataCache>,
    scene_path: Vec<SceneId>,
    next_scene_id: Option<u64>,
    next_effect_id: Option<u64>,
    position: PlayheadPosition,
    selection: SelectionState,
    active_edit_target: Option<(ItemId, EffectInstanceId)>,
    render_revision: u64,
}

impl TimelineEditor {
    pub(super) fn prepare_project_edit(&self) -> ProjectEditState {
        ProjectEditState {
            project: self.project.clone(),
            media_cache: self.media_cache.clone(),
            scene_path: self.scene_path.clone(),
            next_scene_id: self.next_scene_id,
            next_effect_id: self.next_effect_id,
            position: self.position,
            selection: self.selection.clone(),
            active_edit_target: self.active_edit_target,
            render_revision: self.render_revision,
        }
    }

    pub(super) fn discard_project_edit(&mut self, state: ProjectEditState) {
        self.project = state.project;
        self.media_cache = state.media_cache;
        self.scene_path = state.scene_path;
        self.next_scene_id = state.next_scene_id;
        self.next_effect_id = state.next_effect_id;
        self.position = state.position;
        self.selection = state.selection;
        self.active_edit_target = state.active_edit_target;
        self.render_revision = state.render_revision;
    }

    pub(super) fn project(&self) -> &TimelineProject {
        &self.project
    }

    pub(super) fn project_mut(&mut self) -> &mut TimelineProject {
        Arc::make_mut(&mut self.project)
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
        let project_id = ProjectId::generate();
        let main_scene = SceneId::new(project_id, 1);
        let scenes = HashMap::from([(
            main_scene,
            SceneDefinition::from_project(main_scene, "Main".to_owned(), Vec::new(), document),
        )]);
        let next_effect_id = Self::next_effect_id(scenes.values());
        Self {
            plugins,
            project: Arc::new(TimelineProject::new(
                project_id,
                main_scene,
                scenes,
                resolution,
                BeatGuide::default(),
                Default::default(),
            )),
            media_cache: Arc::default(),
            scene_path: Vec::new(),
            next_scene_id: Some(2),
            next_effect_id,
            position: PlayheadPosition::Stopped(Frame::new(0)),
            selection: SelectionState::default(),
            active_edit_target: None,
            render_revision: 0,
            project_revision: 0,
            edit_revision: 0,
            next_media_dependencies_revision: 1,
            history: EditHistory::new(HISTORY_LIMIT),
            history_group: None,
        }
    }

    pub(super) fn advance_render_revision(&mut self) {
        self.render_revision = self.render_revision.saturating_add(1);
    }

    fn next_effect_id<'a>(scenes: impl Iterator<Item = &'a SceneDefinition>) -> Option<u64> {
        scenes
            .flat_map(SceneDefinition::items)
            .flat_map(|item| &item.effects)
            .map(|effect| effect.id.get())
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .filter(|id| *id != u64::MAX)
    }

    pub(super) fn advance_project_revision(&mut self) {
        self.advance_edit_revision();
        self.project_revision = self.edit_revision;
        self.advance_render_revision();
    }

    pub(super) fn advance_edit_revision(&mut self) {
        self.edit_revision = self
            .edit_revision
            .checked_add(1)
            .expect("edit revision exhausted");
    }

    pub(super) fn edit_revision(&self) -> u64 {
        self.edit_revision
    }

    pub(super) fn advance_media_dependencies_revision(&mut self) {
        let revision = self.next_media_dependencies_revision;
        self.project_mut().media_dependencies_revision = revision;
        self.next_media_dependencies_revision = revision.saturating_add(1);
    }

    pub fn media_dependencies_revision(&self) -> u64 {
        self.project().media_dependencies_revision
    }

    pub fn project_id(&self) -> ProjectId {
        self.project().id
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

    /// Capture the whole project for persistence and Main-scene export.
    pub fn snapshot(&self) -> TimelineSnapshot {
        TimelineSnapshot {
            scene_id: self.project.main_scene,
            ..self.preview_snapshot()
        }
    }

    /// Capture the currently edited scene.
    pub fn preview_snapshot(&self) -> TimelineSnapshot {
        TimelineSnapshot {
            project: self.project.clone(),
            media_cache: self.media_cache.clone(),
            playhead: self.playhead(),
            project_revision: self.project_revision,
            scene_id: self.active_scene_id(),
        }
    }

    /// Capture the currently edited scene for audio.
    pub fn audio_snapshot(&self) -> crate::timeline::AudioTimelineSnapshot {
        self.preview_snapshot().audio_snapshot()
    }

    pub(super) fn history_snapshot(&self) -> HistorySnapshot {
        HistorySnapshot {
            project: self.project.clone(),
            scene_path: self.scene_path.clone(),
            playhead: self.playhead(),
            selection: self.selection.clone(),
            project_revision: self.project_revision,
        }
    }

    pub(super) fn restore_history_snapshot(&mut self, snapshot: HistorySnapshot) {
        let location_changed = self.scene_path != snapshot.scene_path;
        let frame_rate_changed =
            self.frame_rate() != snapshot.project.main_scene().document().frame_rate();
        self.project = snapshot.project;
        self.scene_path = snapshot
            .scene_path
            .into_iter()
            .take_while(|id| self.project().scenes.contains_key(id))
            .collect();
        let playhead = if location_changed || frame_rate_changed {
            snapshot.playhead
        } else {
            self.playhead()
        };
        if location_changed {
            self.active_edit_target = None;
        }
        self.selection = snapshot.selection;
        self.project_revision = snapshot.project_revision;
        self.position = PlayheadPosition::Stopped(playhead);
        self.advance_edit_revision();
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
        let project_id = ProjectId::generate();
        let main_scene = SceneId::new(project_id, 1);
        self.replace_project(
            TimelineProject::new(
                project_id,
                main_scene,
                HashMap::from([(
                    main_scene,
                    SceneDefinition::from_project(
                        main_scene,
                        "Main".to_owned(),
                        Vec::new(),
                        document,
                    ),
                )]),
                resolution,
                BeatGuide::default(),
                Default::default(),
            ),
            playhead,
        );
    }

    pub(crate) fn replace_project(&mut self, project: TimelineProject, playhead: Frame) {
        self.next_scene_id = project
            .scenes
            .keys()
            .map(|id| id.get())
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .filter(|id| *id != u64::MAX);
        self.next_effect_id = Self::next_effect_id(project.scenes.values());
        self.project = Arc::new(project);
        self.media_cache = Arc::default();
        self.scene_path.clear();
        self.reset_scene_session();
        self.position = PlayheadPosition::Stopped(playhead);
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

    pub fn invalidate_media(&mut self, source: &crate::media::MediaDependency) -> bool {
        let changed = Arc::make_mut(&mut self.media_cache).invalidate(source);
        if changed {
            self.advance_render_revision();
        }
        changed
    }

    pub fn render_revision(&self) -> u64 {
        self.render_revision
    }

    pub fn frame_rate(&self) -> FrameRate {
        self.project().main_scene().document().frame_rate()
    }

    pub fn resolution(&self) -> ProjectResolution {
        self.project().resolution
    }

    pub fn beat_guide(&self) -> BeatGuide {
        self.project().beat_guide
    }

    pub fn export_settings(&self) -> &crate::timeline::ExportSettings {
        &self.project.export_settings
    }

    pub fn update_export_destination(
        &mut self,
        destination: std::path::PathBuf,
    ) -> Result<bool, ProjectSettingsError> {
        let mut settings = self.export_settings().clone();
        settings.destination = Some(destination);
        settings.validate()?;
        if self.export_settings() == &settings {
            return Ok(false);
        }
        Ok(
            self.edit_project_if_changed(EditImpact::Values, None, |editor| {
                editor.project_mut().export_settings = settings;
                true
            }),
        )
    }

    pub fn update_project_settings(
        &mut self,
        resolution: ProjectResolution,
        frame_rate: FrameRate,
        beat_guide: BeatGuide,
        export_settings: crate::timeline::ExportSettings,
    ) -> Result<bool, ProjectSettingsError> {
        export_settings.validate()?;
        if self.resolution() == resolution
            && self.frame_rate() == frame_rate
            && self.beat_guide() == beat_guide
            && self.export_settings() == &export_settings
        {
            return Ok(false);
        }
        let mut project = self.project().retimed(frame_rate)?;
        project.resolution = resolution;
        project.beat_guide = beat_guide;
        project.export_settings = export_settings;
        let playhead = crate::timeline::model::document::retime_frame(
            self.playhead(),
            self.frame_rate(),
            frame_rate,
        )?;
        Ok(
            self.edit_project_if_changed(EditImpact::Values, None, |editor| {
                editor.project = Arc::new(project);
                editor.position = PlayheadPosition::Stopped(playhead);
                true
            }),
        )
    }

    pub(super) fn active_document(&self) -> &TimelineDocument {
        self.active_scene().document()
    }

    pub(super) fn active_document_mut(&mut self) -> &mut TimelineDocument {
        self.active_scene_mut().document_mut()
    }

    pub fn main_scene_id(&self) -> SceneId {
        self.project().main_scene
    }

    pub fn active_scene_id(&self) -> SceneId {
        self.scene_path
            .last()
            .copied()
            .unwrap_or(self.main_scene_id())
    }

    pub fn active_scene(&self) -> &SceneDefinition {
        &self.project().scenes[&self.active_scene_id()]
    }

    pub(super) fn active_scene_mut(&mut self) -> &mut SceneDefinition {
        let id = self.active_scene_id();
        self.project_mut()
            .scenes_mut()
            .get_mut(&id)
            .expect("active scene exists")
    }

    pub fn scenes(&self) -> impl Iterator<Item = &SceneDefinition> {
        let mut scenes = self.project().scenes.values().collect::<Vec<_>>();
        scenes.sort_by_key(|scene| (scene.id != self.main_scene_id(), scene.id.get()));
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

    /// Property metadata for sampled UI values, independent of authored value storage.
    pub fn evaluated_property_schemas<'a>(
        &'a self,
        item: &'a crate::timeline::EvaluatedItem,
        effect_id: Option<EffectInstanceId>,
    ) -> impl Iterator<Item = &'a PropertySchema> {
        let properties = match effect_id {
            Some(id) => item
                .effects
                .iter()
                .find(|effect| effect.id == id)
                .map(|effect| effect.schema().properties()),
            None => item.schema().map(|schema| schema.properties()),
        };
        let scene = effect_id.is_none().then(|| item.scene_id()).flatten();
        crate::timeline::model::item::property::owner_property_schemas(
            &self.project().scenes,
            properties,
            scene,
        )
    }

    pub fn property_schema(
        &self,
        address: &crate::timeline::PropertyAddress,
    ) -> Option<&PropertySchema> {
        resolve_property_schema(
            &self.project().scenes,
            self.item(address.item_id)?,
            address.effect_id,
            &address.property_id,
        )
    }

    /// Resolve defaults, arguments, and animations in the current editing document.
    pub fn evaluated_item_at(
        &self,
        item: &TimelineItem,
        time: TimelineTime,
    ) -> crate::timeline::EvaluatedItem {
        crate::timeline::evaluation::scene::properties::evaluate_item(
            &self.project().scenes,
            &self.active_scene_variables(),
            item,
            Some(time),
            self.frame_rate(),
        )
    }

    /// Defaults and scene bindings, without sampling authored animation.
    pub fn resolved_item(&self, item: &TimelineItem) -> crate::timeline::EvaluatedItem {
        crate::timeline::evaluation::scene::properties::evaluate_item(
            &self.project().scenes,
            &self.active_scene_variables(),
            item,
            None,
            self.frame_rate(),
        )
    }

    /// Authored value after resolving defaults and the active scene's arguments.
    pub fn property_value(
        &self,
        address: &crate::timeline::PropertyAddress,
    ) -> Option<PropertyValue> {
        self.resolve_property_value(address, None)
    }

    pub fn evaluated_property_value(
        &self,
        address: &crate::timeline::PropertyAddress,
        time: TimelineTime,
    ) -> Option<PropertyValue> {
        self.resolve_property_value(address, Some(time))
    }

    /// Match an owner and array position using authored values, including inherited defaults.
    pub fn corresponding_property_address(
        &self,
        address: &crate::timeline::PropertyAddress,
        item_id: ItemId,
    ) -> Option<crate::timeline::PropertyAddress> {
        let source = self.resolved_item(self.item(address.item_id)?);
        let item = self.resolved_item(self.item(item_id)?);
        address.on_item(&source, &item)
    }

    fn resolve_property_value(
        &self,
        address: &crate::timeline::PropertyAddress,
        time: Option<TimelineTime>,
    ) -> Option<PropertyValue> {
        let item = self.item(address.item_id)?;
        let property = self.property_schema(address)?;
        let value = resolve_property(
            &self.active_scene_variables(),
            item,
            address.effect_id,
            property,
            time,
        )?
        .value;
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

    fn active_scene_variables(&self) -> SceneBindings<'_> {
        SceneBindings::new(&self.project().scenes, self.active_scene(), None)
    }

    pub fn playhead(&self) -> Frame {
        self.position.frame()
    }

    pub fn playback_time_seconds(&self) -> Option<f64> {
        self.position
            .playback_time()
            .map(|time| time.seconds(self.frame_rate()))
    }

    pub fn playhead_seconds(&self) -> f64 {
        self.frame_rate().frame_to_seconds(self.playhead())
    }

    /// The user's explicit selection in stable item-ID order, independent of time.
    pub fn selected_item_ids(&self) -> impl Iterator<Item = ItemId> {
        self.selection.sorted().into_iter()
    }

    pub fn is_item_selected(&self, id: ItemId) -> bool {
        self.selection.contains(id)
    }

    /// Selected items present at the playhead, for operations on the current frame.
    pub fn active_selected_item_ids(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.selected_item_ids().filter(|id| {
            self.item(*id)
                .is_some_and(|item| item.contains(self.playhead(), self.frame_rate()))
        })
    }

    pub fn is_item_active_selected(&self, id: ItemId) -> bool {
        self.is_item_selected(id)
            && self
                .item(id)
                .is_some_and(|item| item.contains(self.playhead(), self.frame_rate()))
    }

    pub fn selected_items(&self) -> Vec<TimelineItem> {
        self.selected_item_ids()
            .filter_map(|id| self.item(id).cloned())
            .collect()
    }

    pub fn active_selected_items(&self) -> Vec<TimelineItem> {
        self.active_selected_item_ids()
            .filter_map(|id| self.item(id).cloned())
            .collect()
    }

    pub fn bake_active_scene_bindings<'a>(
        &self,
        items: impl IntoIterator<Item = &'a mut TimelineItem>,
    ) {
        self.active_scene()
            .apply_variables(&self.project().scenes, None, items);
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
        if self.selected_item_ids().count() != 1 || !self.is_item_selected(item_id) {
            return None;
        }
        self.item(item_id)?.effect(effect_id)?;
        Some(effect_id)
    }

    pub fn set_active_edit_effect(&mut self, effect_id: Option<EffectInstanceId>) -> bool {
        let next = effect_id.and_then(|effect_id| {
            let mut selected = self.selected_item_ids();
            let item_id = selected.next()?;
            if selected.next().is_some() {
                return None;
            }
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

    pub fn active_items_at(&self, frame: Frame) -> Vec<(LayerId, crate::timeline::EvaluatedItem)> {
        self.active_items_at_time(TimelineTime::from_frame(frame))
    }

    pub fn active_items_at_time(
        &self,
        time: TimelineTime,
    ) -> Vec<(LayerId, crate::timeline::EvaluatedItem)> {
        evaluated_items_at_time(self.active_scene(), &self.project().scenes, time)
    }

    pub fn hidden_layer_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.active_scene().hidden_layer_ids()
    }

    pub fn can_move_effect(
        &self,
        item_id: ItemId,
        effect_id: EffectInstanceId,
        offset: i32,
    ) -> bool {
        self.effect_move_target(item_id, effect_id, offset)
            .is_some()
    }

    pub fn item_time_ranges(&self) -> impl Iterator<Item = (ItemId, Frame, Frame)> + '_ {
        self.active_document()
            .items()
            .map(|item| (item.id, item.start(), item.end_exclusive(self.frame_rate())))
    }

    pub fn item_layouts(&self) -> impl Iterator<Item = (ItemId, LayerId, Frame, Frame)> + '_ {
        self.active_document().item_layouts()
    }

    pub fn end_frame_exclusive(&self) -> Frame {
        self.active_document().end_frame_exclusive()
    }
}

impl TimelineView for TimelineEditor {
    fn media_cache(&self) -> &crate::media::MediaMetadataCache {
        TimelineEditor::media_cache(self)
    }

    fn resolution(&self) -> ProjectResolution {
        TimelineEditor::resolution(self)
    }

    fn frame_rate(&self) -> FrameRate {
        TimelineEditor::frame_rate(self)
    }

    fn playhead(&self) -> Frame {
        TimelineEditor::playhead(self)
    }

    fn active_items_at_time(&self, time: TimelineTime) -> Vec<(LayerId, EvaluatedItem)> {
        TimelineEditor::active_items_at_time(self, time)
    }

    fn render_evaluation(&self) -> RenderEvaluation<'_> {
        RenderEvaluation::new(self.active_scene(), &self.project().scenes)
    }

    fn end_frame_exclusive(&self) -> Frame {
        TimelineEditor::end_frame_exclusive(self)
    }
}
