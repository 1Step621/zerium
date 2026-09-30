//! Zerium application library.
//!
//! The binary is intentionally only a process entry point. The application is
//! not a reusable SDK yet, so its internal layers stay private while the
//! project is under active development.

#![deny(unreachable_pub)]

rust_i18n::i18n!("locales", fallback = "en-us");

// Track locale files as Cargo inputs so changes to YAML rebuild the embedded catalog.
const _: [&str; 2] = [
    include_str!("../locales/ja-jp.yml"),
    include_str!("../locales/en-us.yml"),
];

mod app;
mod cli;
mod domain;
mod engine;
mod i18n;
mod plugin_loader;
mod project_session;
mod ui;

pub fn run() -> Result<(), String> {
    let mut arguments = std::env::args().skip(1);
    let Some(command) = arguments.next() else {
        app::run(None);
        return Ok(());
    };
    if command == "plugin" {
        match arguments.next().as_deref() {
            Some("generate") => {
                let path = arguments.next().map(std::path::PathBuf::from);
                cli::plugin::generate(path.as_deref())
                    .map_err(|error| format!("zerium plugin generate: {error}"))?;
            }
            Some("validate") => {
                let path = arguments.next().map(std::path::PathBuf::from);
                cli::plugin::validate(path.as_deref())
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
