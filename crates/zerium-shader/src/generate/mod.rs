//! Build deterministic host and property interfaces from validated core schemas.

use crate::ShaderError;
use contract::{ShaderContract, image_module, property_module};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use wesl::syntax::ModulePath;

pub(crate) mod contract;
mod image;
mod property;

pub use image::{IMAGE_SAMPLER_BINDING, ImageBinding};

pub(crate) const GENERATED_DIR: &str = "generated";
pub(crate) const MANIFEST_FINGERPRINT: &str = "manifest.fingerprint";
const UTIL_INTERFACE: &str = include_str!("wesl/util.wesl");

pub(crate) struct GeneratedModule {
    pub path: ModulePath,
    pub source: String,
}

pub(crate) fn generated_modules(
    contracts: &BTreeMap<String, ShaderContract<'_>>,
) -> Vec<GeneratedModule> {
    let mut modules = [
        (vec!["support", "util"], UTIL_INTERFACE.to_owned()),
        (
            vec!["host", "item"],
            include_str!("wesl/item.wesl").to_owned(),
        ),
        (
            vec!["host", "effect"],
            include_str!("wesl/effect.wesl").to_owned(),
        ),
        (
            vec!["host", "render"],
            include_str!("wesl/render.wesl").to_owned(),
        ),
        (
            vec!["host", "compute"],
            include_str!("wesl/compute.wesl").to_owned(),
        ),
        (
            vec!["host", "temporal"],
            include_str!("wesl/temporal.wesl").to_owned(),
        ),
        (
            vec!["_internal", "context"],
            include_str!("wesl/internal/context.wesl").to_owned(),
        ),
        (
            vec!["_internal", "item"],
            include_str!("wesl/internal/item.wesl").to_owned(),
        ),
        (
            vec!["_internal", "effect_state"],
            include_str!("wesl/internal/effect_state.wesl").to_owned(),
        ),
        (
            vec!["_internal", "properties"],
            include_str!("wesl/internal/properties.wesl").to_owned(),
        ),
        (vec!["_internal", "image"], image::shared_interface()),
        (
            vec!["_internal", "input"],
            image::interface(ImageBinding::INPUT),
        ),
        (
            vec!["_internal", "source"],
            image::interface(ImageBinding::SOURCE),
        ),
    ]
    .into_iter()
    .map(|(components, source)| {
        let mut path = ModulePath::new_root();
        path.push(GENERATED_DIR);
        for component in components {
            path.push(component);
        }
        GeneratedModule { path, source }
    })
    .collect::<Vec<_>>();

    for (entity, contract) in contracts {
        modules.push(GeneratedModule {
            path: property_module(entity),
            source: property::interface(contract.layout, contract.scope),
        });
        for (index, id) in contract.input_ids.iter().enumerate() {
            modules.push(GeneratedModule {
                path: image_module(entity, id),
                source: image::interface(ImageBinding::named(index)),
            });
        }
    }

    for module in &mut modules {
        module.source = module.source.replace("\r\n", "\n");
    }

    modules.sort_by(|left, right| left.path.components.cmp(&right.path.components));
    modules
}

pub(crate) fn fingerprint(modules: &[GeneratedModule]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for module in modules {
        // Module boundaries are explicit; normalize source exactly as write() does.
        for byte in module
            .path
            .to_string()
            .bytes()
            .chain([0])
            .chain(module.source.trim_end().bytes())
            .chain([b'\n', 0])
        {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    format!("{hash:016x}")
}

pub(crate) fn install(root: &Path, modules: &[GeneratedModule]) -> Result<PathBuf, ShaderError> {
    let staging = root.join(format!(".{GENERATED_DIR}.tmp-{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| ShaderError::io("clear", &staging, error))?;
    }
    fs::create_dir(&staging).map_err(|error| ShaderError::io("create", &staging, error))?;
    let result =
        write_generated(&staging, modules).and_then(|()| install_generated(root, &staging));
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    Ok(root.join(GENERATED_DIR))
}

fn write_generated(generated: &Path, modules: &[GeneratedModule]) -> Result<(), ShaderError> {
    write(
        generated,
        Path::new(MANIFEST_FINGERPRINT),
        &fingerprint(modules),
    )?;

    for module in modules {
        // The staging directory is already the generated namespace root.
        let relative = module.path.components[1..]
            .iter()
            .collect::<PathBuf>()
            .with_extension("wesl");
        write(generated, &relative, &module.source)?;
    }

    Ok(())
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

fn write(generated: &Path, name: &Path, source: &str) -> Result<(), ShaderError> {
    let path = generated.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| ShaderError::io("create", parent, error))?;
    }
    fs::write(&path, format!("{}\n", source.trim_end()))
        .map_err(|error| ShaderError::io("write", &path, error))
}
