//! Zerium application library.
//!
//! The binary is intentionally only a process entry point. The application is
//! not a reusable SDK yet, so its internal layers stay private while the
//! project is under active development.

#![deny(unreachable_pub)]

mod app;
mod domain;
mod engine;
mod plugin_catalog;
mod plugin_generator;
mod ui;

pub fn run() {
    let mut arguments = std::env::args().skip(1);
    if arguments.next().as_deref() == Some("plugin") {
        match arguments.next().as_deref() {
            Some("generate") => {
                let path = arguments.next().map(std::path::PathBuf::from);
                if let Err(error) = plugin_generator::run(path.as_deref()) {
                    eprintln!("zerium plugin generate: {error}");
                    std::process::exit(1);
                }
                return;
            }
            Some("validate") => {
                let path = arguments.next().map(std::path::PathBuf::from);
                if let Err(error) = plugin_catalog::validate(path.as_deref()) {
                    eprintln!("zerium plugin validate: {error}");
                    std::process::exit(1);
                }
                println!("plugin is valid");
                return;
            }
            _ => {}
        }
    }
    app::run();
}
