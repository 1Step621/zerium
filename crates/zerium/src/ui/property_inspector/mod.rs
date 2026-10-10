mod control;
mod copy;
mod edit;
mod number_drag;
mod render;
mod rows;
mod state;
mod target;

use crate::ui::numeric_property::{NumericInput, NumericInputSpec, numeric_input_spec};

use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use ::ui::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _, ThemeColor,
    button::{Button, ButtonVariants as _},
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    input::{Input, InputEvent, InputState, NumberInput, NumberInputEvent},
    menu::{PopupMenuItem, context_menu::ContextMenuExt as _, popup_menu::PopupMenuExt as _},
    popover::Popover,
    switch::Switch,
};
use gpui::{
    App, Context, CursorStyle, DismissEvent, Div, DragMoveEvent, Entity, FocusHandle,
    Focusable as _, MouseButton, MouseDownEvent, Render, Rgba, ScrollHandle, SharedString,
    Subscription, Window, div, prelude::*, px,
};

use crate::project_session::{ProjectSession, ProjectSessionId};
use crate::ui::TimelineEditorEntityExt as _;
use crate::ui::animation_curve::AnimationSelection;
use crate::ui::file_input::FileInputController;
use crate::ui::pane::pane_header;
use crate::ui::search_picker::{SearchPicker, SearchPickerEntry};
use crate::ui::session::UiNotifications;
use number_drag::PropertyValueDragOrigin;
use zerium_core::property::{
    PropertyDefinition, PropertyElement, PropertyElementId, PropertySchema, PropertyValue,
    ScalarPropertyType, ValueSchema,
};
use zerium_core::timeline::{
    AnimationStopEdit, EffectInstanceId, ItemId, PropertyAddress, SceneBindingOwner,
    SceneBindingTarget, SceneId, TimelineEditor, TimelineItem, TimelineTime,
};

pub(super) type EffectPickerTarget = (String, String);

pub(crate) struct SceneArgumentRequested {
    pub scene_id: SceneId,
    pub argument_id: String,
}

impl gpui::EventEmitter<SceneArgumentRequested> for PropertyInspector {}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ControlId {
    Property(PropertyAddress),
    AnimationStop {
        property: PropertyAddress,
        stop: usize,
    },
    Group(PropertyAddress),
    EffectGroup(EffectInstanceId),
}

#[derive(Clone)]
pub(super) struct SceneArgumentOption {
    pub id: String,
    pub label: String,
    pub schema: PropertySchema,
    pub bindings: Vec<SceneBindingTarget>,
}

#[derive(Clone)]
pub(super) struct SceneFieldBinding {
    pub target: SceneBindingTarget,
    pub connected: Option<(String, String)>,
    pub compatible: Vec<(String, String)>,
}

pub(crate) struct PropertyInspector {
    pub(super) editor: Entity<TimelineEditor>,
    pub(super) animation_selection: Entity<AnimationSelection>,
    pub(super) file_input: Entity<FileInputController>,
    pub(super) session: Entity<ProjectSession>,
    pub(super) session_id: ProjectSessionId,
    pub(super) notifications: Entity<UiNotifications>,
    pub(super) focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
    item_id: Option<ItemId>,
    store: state::ControlStore,
    pub(super) font_names: Vec<String>,
    pub(super) effect_picker: Option<Entity<SearchPicker<EffectPickerTarget>>>,
    _subscriptions: Vec<Subscription>,
}

impl PropertyInspector {
    pub(super) const DRAG_RANGE_PIXELS: f64 = 200.;
    pub(super) const MIN_STEP_MULTIPLIER: f64 = 0.1;
    pub(super) const MAX_STEP_MULTIPLIER: f64 = 2.;

    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        animation_selection: Entity<AnimationSelection>,
        file_input: Entity<FileInputController>,
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let session_id = session.read(cx).id();
        let mut document = (
            editor.read(cx).snapshot().project_id(),
            editor.read(cx).active_scene_id(),
        );
        let editor_subscription =
            cx.observe_in(&editor, window, move |this, editor, window, cx| {
                let next = (
                    editor.read(cx).snapshot().project_id(),
                    editor.read(cx).active_scene_id(),
                );
                if document != next {
                    document = next;
                    this.item_id = None;
                    this.effect_picker = None;
                    this.reset_input_state();
                }
                this.sync_from_editor(window, cx);
            });
        let animation_selection_subscription =
            cx.observe(&animation_selection, |_, _, cx| cx.notify());
        let session_subscription = cx.observe(&session, |this, _, cx| {
            let session_id = this.session.read(cx).id();
            if session_id == this.session_id {
                return;
            }
            this.session_id = session_id;
            this.item_id = None;
            this.reset_input_state();
            cx.notify();
        });

        let mut inspector = Self {
            editor,
            animation_selection,
            file_input: file_input.clone(),
            session,
            session_id,
            notifications,
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
            item_id: None,
            store: state::ControlStore::default(),
            font_names: {
                let mut names = cx.text_system().all_font_names();
                names.sort_unstable();
                names.dedup();
                names
            },
            effect_picker: None,
            _subscriptions: vec![
                editor_subscription,
                animation_selection_subscription,
                session_subscription,
                cx.observe(&file_input, |_, _, cx| cx.notify()),
            ],
        };
        inspector.sync_from_editor(window, cx);
        inspector
    }
}
