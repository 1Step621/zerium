use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crate::{ShaderContract, ShaderError, capability_input, plugin_directory, shader_contracts};
use zerium_core::plugin::{PluginManifest, ShaderKind};

mod property;

const GENERATED_DIR: &str = "generated";
const MANIFEST_FINGERPRINT: &str = "manifest.fingerprint";
const UTIL_INTERFACE: &str = include_str!("wesl/util.wesl");

pub fn generate(path: Option<&Path>) -> Result<PathBuf, ShaderError> {
    let root = plugin_directory(path)?;
    let manifest_path = root.join("plugin.json");
    let manifest_source = fs::read_to_string(&manifest_path)
        .map_err(|error| ShaderError::io("read", &manifest_path, error))?;
    let manifest = PluginManifest::from_json(&manifest_source)?;
    for module in manifest.shader_modules() {
        fs::read_to_string(root.join(format!("{module}.wesl"))).map_err(|error| {
            ShaderError::backend(format!(
                "shader module '{module}' could not be read: {error}"
            ))
        })?;
    }

    let contracts = shader_contracts(&manifest)?;

    let staging = root.join(format!(".{GENERATED_DIR}.tmp-{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| ShaderError::io("clear", &staging, error))?;
    }
    fs::create_dir(&staging).map_err(|error| ShaderError::io("create", &staging, error))?;
    let result =
        write_generated(&staging, &contracts).and_then(|()| install_generated(&root, &staging));
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    Ok(root.join(GENERATED_DIR))
}

fn write_generated(
    generated: &Path,
    contracts: &BTreeMap<String, ShaderContract>,
) -> Result<(), ShaderError> {
    let modules = generated_modules(contracts);
    write(generated, MANIFEST_FINGERPRINT, &fingerprint(&modules))?;
    let host = generated.join("host");
    fs::create_dir(&host).map_err(|error| ShaderError::io("create", &host, error))?;
    for (name, source) in modules {
        write(generated, &name, &source)?;
    }
    Ok(())
}

fn generated_modules(contracts: &BTreeMap<String, ShaderContract>) -> BTreeMap<String, String> {
    let mut modules = BTreeMap::from([
        ("host/util.wesl".to_owned(), UTIL_INTERFACE.to_owned()),
        (
            "host/_context.wesl".to_owned(),
            include_str!("wesl/_context.wesl").to_owned(),
        ),
    ]);
    for kind in ShaderKind::ALL {
        modules.insert(
            format!("host/{}.wesl", kind.module_name()),
            host_interface(kind).to_owned(),
        );
        modules.insert(
            format!("host/{}.wesl", kind.internal_module_name()),
            internal_interface(kind),
        );
    }
    for (module, contract) in contracts {
        let source = property::interface(&contract.properties, contract.kind)
            + "\n"
            + &capability_interface(&contract.input_ids);
        modules.insert(format!("{module}.wesl"), source);
    }
    for source in modules.values_mut() {
        *source = source.replace("\r\n", "\n");
    }
    modules
}

pub fn shader_contract_fingerprint(contracts: &BTreeMap<String, ShaderContract>) -> String {
    fingerprint(&generated_modules(contracts))
}

fn fingerprint(modules: &BTreeMap<String, String>) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for (name, source) in modules {
        // File boundaries are explicit; normalize content exactly as write() does.
        for byte in name
            .bytes()
            .chain([0])
            .chain(source.trim_end().bytes())
            .chain([b'\n', 0])
        {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

fn install_generated(root: &Path, staging: &Path) -> Result<(), ShaderError> {
    let generated = root.join(GENERATED_DIR);
    let backup = root.join(format!(".{GENERATED_DIR}.old-{}", std::process::id()));
    if backup.exists() {
        fs::remove_dir_all(&backup).map_err(|error| ShaderError::io("clear", &backup, error))?;
    }
    if generated.exists() {
        fs::rename(&generated, &backup)
            .map_err(|error| ShaderError::io("prepare generated directory", &generated, error))?;
    }
    if let Err(error) = fs::rename(staging, &generated) {
        if backup.exists() {
            let _ = fs::rename(&backup, &generated);
        }
        return Err(ShaderError::io(
            "install generated directory",
            &generated,
            error,
        ));
    }
    if backup.exists() {
        fs::remove_dir_all(&backup).map_err(|error| ShaderError::io("remove", &backup, error))?;
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

fn write(generated: &Path, name: &str, source: &str) -> Result<(), ShaderError> {
    let path = generated.join(name);
    fs::write(&path, format!("{}\n", source.trim_end()))
        .map_err(|error| ShaderError::io("write", &path, error))
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
