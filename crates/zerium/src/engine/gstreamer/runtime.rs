//! Shared GStreamer runtime for media input and file encoding.
use std::sync::OnceLock;

pub(crate) fn initialize() -> Result<(), String> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| gstreamer::init().map_err(|error| error.to_string()))
        .clone()
}

/// Configure relocated release bundles before any application threads start.
pub(crate) fn configure_bundle() {
    let Some(directory) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(ToOwned::to_owned))
    else {
        return;
    };
    #[cfg(target_os = "macos")]
    let (plugins, scanner) = (
        directory.join("../Frameworks/gstreamer-1.0"),
        directory.join("gst-plugin-scanner"),
    );
    #[cfg(target_os = "windows")]
    let (plugins, scanner) = (
        directory.join("lib/gstreamer-1.0"),
        directory.join("libexec/gstreamer-1.0/gst-plugin-scanner.exe"),
    );
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let (plugins, scanner) = (
        directory.join("../lib/gstreamer-1.0"),
        directory.join("../libexec/gstreamer-1.0/gst-plugin-scanner"),
    );
    // SAFETY: main calls this before creating threads or initializing libraries.
    unsafe {
        #[cfg(target_os = "windows")]
        if plugins.is_dir() {
            let existing = std::env::var_os("PATH").unwrap_or_default();
            if let Ok(path) = std::env::join_paths(
                std::iter::once(directory).chain(std::env::split_paths(&existing)),
            ) {
                std::env::set_var("PATH", path);
            }
        }
        if plugins.is_dir() && std::env::var_os("GST_PLUGIN_SYSTEM_PATH_1_0").is_none() {
            std::env::set_var("GST_PLUGIN_SYSTEM_PATH_1_0", plugins);
        }
        if scanner.is_file() && std::env::var_os("GST_PLUGIN_SCANNER_1_0").is_none() {
            std::env::set_var("GST_PLUGIN_SCANNER_1_0", scanner);
        }
    }
}
