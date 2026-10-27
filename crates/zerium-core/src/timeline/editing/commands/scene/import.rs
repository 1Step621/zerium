//! Add independent copies of a checked scene bundle in one undoable edit.
use std::collections::{HashMap, HashSet};

use crate::timeline::{
    EffectInstanceId, SceneBindingOwner, SceneBindingTarget, SceneDefinition, SceneId,
    TimelineDocument, TimelineEditError, TimelineEditor, TimelineItemKind,
    editing::history::EditImpact,
};

impl TimelineEditor {
    pub(crate) fn import_scenes(
        &mut self,
        root: SceneId,
        mut scenes: Vec<SceneDefinition>,
    ) -> Result<SceneId, TimelineEditError> {
        // Stable order makes generated names and IDs independent of map iteration order.
        scenes.sort_by_key(|scene| scene.id.get());
        self.try_edit_project(EditImpact::MediaDependencies, None, |editor| {
            let mut next_scene = editor.next_scene_id;
            let mut next_effect = editor.next_effect_id;
            let mut scene_ids = HashMap::new();
            for scene in &scenes {
                let id = next_scene.ok_or(TimelineEditError::IdentifierExhausted)?;
                scene_ids.insert(scene.id, SceneId::new(editor.project().id, id));
                next_scene = id.checked_add(1).filter(|id| *id != u64::MAX);
            }
            let root = *scene_ids
                .get(&root)
                .ok_or(TimelineEditError::SceneNotFound(root))?;
            let mut names = editor
                .project()
                .scenes
                .values()
                .map(|scene| scene.name.clone())
                .collect::<HashSet<_>>();
            let mut imported = Vec::with_capacity(scenes.len());
            for source in scenes {
                let source = source
                    .retimed(editor.frame_rate())
                    .map_err(TimelineEditError::InvalidTiming)?;
                let mut items = source.document().source_items();
                let mut effect_ids = HashMap::new();
                for (_, item) in &mut items {
                    if let TimelineItemKind::Scene { scene_id } = &mut item.kind {
                        *scene_id = *scene_ids
                            .get(scene_id)
                            .ok_or(TimelineEditError::SceneNotFound(*scene_id))?;
                    }
                    for effect in &mut item.effects {
                        let id = next_effect.ok_or(TimelineEditError::IdentifierExhausted)?;
                        let new_id = EffectInstanceId::new(id);
                        effect_ids.insert(effect.id, new_id);
                        effect.id = new_id;
                        next_effect = id.checked_add(1).filter(|id| *id != u64::MAX);
                    }
                }
                let mut variables = source.variables().to_vec();
                for variable in &mut variables {
                    for binding in &mut variable.bindings {
                        if let SceneBindingOwner::Effect(id) = binding.owner() {
                            *binding = SceneBindingTarget::new(
                                binding.item_id(),
                                SceneBindingOwner::Effect(effect_ids[&id]),
                                binding.property_id(),
                                binding.element_id(),
                                binding.scalar_index(),
                            );
                        }
                    }
                }
                let mut name = source.name.clone();
                for suffix in 2_u64.. {
                    if names.insert(name.clone()) {
                        break;
                    }
                    name = format!("{} {suffix}", source.name);
                }
                let mut scene = SceneDefinition::from_project(
                    scene_ids[&source.id],
                    name,
                    variables,
                    TimelineDocument::from_items(
                        editor.frame_rate(),
                        items,
                        source.hidden_layer_ids(),
                    ),
                );
                scene.set_stretch_point(source.stretch_point());
                imported.push((scene.id, scene));
            }
            editor.project_mut().scenes_mut().extend(imported);
            editor.next_scene_id = next_scene;
            editor.next_effect_id = next_effect;
            Ok((root, true))
        })
    }
}
