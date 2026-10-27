//! Scene import menus shared by the workspace and timeline.
use ::ui::{IconName, menu::PopupMenu};
use gpui::{Context, FocusHandle, Window};
use rust_i18n::t;
use zerium_core::persistence::SCENE_FILE_SUFFIX;

use crate::app::{actions::ImportScene, storage::config::ConfigStore};

pub(crate) fn scene_library_menu(
    menu: PopupMenu,
    action_context: FocusHandle,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let scenes = ConfigStore::read(cx)
        .map(|config| config.scene_library.files().to_vec())
        .unwrap_or_default();
    if scenes.is_empty() {
        return menu;
    }
    menu.submenu(
        t!("scene_library.import").to_string(),
        window,
        cx,
        move |menu, _, _| {
            scenes
                .iter()
                .fold(menu.action_context(action_context.clone()), |menu, path| {
                    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                    let label = file_name
                        .strip_suffix(SCENE_FILE_SUFFIX)
                        .unwrap_or(&file_name)
                        .to_owned();
                    menu.menu_with_icon(
                        label,
                        IconName::Page,
                        Box::new(ImportScene {
                            path: Some(path.clone()),
                        }),
                    )
                })
        },
    )
}
