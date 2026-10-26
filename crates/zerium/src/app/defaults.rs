//! User defaults are creation inputs, never a replacement for plugin schemas.
use std::collections::BTreeMap;

use gpui::App;
use serde::{Deserialize, Serialize};
use zerium_core::{
    property::{PropertyOverrides, PropertySchema, PropertyValue},
    timeline::{EffectInstanceId, TimelineItem},
};

use super::storage::config::ConfigStore;

type Properties = BTreeMap<String, PropertyValue>;
type Definitions = BTreeMap<String, BTreeMap<String, Properties>>;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct CreationDefaults {
    items: Definitions,
    effects: Definitions,
}

#[derive(Clone)]
pub(crate) enum DefaultsTarget {
    Item {
        plugin_id: String,
        item_id: String,
    },
    Effect {
        plugin_id: String,
        effect_id: String,
    },
}

impl DefaultsTarget {
    pub(crate) fn for_item(item: &TimelineItem, effect: Option<EffectInstanceId>) -> Option<Self> {
        if let Some(id) = effect {
            let effect = item.effects.iter().find(|effect| effect.id == id)?;
            Some(Self::Effect {
                plugin_id: effect.plugin_id.clone(),
                effect_id: effect.effect_id.clone(),
            })
        } else {
            Some(Self::Item {
                plugin_id: item.plugin_id()?.to_owned(),
                item_id: item.item_id()?.to_owned(),
            })
        }
    }

    fn ids(&self) -> (&str, &str) {
        match self {
            Self::Item { plugin_id, item_id } => (plugin_id, item_id),
            Self::Effect {
                plugin_id,
                effect_id,
            } => (plugin_id, effect_id),
        }
    }
}

impl CreationDefaults {
    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty() && self.effects.is_empty()
    }

    fn get(&self, target: &DefaultsTarget) -> Option<&Properties> {
        let definitions = match target {
            DefaultsTarget::Item { .. } => &self.items,
            DefaultsTarget::Effect { .. } => &self.effects,
        };
        let (plugin, definition) = target.ids();
        definitions.get(plugin)?.get(definition)
    }

    pub(crate) fn store(&mut self, target: &DefaultsTarget, mut values: Properties) {
        for value in values.values_mut() {
            value.map_file_paths(&mut |path| {
                std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
            });
        }
        let definitions = match target {
            DefaultsTarget::Item { .. } => &mut self.items,
            DefaultsTarget::Effect { .. } => &mut self.effects,
        };
        let (plugin, definition) = target.ids();
        definitions
            .entry(plugin.to_owned())
            .or_default()
            .insert(definition.to_owned(), values);
    }
}

pub(crate) fn properties(
    cx: &App,
    target: &DefaultsTarget,
    schemas: &[PropertySchema],
) -> PropertyOverrides {
    let mut values = PropertyOverrides::default();
    for schema in schemas {
        if let Some(value) = property(cx, target, schema) {
            // Stored settings may outlive a plugin's property contract.
            values
                .set(schema, value)
                .expect("stored default was validated");
        }
    }
    values
}

pub(crate) fn property(
    cx: &App,
    target: &DefaultsTarget,
    schema: &PropertySchema,
) -> Option<PropertyValue> {
    let value = ConfigStore::read(cx)
        .ok()?
        .creation_defaults
        .get(target)?
        .get(schema.id())?;
    (schema.is_editable(None) && schema.accepts_value(value)).then(|| value.clone())
}
