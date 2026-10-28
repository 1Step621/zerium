//! GPU-independent shader descriptions consumed by the renderer.

use std::{collections::BTreeMap, fmt, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ItemShaderId(String);

impl ItemShaderId {
    pub fn plugin_item(plugin_id: &str, item_id: &str) -> Self {
        Self(format!("{plugin_id}::item::{item_id}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ItemShaderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EffectShaderId(String);

impl EffectShaderId {
    pub fn plugin_pass(plugin_id: &str, effect_id: &str, pass_index: usize) -> Self {
        Self(format!(
            "{plugin_id}::effect::{effect_id}::pass::{pass_index}"
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EffectShaderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug)]
pub struct ItemShaderDescriptor {
    pub id: ItemShaderId,
    pub label: String,
    pub wgsl: Arc<str>,
    pub vertex_entry: String,
    pub fragment_entry: String,
    pub vertex_count: u32,
}

#[derive(Clone, Debug)]
pub struct EffectShaderDescriptor {
    pub id: EffectShaderId,
    pub label: String,
    pub wgsl: Arc<str>,
    pub vertex_entry: String,
    pub fragment_entry: String,
    pub overrides: BTreeMap<String, f64>,
}

#[derive(Clone, Debug)]
pub struct ComputeShaderDescriptor {
    pub id: EffectShaderId,
    pub wgsl: Arc<str>,
    pub entry: String,
    pub workgroup_size: [u32; 3],
    pub overrides: BTreeMap<String, f64>,
}

#[derive(Clone, Debug)]
pub enum ValidatedEffectShader {
    Render(EffectShaderDescriptor),
    Compute(ComputeShaderDescriptor),
}

#[derive(Clone, Debug)]
pub struct ValidatedPluginShaders {
    pub(super) items: Vec<ItemShaderDescriptor>,
    pub(super) effects: Vec<ValidatedEffectShader>,
}

impl ValidatedPluginShaders {
    pub fn items(&self) -> &[ItemShaderDescriptor] {
        &self.items
    }

    pub fn effects(&self) -> &[ValidatedEffectShader] {
        &self.effects
    }
}
