fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../locales");
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => {
            // Leave room to expand dependency paths when packaging the app.
            println!("cargo:rustc-link-arg=-Wl,-headerpad_max_install_names");
        }
        Ok("windows") => {
            let icon = "../../assets/zerium.ico";
            println!("cargo:rerun-if-changed={icon}");
            winresource::WindowsResource::new()
                .set_icon(icon)
                .set("ProductName", "Zerium")
                .compile()?;
        }
        _ => {}
    }
    Ok(())
}
