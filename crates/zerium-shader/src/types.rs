use std::{fmt, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ItemShaderId(String);

impl ItemShaderId {
    pub fn plugin_item(plugin_id: &str, item_id: &str) -> Self {
        Self(format!("{plugin_id}::item::{item_id}"))
    }

    pub fn host_capability_frame() -> Self {
        Self("zerium::host::capability_frame".to_owned())
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
pub struct TextureShaderDescriptor {
    pub id: ItemShaderId,
    pub label: String,
    pub wgsl: Arc<str>,
    pub vertex_entry: String,
    pub fragment_entry: String,
    pub vertex_count: u32,
    pub input_ids: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct EffectShaderDescriptor {
    pub id: EffectShaderId,
    pub label: String,
    pub wgsl: Arc<str>,
    pub vertex_entry: String,
    pub fragment_entry: String,
}

#[derive(Clone, Debug)]
pub struct ComputeShaderDescriptor {
    pub id: EffectShaderId,
    pub wgsl: Arc<str>,
    pub entry: String,
    pub workgroup_size: [u32; 3],
}

#[derive(Clone, Debug)]
pub enum CompiledEffectShader {
    Render(EffectShaderDescriptor),
    Compute(ComputeShaderDescriptor),
    Temporal(EffectShaderDescriptor),
}

#[derive(Clone, Debug, Default)]
pub struct CompiledPluginShaders {
    pub items: Vec<ItemShaderDescriptor>,
    pub textures: Vec<TextureShaderDescriptor>,
    pub effects: Vec<CompiledEffectShader>,
}
