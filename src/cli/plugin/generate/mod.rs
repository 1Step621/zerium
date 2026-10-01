use std::{collections::BTreeMap, fs, path::Path};

use crate::domain::plugin::{
    PluginManifest, ShaderContract, ShaderKind, shader_contract_fingerprint, shader_contracts,
};
use crate::engine::rendering::capability_input;

mod property;

const GENERATED_DIR: &str = "generated";
const MANIFEST_FINGERPRINT: &str = "manifest.fingerprint";
const UTIL_INTERFACE: &str = include_str!("wesl/util.wesl");

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

    let contracts = shader_contracts(&manifest).map_err(|error| error.to_string())?;

    let staging = root.join(format!(".{GENERATED_DIR}.tmp-{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|error| format!("cannot clear '{}': {error}", staging.display()))?;
    }
    fs::create_dir(&staging)
        .map_err(|error| format!("cannot create '{}': {error}", staging.display()))?;
    let result = write_generated(&staging, &contracts);
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
    contracts: &BTreeMap<String, ShaderContract>,
) -> Result<(), String> {
    write(
        generated,
        MANIFEST_FINGERPRINT,
        &shader_contract_fingerprint(contracts).map_err(|error| error.to_string())?,
    )?;
    let host = generated.join("host");
    fs::create_dir(&host)
        .map_err(|error| format!("cannot create '{}': {error}", host.display()))?;
    write(&host, "util.wesl", UTIL_INTERFACE)?;
    write(&host, "_context.wesl", include_str!("wesl/_context.wesl"))?;
    for kind in ShaderKind::ALL {
        write(
            &host,
            &format!("{}.wesl", kind.module_name()),
            host_interface(kind),
        )?;
        write(
            &host,
            &format!("{}.wesl", kind.internal_module_name()),
            &internal_interface(kind),
        )?;
    }
    for (module, contract) in contracts {
        let property_interface = property::interface(&contract.properties, contract.kind);
        let host = if contract.properties.is_empty() {
            String::new()
        } else {
            format!(
                "import package::generated::host::_props::{{{}}};\n\n",
                property_imports(contract.kind)
            )
        };
        let source = host + &property_interface + "\n" + &capability_interface(&contract.input_ids);
        write(generated, &format!("{module}.wesl"), &source)?;
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

fn capability_interface(input_ids: &[String]) -> String {
    if input_ids.is_empty() {
        return String::new();
    }
    let mut source = String::new();
    for (binding, id) in input_ids.iter().enumerate() {
        source.push_str(&format!(
            "@group(1) @binding({binding})\nvar {id}: texture_2d<f32>;\n\n"
        ));
    }
    source.push_str(&format!(
        "@group(1) @binding({})\nvar capability_sampler: sampler;\n",
        capability_input::SAMPLER_BINDING,
    ));
    source
}

fn write(generated: &Path, name: &str, source: &str) -> Result<(), String> {
    fs::write(generated.join(name), format!("{}\n", source.trim_end()))
        .map_err(|error| format!("cannot write '{}': {error}", generated.join(name).display()))
}

fn host_interface(kind: ShaderKind) -> &'static str {
    match kind {
        ShaderKind::Item => include_str!("wesl/item.wesl"),
        ShaderKind::Effect => include_str!("wesl/effect.wesl"),
        ShaderKind::Compute => include_str!("wesl/compute.wesl"),
        ShaderKind::Temporal => include_str!("wesl/temporal.wesl"),
    }
}

fn internal_interface(kind: ShaderKind) -> String {
    let kind_source = match kind {
        ShaderKind::Item => include_str!("wesl/_item.wesl"),
        ShaderKind::Effect => include_str!("wesl/_effect.wesl"),
        ShaderKind::Compute => include_str!("wesl/_compute.wesl"),
        ShaderKind::Temporal => include_str!("wesl/_temporal.wesl"),
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
