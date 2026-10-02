//! Zerium application entry point.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![deny(unreachable_pub)]

rust_i18n::i18n!("../../locales", fallback = "en-us");

mod app;
mod engine;
mod i18n;
mod plugin_loader;
mod project_session;
mod ui;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = std::env::args().skip(1);
    let Some(command) = arguments.next() else {
        app::run(None);
        return Ok(());
    };
    if command == "plugin" {
        match arguments.next().as_deref() {
            Some("generate") => {
                let path = arguments.next().map(std::path::PathBuf::from);
                let generated = zerium_shader::generate(path.as_deref())
                    .map_err(|error| format!("zerium plugin generate: {error}"))?;
                println!("generated WESL modules in {}", generated.display());
            }
            Some("validate") => {
                let path = arguments.next().map(std::path::PathBuf::from);
                zerium_shader::validate(path.as_deref())
                    .map_err(|error| format!("zerium plugin validate: {error}"))?;
                println!("plugin is valid");
            }
            _ => return Err("zerium plugin: unknown command".to_owned()),
        }
    } else {
        app::run(Some(command.into()));
    }
    Ok(())
}
