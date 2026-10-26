use gpui::App;
use rust_i18n::t;
use zerium_core::{
    plugin::TextureInput,
    property::{PropertyOverrides, PropertySchema, ScalarPropertyType},
    timeline::{ItemSource, TimelineEditor, TimelineItem},
};

use crate::app::defaults::{self, DefaultsTarget};

/// The selected source and string property, independent of labels and menu ordering.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Target {
    pub(super) source: ItemSource,
    pub(super) property: String,
}

impl Target {
    pub(super) fn matches(&self, item: &TimelineItem) -> bool {
        match &self.source {
            ItemSource::Plugin { plugin_id, item_id } => {
                item.plugin_id() == Some(plugin_id.as_str())
                    && item.item_id() == Some(item_id.as_str())
            }
            ItemSource::Scene(id) => item.scene_id() == Some(*id),
        }
    }

    pub(super) fn available(editor: &TimelineEditor) -> Vec<(Self, String)> {
        let mut plugins = editor
            .plugin_registry()
            .items()
            .filter(|(_, schema)| schema.render().is_some() && schema.playback().is_none())
            .collect::<Vec<_>>();
        plugins.sort_by_key(|(_, schema)| {
            !schema
                .inputs()
                .iter()
                .any(|input| matches!(input, TextureInput::Text { .. }))
        });
        let mut targets = plugins
            .into_iter()
            .flat_map(|(plugin_id, schema)| {
                Self::choices(
                    ItemSource::Plugin {
                        plugin_id: plugin_id.to_owned(),
                        item_id: schema.id().to_owned(),
                    },
                    schema.label().to_owned(),
                    schema.properties(),
                )
            })
            .collect::<Vec<_>>();
        targets.extend(
            editor
                .scenes()
                .filter(|scene| editor.can_add_scene_instance(scene.id))
                .flat_map(|scene| {
                    let schemas = scene
                        .arguments()
                        .filter_map(|argument| argument.argument_schema().cloned())
                        .collect::<Vec<_>>();
                    Self::choices(
                        ItemSource::Scene(scene.id),
                        t!("text_placement.scene_target", name = scene.name).to_string(),
                        &schemas,
                    )
                }),
        );
        targets
    }

    fn choices(
        source: ItemSource,
        label: String,
        schemas: &[PropertySchema],
    ) -> Vec<(Self, String)> {
        let fields = schemas
            .iter()
            .filter(|schema| is_string(schema))
            .collect::<Vec<_>>();
        fields
            .iter()
            .map(|field| {
                let label = if fields.len() > 1 {
                    format!("{} / {}", label, field.label())
                } else {
                    label.clone()
                };
                (
                    Self {
                        source: source.clone(),
                        property: field.id().to_owned(),
                    },
                    label,
                )
            })
            .collect()
    }

    pub(super) fn schema(&self, editor: &TimelineEditor) -> Option<PropertySchema> {
        let schema = match &self.source {
            ItemSource::Plugin { plugin_id, item_id } => {
                let item = editor.plugin_registry().item(plugin_id, item_id)?;
                if item.render().is_none() || item.playback().is_some() {
                    return None;
                }
                item.property(&self.property)?.clone()
            }
            ItemSource::Scene(id) => {
                if !editor.can_add_scene_instance(*id) {
                    return None;
                }
                editor
                    .scene(*id)?
                    .arguments()
                    .filter_map(|argument| argument.argument_schema())
                    .find(|schema| schema.id() == self.property)?
                    .clone()
            }
        };
        is_string(&schema).then_some(schema)
    }

    pub(super) fn initial_properties(
        &self,
        editor: &TimelineEditor,
        cx: &App,
    ) -> Option<PropertyOverrides> {
        match &self.source {
            ItemSource::Plugin { plugin_id, item_id } => {
                let schema = editor.plugin_registry().item(plugin_id, item_id)?;
                Some(defaults::properties(
                    cx,
                    &DefaultsTarget::Item {
                        plugin_id: plugin_id.clone(),
                        item_id: item_id.clone(),
                    },
                    schema.properties(),
                ))
            }
            ItemSource::Scene(_) => Some(PropertyOverrides::default()),
        }
    }
}

fn is_string(schema: &PropertySchema) -> bool {
    schema.scalar_type(None, None) == Some(&ScalarPropertyType::String) && schema.is_editable(None)
}
