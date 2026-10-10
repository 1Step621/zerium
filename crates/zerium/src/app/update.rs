use gpui::{App, AppContext as _};
use velopack::{UpdateCheck, UpdateManager, sources::GithubSource};

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

pub(crate) fn start(cx: &mut App) {
    cx.background_spawn(async {
        match download_update() {
            Ok(()) | Err(velopack::Error::NotInstalled(_)) => {}
            Err(error) => eprintln!("automatic update failed: {error}"),
        }
    })
    .detach();
}

fn download_update() -> Result<(), velopack::Error> {
    let source = GithubSource::new("https://github.com/1Step621/zerium", None, false);
    let manager = UpdateManager::new(source, None, None)?;
    if let UpdateCheck::UpdateAvailable(update) = manager.check_for_updates()? {
        manager.download_updates(&update, None)?;
    }
    Ok(())
}
