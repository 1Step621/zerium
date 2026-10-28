//! Shared file selection for property and scene argument controls.
pub(in crate::ui) mod import;

use self::import::FileImportTarget;
use crate::{
    app::project::{
        ProjectActivity, ProjectOperation, ProjectSession, media::MediaMetadataController,
    },
    engine::input::{MediaError, prepare_import},
    ui::workspace::notifications::UiNotifications,
};
use ::ui::{
    Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
};
use gpui::{Context, Div, Entity, SharedString, Subscription, Task, Window, div, prelude::*};
use rust_i18n::t;
use std::path::{Path, PathBuf};
use zerium_core::{
    media::ImportedFile,
    property::{PropertyUi, PropertyValue, ScalarPropertyType},
    timeline::{EditContext, PropertyAddress, SceneId, TimelineEditor},
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
            } => {
                if editor.active_scene_id() != *scene_id {
                    return None;
                }
                (
                    editor
                        .scene(*scene_id)?
                        .argument(argument_id)?
                        .argument_schema()?,
                    None,
                    None,
                )
            }
            Self::Property(address) => {
                if !editor.is_item_selected(address.item_id) {
                    return None;
                }
                (
                    editor.property_schema(address)?,
                    address.element_id,
                    address.scalar_index,
                )
            }
        };
        (schema.scalar_type(element_id, scalar_index) == Some(&ScalarPropertyType::File)
            && schema.is_editable(scalar_index))
        .then(|| schema.configuration_ui(scalar_index))
    }
}

pub(crate) struct FilePickerOptions<'a> {
    pub(crate) path: Option<&'a Path>,
    pub(crate) directory: bool,
    pub(crate) selecting: bool,
    pub(crate) disabled: bool,
}

pub(crate) fn file_picker(
    input: &Entity<FileInputController>,
    key: SharedString,
    target: FileTarget,
    options: FilePickerOptions<'_>,
) -> Div {
    let FilePickerOptions {
        path,
        directory,
        selecting,
        disabled,
    } = options;
    let choose_label = if directory {
        t!("inspector.choose_folder")
    } else {
        t!("inspector.choose_file")
    }
    .to_string();
    let label = if let Some(path) = path {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned()
    } else {
        choose_label.clone()
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
                    .tooltip(choose_label)
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

enum SelectedFile {
    Property(PathBuf),
    Import(ImportedFile),
}

struct FileSelection {
    operation: ProjectOperation,
    target: FileTarget,
    context: EditContext,
    _task: Task<()>,
}

pub(crate) struct FileInputController {
    editor: Entity<TimelineEditor>,
    metadata: Entity<MediaMetadataController>,
    session: Entity<ProjectSession>,
    notifications: Entity<UiNotifications>,
    selection: Option<FileSelection>,
    _subscriptions: Vec<Subscription>,
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
        let session_subscription = cx.observe(&session, move |this, session, cx| {
            let next = session.read(cx).id();
            if next != session_id {
                session_id = next;
                this.cancel_selection(cx);
            }
        });
        let editor_subscription = cx.observe(&editor, |this, editor, cx| {
            if this.selection.as_ref().is_some_and(|selection| {
                !selection.context.is_current(editor.read(cx))
                    || selection.target.ui(editor.read(cx)).is_none()
            }) {
                this.cancel_selection(cx);
            }
        });
        Self {
            editor,
            metadata,
            session,
            notifications,
            selection: None,
            _subscriptions: vec![session_subscription, editor_subscription],
        }
    }

    pub(crate) fn is_selecting(&self) -> bool {
        self.selection.is_some()
    }

    fn cancel_selection(&mut self, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.take() else {
            return;
        };
        self.session.update(cx, |session, cx| {
            if session.finish(selection.operation) {
                cx.notify();
            }
        });
        cx.notify();
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
        let context = editor.edit_context();
        let directory = ui.selects_directory();
        let import = match &target {
            FileTarget::Property(address) => FileImportTarget::for_property(address, editor),
            FileTarget::Argument { .. } => None,
        };
        let plugins = editor.plugin_registry_arc();
        let mut dialog = rfd::AsyncFileDialog::new().set_parent(window).set_title(
            if directory {
                t!("inspector.choose_folder")
            } else {
                t!("edit.choose_file")
            }
            .to_string(),
        );
        // macOS merges filters instead of offering an unrestricted alternative.
        if cfg!(not(target_os = "macos")) && !ui.extensions().is_empty() {
            dialog = dialog
                .add_filter(t!("inspector.suggested_files").to_string(), ui.extensions())
                .add_filter(t!("inspector.all_files").to_string(), &["*"]);
        }
        let selection = async move {
            if directory {
                dialog.pick_folder().await
            } else {
                dialog.pick_file().await
            }
        };
        let session = self.session.clone();
        let operation = session.update(cx, |session, cx| {
            let operation = session.begin(ProjectActivity::SelectFile);
            cx.notify();
            operation
        });
        let task = cx.spawn(async move |controller, cx| {
            let result = match selection.await {
                Some(file) => {
                    let path = file.path().to_path_buf();
                    if let Some(import) = import {
                        cx.background_spawn(async move {
                            prepare_import(
                                path,
                                &plugins,
                                &import.plugin_id,
                                &import.item_id,
                                &import.property_id,
                            )
                            .map(|file| Some(SelectedFile::Import(file)))
                        })
                        .await
                    } else {
                        Ok(Some(SelectedFile::Property(path)))
                    }
                }
                None => Ok(None),
            };
            let _ = controller.update(cx, |this, cx| {
                this.finish_selection(operation, result, cx);
            });
        });
        self.selection = Some(FileSelection {
            operation,
            target,
            context,
            _task: task,
        });
        cx.notify();
    }

    fn finish_selection(
        &mut self,
        operation: ProjectOperation,
        result: Result<Option<SelectedFile>, MediaError>,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self
            .selection
            .take_if(|selection| selection.operation == operation)
        else {
            return;
        };
        let current = self.session.update(cx, |session, cx| {
            let current = session.finish(operation);
            if current {
                cx.notify();
            }
            current
        });
        if current
            && selection.context.is_current(self.editor.read(cx))
            && selection.target.ui(self.editor.read(cx)).is_some()
        {
            match result {
                Ok(Some(SelectedFile::Import(imported))) => {
                    if let FileTarget::Property(address) = selection.target {
                        let result = self.editor.update(cx, |editor, cx| {
                            let result = editor.import_item_file(address.item_id, imported);
                            if result.is_ok() {
                                cx.notify();
                            }
                            result
                        });
                        if let Err(error) = result {
                            self.notifications.update(cx, |notifications, cx| {
                                notifications.push(error.to_string(), cx)
                            });
                        }
                    }
                }
                Ok(Some(SelectedFile::Property(path))) => {
                    self.metadata.read(cx).retry(path.clone());
                    self.set_value(selection.target, Some(path), cx);
                }
                Ok(None) => {}
                Err(error) => self.notifications.update(cx, |notifications, cx| {
                    notifications.push(error.to_string(), cx)
                }),
            }
        }
        cx.notify();
    }

    fn set_value(&mut self, target: FileTarget, path: Option<PathBuf>, cx: &mut Context<Self>) {
        let result = self.editor.update(cx, |editor, cx| {
            let result = target.apply(editor, path);
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
}

impl FileTarget {
    fn apply(self, editor: &mut TimelineEditor, path: Option<PathBuf>) -> Result<bool, String> {
        if self.ui(editor).is_none() {
            return Ok(false);
        }
        let value = PropertyValue::File(path);
        match self {
            FileTarget::Argument { argument_id, .. } => editor
                .update_scene_argument_default(&argument_id, value)
                .map_err(|error| error.to_string()),
            FileTarget::Property(address) => editor
                .edit_property(&address, value)
                .map_err(|error| error.to_string()),
        }
    }
}
