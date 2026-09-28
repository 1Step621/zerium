use std::{collections::BTreeMap, fs, path::Path};

use crate::domain::plugin::{PluginManifest, ShaderKind};
use crate::engine::rendering::capability_input;

mod property;

const GENERATED_DIR: &str = "generated";
const MANIFEST_FINGERPRINT: &str = "manifest.fingerprint";
const UTIL_INTERFACE: &str = include_str!("wesl/util.wesl");

struct ShaderContract {
    kind: ShaderKind,
    properties: property::Layout,
    capability_interface: String,
}

pub(crate) fn generate(path: Option<&Path>) -> Result<(), String> {
    let root = path
        .map(Path::to_owned)
        .unwrap_or(std::env::current_dir().map_err(|error| error.to_string())?);
    let root = root.canonicalize().map_err(|error| {
        format!(
            "cannot access plugin directory '{}': {error}",
            root.display()
        )
    })?;
    let manifest_path = root.join("plugin.json");
    let manifest_source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read '{}': {error}", manifest_path.display()))?;
    let manifest =
        PluginManifest::from_json(&manifest_source).map_err(|error| error.to_string())?;
    for module in manifest.shader_modules() {
        fs::read_to_string(root.join(format!("{module}.wesl")))
            .map_err(|error| format!("shader module '{module}' could not be read: {error}"))?;
    }

    let mut contracts = BTreeMap::<String, ShaderContract>::new();
    for schema in manifest.items() {
        let layout = property::Layout::from_abi(schema.property_layout());
        if let Some(shader) = schema.shader() {
            insert_contract(
                &mut contracts,
                shader.module(),
                ShaderContract {
                    kind: ShaderKind::Item,
                    properties: layout.clone(),
                    capability_interface: capability_input::interface(schema.capabilities()),
                },
            )?;
        }
    }
    for schema in manifest.effects() {
        let layout = property::Layout::from_abi(schema.property_layout());
        for pass in schema.passes() {
            insert_contract(
                &mut contracts,
                pass.shader_module(),
                ShaderContract {
                    kind: pass.shader_kind(),
                    properties: layout.clone(),
                    capability_interface: capability_input::interface(schema.capabilities()),
                },
            )?;
        }
    }

    let staging = root.join(format!(".{GENERATED_DIR}.tmp-{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|error| format!("cannot clear '{}': {error}", staging.display()))?;
    }
    fs::create_dir(&staging)
        .map_err(|error| format!("cannot create '{}': {error}", staging.display()))?;
    let result = write_generated(&staging, &manifest_source, &contracts);
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    install_generated(&root, &staging)?;
    println!(
        "generated WESL modules in {}",
        root.join(GENERATED_DIR).display()
    );
    Ok(())
}

fn write_generated(
    generated: &Path,
    manifest_source: &str,
    contracts: &BTreeMap<String, ShaderContract>,
) -> Result<(), String> {
    write(
        generated,
        MANIFEST_FINGERPRINT,
        &crate::plugin_loader::manifest_fingerprint(manifest_source),
    )?;
    write(generated, "util.wesl", UTIL_INTERFACE)?;
    for kind in ShaderKind::ALL {
        let source = host_interface(kind);
        write(generated, &format!("{}.wesl", kind.module_name()), &source)?;
    }
    for (module, contract) in contracts {
        let property_interface = contract.properties.interface(contract.kind);
        let host = format!(
            "import package::generated::{}::{{{}}};\n\n",
            contract.kind.module_name(),
            property_imports(contract.kind)
        );
        write(
            generated,
            &format!("properties_{module}.wesl"),
            &(host + &property_interface),
        )?;
        if contract.capability_interface.contains("texture_2d<f32>") {
            write(
                generated,
                &format!("capability_input_{module}.wesl"),
                &contract.capability_interface,
            )?;
        }
    }
    Ok(())
}

fn install_generated(root: &Path, staging: &Path) -> Result<(), String> {
    let generated = root.join(GENERATED_DIR);
    let backup = root.join(format!(".{GENERATED_DIR}.old-{}", std::process::id()));
    if backup.exists() {
        fs::remove_dir_all(&backup)
            .map_err(|error| format!("cannot clear '{}': {error}", backup.display()))?;
    }
    if generated.exists() {
        fs::rename(&generated, &backup).map_err(|error| {
            format!(
                "cannot prepare generated directory '{}': {error}",
                generated.display()
            )
        })?;
    }
    if let Err(error) = fs::rename(staging, &generated) {
        if backup.exists() {
            let _ = fs::rename(&backup, &generated);
        }
        return Err(format!(
            "cannot install generated directory '{}': {error}",
            generated.display()
        ));
    }
    if backup.exists() {
        fs::remove_dir_all(&backup)
            .map_err(|error| format!("cannot remove '{}': {error}", backup.display()))?;
    }
    Ok(())
}

fn insert_contract(
    contracts: &mut BTreeMap<String, ShaderContract>,
    module: &str,
    contract: ShaderContract,
) -> Result<(), String> {
    if let Some(existing) = contracts.get_mut(module) {
        if existing.kind != contract.kind
            || existing.capability_interface != contract.capability_interface
        {
            return Err(format!(
                "shader module '{module}' is used with incompatible shader contracts"
            ));
        }
        existing.properties.retain_compatible(&contract.properties);
        return Ok(());
    }
    contracts.insert(module.to_owned(), contract);
    Ok(())
}

fn write(generated: &Path, name: &str, source: &str) -> Result<(), String> {
    fs::write(generated.join(name), source)
        .map_err(|error| format!("cannot write '{}': {error}", generated.join(name).display()))
}

fn host_interface(kind: ShaderKind) -> String {
    let kind_source = match kind {
        ShaderKind::Item => include_str!("wesl/item.wesl"),
        ShaderKind::Effect => include_str!("wesl/effect.wesl"),
        ShaderKind::Compute => include_str!("wesl/compute.wesl"),
        ShaderKind::Temporal => include_str!("wesl/temporal.wesl"),
    };
    format!("{kind_source}\n{}", include_str!("wesl/raw_props.wesl"))
}

const fn property_imports(kind: ShaderKind) -> &'static str {
    match kind {
        ShaderKind::Item => "ZeriumRawProps, item_props, read_u32, read_i32, read_f32, read_bool",
        ShaderKind::Effect | ShaderKind::Compute | ShaderKind::Temporal => {
            "ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool"
        }
    }
}
