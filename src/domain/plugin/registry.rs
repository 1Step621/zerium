//! Deterministic lookup across structurally valid plugins with resolved assets.
//!
//! The registry does not claim that renderer-composed WGSL has been compiled. The rendering layer
//! must validate each complete module before treating a registered plugin as executable.

use std::{
    collections::{BTreeMap, btree_map::Entry},
    sync::Arc,
};

use super::{EffectSchema, ItemSchema, Plugin, PluginError, PluginManifest};

#[derive(Clone, Debug, Default)]
pub(crate) struct PluginRegistry {
    plugins: BTreeMap<String, Arc<Plugin>>,
    items: BTreeMap<(String, String), Arc<ItemSchema>>,
    effects: BTreeMap<(String, String), Arc<EffectSchema>>,
}

impl PluginRegistry {
    pub(crate) fn new(plugins: impl IntoIterator<Item = Plugin>) -> Result<Self, PluginError> {
        let mut registry = Self::default();
        for plugin in plugins {
            registry.register(plugin)?;
        }
        Ok(registry)
    }

    pub(crate) fn register(&mut self, plugin: Plugin) -> Result<(), PluginError> {
        let plugin = Arc::new(plugin);
        let plugin_id = plugin.manifest().id().to_owned();
        let entry = match self.plugins.entry(plugin_id.clone()) {
            Entry::Vacant(entry) => entry,
            Entry::Occupied(_) => {
                return Err(PluginError::invalid_definition(format!(
                    "duplicate plugin ID '{plugin_id}'"
                )));
            }
        };
        for schema in plugin.manifest().item_schemas() {
            self.items.insert(
                (plugin_id.clone(), schema.id().to_owned()),
                Arc::clone(schema),
            );
        }
        for schema in plugin.manifest().effect_schemas() {
            self.effects.insert(
                (plugin_id.clone(), schema.id().to_owned()),
                Arc::clone(schema),
            );
        }
        entry.insert(plugin);
        Ok(())
    }

    pub(crate) fn manifests(&self) -> impl Iterator<Item = &PluginManifest> {
        self.plugins.values().map(|plugin| plugin.manifest())
    }

    pub(crate) fn items(&self) -> impl Iterator<Item = (&str, &ItemSchema)> {
        self.items
            .iter()
            .map(|((plugin_id, _), schema)| (plugin_id.as_str(), schema.as_ref()))
    }

    pub(crate) fn effects(&self) -> impl Iterator<Item = (&str, &EffectSchema)> {
        self.effects
            .iter()
            .map(|((plugin_id, _), schema)| (plugin_id.as_str(), schema.as_ref()))
    }

    pub(crate) fn item(&self, plugin_id: &str, item_id: &str) -> Option<Arc<ItemSchema>> {
        self.items
            .get(&(plugin_id.to_owned(), item_id.to_owned()))
            .cloned()
    }

    pub(crate) fn effect(&self, plugin_id: &str, effect_id: &str) -> Option<Arc<EffectSchema>> {
        self.effects
            .get(&(plugin_id.to_owned(), effect_id.to_owned()))
            .cloned()
    }

    pub(crate) fn shader_source(&self, plugin_id: &str, source: &str) -> Option<&str> {
        self.plugin(plugin_id)?.shader_source(source)
    }

    pub(crate) fn plugin(&self, plugin_id: &str) -> Option<&Plugin> {
        self.plugins.get(plugin_id).map(Arc::as_ref)
    }
}
