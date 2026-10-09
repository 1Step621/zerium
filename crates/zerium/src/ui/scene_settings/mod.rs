//! Scene schema authoring lives outside the plugin property inspector.

mod edit;
mod number_drag;
mod numeric;
mod render;
mod state;

use super::{
    number_input::NumberValueDrag,
    numeric_property::{NumericDrag, NumericInput},
};
use ::ui::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, ThemeColor,
    button::{Button, ButtonVariants as _},
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    input::{Input, InputEvent, InputState, NumberInput, NumberInputEvent, StepAction},
    menu::{PopupMenuItem, popup_menu::PopupMenuExt as _},
    switch::Switch,
};
use gpui::{
    App, Context, CursorStyle, Div, DragMoveEvent, Entity, FocusHandle, MouseButton,
    MouseDownEvent, Render, Rgba, ScrollHandle, SharedString, Subscription, Window, div,
    prelude::*, px,
};
use rust_i18n::t;
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};
use zerium_core::{
    property::{PropertySchema, PropertyValue},
    timeline::{SceneArgumentPreset, SceneId, TimelineEditor},
};

use crate::ui::file_input::{FileInputController, FileTarget};
use crate::ui::session::UiNotifications;
use number_drag::NumberDragOrigin;
use numeric::{NumericSetting, NumericSettingDraft};
use state::SettingsStore;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ControlId {
    Name(SceneId),
    ArgumentName {
        scene_id: SceneId,
        argument_id: String,
    },
    ArgumentDefault {
        scene_id: SceneId,
        argument_id: String,
    },
    ArgumentSetting {
        scene_id: SceneId,
        argument_id: String,
        setting: NumericSetting,
    },
    ArgumentColor {
        scene_id: SceneId,
        argument_id: String,
    },
}

impl ControlId {
    fn scene_name(scene_id: SceneId) -> Self {
        Self::Name(scene_id)
    }

    fn scene_argument_name(scene_id: SceneId, argument_id: &str) -> Self {
        Self::ArgumentName {
            scene_id,
            argument_id: argument_id.to_owned(),
        }
    }

    fn scene_argument_default(scene_id: SceneId, argument_id: &str) -> Self {
        Self::ArgumentDefault {
            scene_id,
            argument_id: argument_id.to_owned(),
        }
    }

    fn scene_argument_setting(
        scene_id: SceneId,
        argument_id: &str,
        setting: NumericSetting,
    ) -> Self {
        Self::ArgumentSetting {
            scene_id,
            argument_id: argument_id.to_owned(),
            setting,
        }
    }

    fn scene_argument_color(scene_id: SceneId, argument_id: &str) -> Self {
        Self::ArgumentColor {
            scene_id,
            argument_id: argument_id.to_owned(),
        }
    }
}

pub(crate) struct SceneSettings {
    editor: Entity<TimelineEditor>,
    notifications: Entity<UiNotifications>,
    file_input: Entity<FileInputController>,
    scene_id: Option<SceneId>,
    store: SettingsStore,
    focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl SceneSettings {
    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        notifications: Entity<UiNotifications>,
        file_input: Entity<FileInputController>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe_in(&editor, window, |this, _, window, cx| {
            this.sync(window, cx);
        });
        let mut this = Self {
            scene_id: None,
            editor,
            notifications,
            _subscriptions: vec![
                subscription,
                cx.observe(&file_input, |_, _, cx| cx.notify()),
            ],
            file_input,
            store: SettingsStore::default(),
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
        };
        this.sync(window, cx);
        this
    }

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let scene_id = self.editor.read(cx).active_scene_id();
        if self.scene_id != scene_id {
            self.scene_id = scene_id;
            self.store = SettingsStore::default();
            self.scroll_handle = ScrollHandle::new();
        }
        self.sync_inputs(window, cx);
        cx.notify();
    }

    pub(crate) fn reveal_argument(
        &mut self,
        scene_id: SceneId,
        argument_id: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.scene_id != Some(scene_id) {
            return false;
        }
        let Some(index) = self.editor.read(cx).scene(scene_id).and_then(|scene| {
            scene
                .arguments
                .iter()
                .position(|argument| argument.schema.id() == argument_id)
        }) else {
            return false;
        };
        self.scroll_handle.scroll_to_top_of_item(index + 2);
        cx.notify();
        true
    }
}
