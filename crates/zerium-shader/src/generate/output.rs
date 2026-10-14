//! Publish a complete generated interface directory, with rollback on rename failure.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use super::{GENERATED_DIR, MANIFEST_FINGERPRINT, fingerprint};
use crate::ShaderError;

pub(crate) fn install(
    root: &Path,
    files: &BTreeMap<String, String>,
) -> Result<PathBuf, ShaderError> {
    let staging = root.join(format!(".{GENERATED_DIR}.tmp-{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| ShaderError::io("clear", &staging, error))?;
    }
    fs::create_dir(&staging).map_err(|error| ShaderError::io("create", &staging, error))?;
    let result = write_generated(&staging, files).and_then(|()| install_generated(root, &staging));
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    Ok(root.join(GENERATED_DIR))
}

fn write_generated(generated: &Path, files: &BTreeMap<String, String>) -> Result<(), ShaderError> {
    write(generated, MANIFEST_FINGERPRINT, &fingerprint(files))?;
    for (name, source) in files {
        write(generated, name, source)?;
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

fn write(generated: &Path, name: &str, source: &str) -> Result<(), ShaderError> {
    let path = generated.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| ShaderError::io("create", parent, error))?;
    }
    fs::write(&path, format!("{}\n", source.trim_end()))
        .map_err(|error| ShaderError::io("write", &path, error))
}
