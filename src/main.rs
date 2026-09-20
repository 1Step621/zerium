#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    if let Err(error) = zerium::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
