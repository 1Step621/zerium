use std::{collections::BTreeMap, fs, path::Path};

use crate::domain::plugin::{PluginManifest, ShaderKind, VisualCapability};

mod property;

const GENERATED_DIR: &str = "generated";
const MANIFEST_FINGERPRINT: &str = "manifest.fingerprint";
const UTIL_INTERFACE: &str = include_str!("wesl/util.wesl");

struct ShaderContract {
    kind: ShaderKind,
    properties: property::Layout,
    media_interface: Option<String>,
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
    validate_source_modules(&manifest)?;
    for source in manifest.shader_sources() {
        fs::read_to_string(root.join(source))
            .map_err(|error| format!("shader source '{source}' could not be read: {error}"))?;
    }

    let mut contracts = BTreeMap::<String, ShaderContract>::new();
    for schema in manifest.items() {
        let Some(visual) = schema.visual() else {
            continue;
        };
        let shader = visual.shader();
        insert_contract(
            &mut contracts,
            shader.source(),
            ShaderContract {
                kind: ShaderKind::Item,
                properties: property::Layout::from_abi(schema.property_layout()),
                media_interface: matches!(
                    visual,
                    VisualCapability::Media { .. }
                        | VisualCapability::Text { .. }
                        | VisualCapability::RenderResult { .. }
                )
                .then(|| texture_media_interface(&schema.texture_input_ids())),
            },
        )?;
    }
    for schema in manifest.effects() {
        for pass in schema.passes() {
            insert_contract(
                &mut contracts,
                pass.shader_source(),
                ShaderContract {
                    kind: pass.shader_kind(),
                    properties: property::Layout::from_abi(schema.property_layout()),
                    media_interface: None,
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
        &crate::plugin::manifest_fingerprint(manifest_source),
    )?;
    write(generated, "util.wesl", UTIL_INTERFACE)?;
    for kind in ShaderKind::ALL {
        let source = host_interface(kind);
        write(generated, &format!("{}.wesl", kind.module_name()), &source)?;
    }
    for (source_name, contract) in contracts {
        let module = source_module_name(source_name);
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
        let media_module = contract
            .media_interface
            .as_ref()
            .map(|_| format!("media_interface_{module}"));
        if let Some(media_module) = &media_module {
            write(
                generated,
                &format!("{media_module}.wesl"),
                contract.media_interface.as_deref().unwrap(),
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
    source: &str,
    contract: ShaderContract,
) -> Result<(), String> {
    if let Some(existing) = contracts.get_mut(source) {
        if existing.kind != contract.kind || existing.media_interface != contract.media_interface {
            return Err(format!(
                "shader source '{source}' is used with incompatible shader contracts"
            ));
        }
        existing.properties.retain_compatible(&contract.properties);
        return Ok(());
    }
    contracts.insert(source.to_owned(), contract);
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

fn validate_source_modules(manifest: &PluginManifest) -> Result<(), String> {
    let mut source_modules = BTreeMap::<String, &str>::new();

    for source in manifest.shader_sources() {
        let source_module = source_module_name(source);
        if let Some(existing) = source_modules.insert(source_module.clone(), source)
            && existing != source
        {
            return Err(format!(
                "shader sources '{existing}' and '{source}' map to the same generated module '{source_module}'"
            ));
        }
    }
    Ok(())
}

fn source_module_name(source: &str) -> String {
    let stem = source
        .strip_suffix(".wesl")
        .expect("validated shader sources end in .wesl");
    let mut name = String::with_capacity(stem.len());
    if stem.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        name.push('_');
    }
    for byte in stem.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'_' {
            name.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            write!(name, "_{byte:02x}").expect("writing to a String cannot fail");
        }
    }
    name
}

fn texture_media_interface(input_ids: &[String]) -> String {
    let sampler_binding = 2 + input_ids.len();
    let metadata_binding = sampler_binding + 1;
    let mut source = format!(
        "struct ZeriumMedia {{\n    source_sizes: array<vec4<f32>, {}>,\n    target_size: vec2<f32>,\n    padding: vec2<f32>,\n}};\n\n",
        input_ids.len()
    );
    for index in 0..input_ids.len() {
        source.push_str(&format!(
            "@group(0) @binding({})\nvar slot_{}: texture_2d<f32>;\n\nfn slot_{}_size() -> vec2<f32> {{\n    return media_inputs.source_sizes[{}].xy;\n}}\n\n",
            index + 2,
            index,
            index,
            index
        ));
    }
    source.push_str(&format!(
        "@group(0) @binding({sampler_binding})\nvar media_sampler: sampler;\n\n@group(0) @binding({metadata_binding})\nvar<uniform> media_inputs: ZeriumMedia;\n"
    ));
    source
}
