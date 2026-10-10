#[cfg(target_os = "windows")]
#[path = "windows_install.rs"]
mod windows_install;

pub(crate) fn initialize() {
    let mut updater = velopack::VelopackApp::build();
    #[cfg(target_os = "windows")]
    {
        use velopack::locator::{LocationContext, auto_locate_app_manifest};

        if auto_locate_app_manifest(LocationContext::FromCurrentExe)
            .is_ok_and(|locator| !locator.get_is_portable())
        {
            updater = updater
                .on_after_install_fast_callback(|_| windows_install::configure(true))
                .on_after_update_fast_callback(|_| windows_install::configure(true))
                .on_before_uninstall_fast_callback(|_| windows_install::configure(false));
        }
    }
    updater.run();
}
