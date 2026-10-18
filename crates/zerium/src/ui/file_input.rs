//! Shared file selection for property and scene argument controls.

use super::notifications::UiNotifications;
use crate::{
    app::media_metadata::MediaMetadataController,
    project_session::{ProjectActivity, ProjectOperation, ProjectSession},
};
use ::ui::{
    Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
};
use gpui::{Context, Div, Entity, SharedString, Subscription, Task, Window, div, prelude::*};
use rust_i18n::t;
use std::path::{Path, PathBuf};
use zerium_core::{
    property::{PropertyUi, PropertyValue, ScalarPropertyType},
    timeline::{PropertyAddress, SceneId, TimelineEditor},
};

#[derive(Clone)]
pub(crate) enum FileTarget {
    Property(PropertyAddress),
    Argument {
        scene_id: SceneId,
        argument_id: String,
    },
}

impl FileTarget {
    fn ui<'a>(&self, editor: &'a TimelineEditor) -> Option<&'a PropertyUi> {
        let (schema, element_id, scalar_index) = match self {
            Self::Argument {
                scene_id,
                argument_id,
            } => (
                &editor.scene(*scene_id)?.argument(argument_id)?.schema,
                None,
                None,
            ),
            Self::Property(address) => (
                editor.property_schema(address)?,
                address.element_id,
                address.scalar_index,
            ),
        };
        (schema.scalar_type(element_id, scalar_index) == Some(&ScalarPropertyType::File)
            && schema.is_editable(scalar_index))
        .then(|| schema.configuration_ui(scalar_index))
    }
}

pub(crate) fn file_picker(
    input: &Entity<FileInputController>,
    key: SharedString,
    target: FileTarget,
    path: Option<&Path>,
    selecting: bool,
    disabled: bool,
) -> Div {
    let label = if let Some(path) = path {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned()
    } else {
        t!("inspector.choose_file").to_string()
    };
    let choose_input = input.clone();
    let choose_target = target.clone();
    let clear_input = input.clone();

    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap_1()
        .child(
            div().min_w_0().flex_1().child(
                Button::new(SharedString::from(format!("select-{key}")))
                    .small()
                    .compact()
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .child(div().min_w_0().truncate().child(label))
                    .disabled(disabled || selecting)
                    .tooltip(t!("inspector.choose_file").to_string())
                    .when_some(path, |button, path| {
                        button.tooltip(path.display().to_string())
                    })
                    .on_click(move |_, window, cx| {
                        choose_input.update(cx, |input, cx| {
                            input.choose(choose_target.clone(), window, cx)
                        });
                    }),
            ),
        )
        .when(path.is_some(), |row| {
            row.child(
                div().flex_none().child(
                    Button::new(SharedString::from(format!("clear-{key}")))
                        .small()
                        .compact()
                        .ghost()
                        .icon(IconName::Xmark)
                        .tooltip(t!("inspector.clear_file").to_string())
                        .disabled(disabled || selecting)
                        .on_click(move |_, _, cx| {
                            clear_input
                                .update(cx, |input, cx| input.set_value(target.clone(), None, cx))
                        }),
                ),
            )
        })
}

struct FileSelection {
    operation: ProjectOperation,
    _task: Task<()>,
}

pub(crate) struct FileInputController {
    editor: Entity<TimelineEditor>,
    metadata: Entity<MediaMetadataController>,
    session: Entity<ProjectSession>,
    notifications: Entity<UiNotifications>,
    selection: Option<FileSelection>,
    _session_subscription: Subscription,
}

impl FileInputController {
    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        metadata: Entity<MediaMetadataController>,
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut session_id = session.read(cx).id();
        let subscription = cx.observe(&session, move |this, session, cx| {
            let next = session.read(cx).id();
            if next != session_id {
                session_id = next;
                this.selection = None;
                cx.notify();
            }
        });
        Self {
            editor,
            metadata,
            session,
            notifications,
            selection: None,
            _session_subscription: subscription,
        }
    }

    pub(crate) fn is_selecting(&self) -> bool {
        self.selection.is_some()
    }

    pub(crate) fn choose(
        &mut self,
        target: FileTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_selecting() {
            return;
        }
        let editor = self.editor.read(cx);
        let Some(ui) = target.ui(editor) else {
            return;
        };
        let scene_id = editor.active_scene_id();
        let mut dialog = rfd::AsyncFileDialog::new()
            .set_parent(window)
            .set_title(t!("edit.choose_file").to_string());
        // macOS merges filters instead of offering an unrestricted alternative.
        if cfg!(not(target_os = "macos")) && !ui.extensions().is_empty() {
            dialog = dialog
                .add_filter(t!("inspector.suggested_files").to_string(), ui.extensions())
                .add_filter(t!("inspector.all_files").to_string(), &["*"]);
        }
        let selection = dialog.pick_file();
        let session = self.session.clone();
        let operation = session.update(cx, |session, cx| {
            let operation = session.begin(ProjectActivity::SelectFile);
            cx.notify();
            operation
        });
        let task = cx.spawn(async move |controller, cx| {
            let selected = selection.await.map(|file| file.path().to_path_buf());
            let _ = controller.update(cx, |this, cx| {
                this.finish_selection(operation, scene_id, target, selected, cx);
            });
        });
        self.selection = Some(FileSelection {
            operation,
            _task: task,
        });
        cx.notify();
    }

    fn finish_selection(
        &mut self,
        operation: ProjectOperation,
        scene_id: Option<SceneId>,
        target: FileTarget,
        selected: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self
            .selection
            .as_ref()
            .is_none_or(|selection| selection.operation != operation)
        {
            return;
        }
        self.selection = None;
        let current = self.session.update(cx, |session, cx| {
            let current = session.finish(operation);
            if current {
                cx.notify();
            }
            current
        });
        if current
            && self.editor.read(cx).active_scene_id() == scene_id
            && target.ui(self.editor.read(cx)).is_some()
            && let Some(path) = selected
        {
            self.metadata.read(cx).retry(path.clone());
            self.set_value(target, Some(path), cx);
        }
        cx.notify();
    }

    fn set_value(&mut self, target: FileTarget, path: Option<PathBuf>, cx: &mut Context<Self>) {
        let result = self.editor.update(cx, |editor, cx| {
            let result = Self::apply(editor, target, path);
            if result == Ok(true) {
                cx.notify();
            }
            result
        });
        if let Err(error) = result {
            self.notifications
                .update(cx, |notifs, cx| notifs.push(error, cx));
        }
    }

    fn apply(
        editor: &mut TimelineEditor,
        target: FileTarget,
        path: Option<PathBuf>,
    ) -> Result<bool, String> {
        let value = PropertyValue::File(path);
        match target {
            FileTarget::Argument {
                scene_id,
                argument_id,
            } => {
                if editor.active_scene_id() != Some(scene_id) {
                    return Err("Scene changed".into());
                }
                editor
                    .update_scene_argument_default(&argument_id, value)
                    .map_err(|error| error.to_string())
            }
            FileTarget::Property(address) => {
                if !editor.is_item_selected(address.item_id) {
                    return Err("Selection changed".into());
                }
                editor
                    .edit_property(&address, value)
                    .map_err(|error| error.to_string())
            }
        }
    }
}
