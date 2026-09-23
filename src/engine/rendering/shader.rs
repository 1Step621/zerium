use std::{fmt, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ItemShaderId(String);

#[derive(Clone, Copy, Debug)]
pub(crate) enum PluginShaderOwnerKind {
    Item,
    Effect,
}

impl PluginShaderOwnerKind {
    const fn namespace(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Effect => "effect",
        }
    }
}

impl ItemShaderId {
    pub(crate) fn plugin_item(plugin_id: &str, item_id: &str) -> Self {
        Self(format!("{plugin_id}::item::{item_id}"))
    }

    pub(crate) fn plugin_capability(
        plugin_id: &str,
        owner_kind: PluginShaderOwnerKind,
        owner_id: &str,
        id: &str,
    ) -> Self {
        let namespace = owner_kind.namespace();
        Self(format!(
            "{plugin_id}::{namespace}::{owner_id}::capability::{id}"
        ))
    }

    pub(crate) fn host_capability_frame() -> Self {
        Self("zerium::host::capability_frame".to_owned())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ItemShaderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct EffectShaderId(String);

impl EffectShaderId {
    pub(crate) fn plugin_pass(plugin_id: &str, effect_id: &str, pass_index: usize) -> Self {
        Self(format!(
            "{plugin_id}::effect::{effect_id}::pass::{pass_index}"
        ))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EffectShaderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug)]
pub(super) struct ItemShaderDescriptor {
    pub(super) id: ItemShaderId,
    pub(super) label: String,
    pub(super) wgsl: Arc<str>,
    pub(super) vertex_entry: String,
    pub(super) fragment_entry: String,
    pub(super) vertex_count: u32,
}

#[derive(Clone, Debug)]
pub(super) struct TextureShaderDescriptor {
    pub(super) id: ItemShaderId,
    pub(super) label: String,
    pub(super) wgsl: Arc<str>,
    pub(super) vertex_entry: String,
    pub(super) fragment_entry: String,
    pub(super) vertex_count: u32,
    pub(super) input_ids: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) struct EffectShaderDescriptor {
    pub(super) id: EffectShaderId,
    pub(super) label: String,
    pub(super) wgsl: Arc<str>,
    pub(super) vertex_entry: String,
    pub(super) fragment_entry: String,
}

#[derive(Clone, Debug)]
pub(super) struct ComputeShaderDescriptor {
    pub(super) id: EffectShaderId,
    pub(super) wgsl: Arc<str>,
    pub(super) entry: String,
    pub(super) workgroup_size: [u32; 3],
}

#[derive(Clone, Debug)]
pub(super) enum CompiledEffectShader {
    Render(EffectShaderDescriptor),
    Compute(ComputeShaderDescriptor),
    Temporal(EffectShaderDescriptor),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CompiledPluginShaders {
    pub(super) items: Vec<ItemShaderDescriptor>,
    pub(super) textures: Vec<TextureShaderDescriptor>,
    pub(super) effects: Vec<CompiledEffectShader>,
}
