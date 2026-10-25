mod control;
mod copy;
mod editing;
mod number_drag;
mod render;
mod rows;
mod state;
mod target;

use self::{
    editing::{edit_array, push_element},
    number_drag::PropertyValueDrag,
};

use crate::{
    app::project::{ProjectSession, ProjectSessionId},
    ui::{
        TimelineEditorEntityExt as _,
        components::{
            input::{
                file::FileInputController,
                number::value::{NumericInputSpec, numeric_input_spec},
            },
            pane::pane_header,
            search_picker::{SearchPicker, SearchPickerEntry},
        },
        workspace::notifications::UiNotifications,
    },
};

use std::collections::HashSet;

use ::ui::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    color_picker::ColorPickerEvent,
    input::{InputEvent, InputState, NumberInputEvent},
    menu::{PopupMenuItem, context_menu::ContextMenuExt as _, popup_menu::PopupMenuExt as _},
    popover::Popover,
};
use gpui::{
    App, Context, CursorStyle, DismissEvent, Div, DragMoveEvent, Entity, FocusHandle,
    Focusable as _, MouseButton, Render, Rgba, ScrollHandle, SharedString, Subscription, Window,
    div, prelude::*, px,
};

use super::animation_curve::AnimationSelection;
use zerium_core::{
    property::{
        PropertyDefinition, PropertyElementId, PropertySchema, PropertyValue, ScalarPropertyType,
        ValueSchema,
    },
    timeline::{
        AnimationValuePlan, EffectInstanceId, EvaluatedItem, ItemId, PropertyAddress,
        SceneBindingOwner, SceneBindingTarget, SceneVariable, TimelineEditor, TimelineItem,
        TimelineTime, ValueEdit, ValueEditTarget,
    },
};

pub(in crate::ui) type EffectPickerTarget = (String, String);

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
pub(in crate::ui) struct SceneFieldBinding {
    pub target: SceneBindingTarget,
    pub connected: Option<(String, String)>,
    pub compatible: Vec<(String, String)>,
}

pub(crate) struct PropertyInspector {
    pub(in crate::ui) editor: Entity<TimelineEditor>,
    pub(in crate::ui) animation_selection: Entity<AnimationSelection>,
    pub(in crate::ui) file_input: Entity<FileInputController>,
    pub(in crate::ui) session: Entity<ProjectSession>,
    pub(in crate::ui) session_id: ProjectSessionId,
    pub(in crate::ui) notifications: Entity<UiNotifications>,
    pub(in crate::ui) focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
    item_id: Option<ItemId>,
    store: state::ControlStore,
    pub(in crate::ui) font_names: Vec<String>,
    pub(in crate::ui) effect_picker: Option<Entity<SearchPicker<EffectPickerTarget>>>,
    _subscriptions: Vec<Subscription>,
}

impl PropertyInspector {
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
        let mut document = editor.read(cx).active_scene_id();
        let editor_subscription =
            cx.observe_in(&editor, window, move |this, editor, window, cx| {
                let next = editor.read(cx).active_scene_id();
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
