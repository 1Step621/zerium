//! Per-user integration for Velopack installs; portable copies do not run these hooks.
use std::{io, path::Path};
use winreg::{
    HKCU,
    enums::{KEY_READ, KEY_WRITE},
};

pub(crate) fn configure(install: bool) {
    let result = std::env::current_exe().and_then(|exe| {
        let directory = exe
            .parent()
            .ok_or_else(|| io::Error::other("missing executable directory"))?;
        configure_registry(directory, install)
    });
    if let Err(error) = result {
        eprintln!("failed to configure Windows integration: {error}");
    }
}

fn configure_registry(directory: &Path, install: bool) -> io::Result<()> {
    let classes = HKCU.create_subkey(r"Software\Classes")?.0;
    if install {
        let executable = directory.join("zerium.exe");
        for (key, value) in [
            ("Zerium.Project", "Zerium Project".to_owned()),
            (
                r"Zerium.Project\DefaultIcon",
                format!("\"{}\",0", executable.display()),
            ),
            (
                r"Zerium.Project\shell\open\command",
                format!("\"{}\" \"%1\"", executable.display()),
            ),
        ] {
            classes.create_subkey(key)?.0.set_value("", &value)?;
        }
        classes
            .create_subkey(r".zero\OpenWithProgids")?
            .0
            .set_value("Zerium.Project", &"")?;
        let extension = classes.create_subkey(".zero")?.0;
        if extension
            .get_value::<String, _>("")
            .unwrap_or_default()
            .is_empty()
        {
            extension.set_value("", &"Zerium.Project")?;
        }
    } else {
        classes
            .delete_subkey_all("Zerium.Project")
            .or_else(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(error)
                }
            })?;
        if let Ok(extension) = classes.open_subkey_with_flags(".zero", KEY_READ | KEY_WRITE) {
            if extension.get_value::<String, _>("").ok().as_deref() == Some("Zerium.Project") {
                extension.delete_value("")?;
            }
            if let Ok(open_with) = extension.open_subkey_with_flags("OpenWithProgids", KEY_WRITE) {
                let _ = open_with.delete_value("Zerium.Project");
            }
        }
    }

    unsafe {
        use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
    Ok(())
}
