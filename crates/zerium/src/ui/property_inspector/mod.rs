mod control;
mod edit;
mod number_drag;
mod path;
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
    menu::{PopupMenuItem, popup_menu::PopupMenuExt as _},
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
use path::InspectorPath;
use zerium_core::plugin::ItemSchema;
use zerium_core::property::{
    PropertyElement, PropertyElementId, PropertySchema, PropertyType, PropertyValue,
    PropertyValueType, ScalarPropertyType,
};
use zerium_core::timeline::{
    AnimationStopEdit, EditScope, EffectInstance, EffectInstanceId, ItemId, PropertyAddress,
    SceneBindingOwner, SceneBindingTarget, SceneId, TimelineEditor, TimelineItem, TimelineTime,
};

pub(super) type EffectPickerTarget = (String, String);

pub(crate) struct SceneArgumentRequested {
    pub scene_id: SceneId,
    pub argument_id: String,
}

impl gpui::EventEmitter<SceneArgumentRequested> for PropertyInspector {}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ControlId {
    Property(InspectorPath),
    AnimationStop {
        property: InspectorPath,
        stop: usize,
    },
    Group(InspectorPath),
    EffectGroup(EffectInstanceId),
}

impl ControlId {
    fn property(path: &InspectorPath) -> Self {
        Self::Property(path.clone())
    }

    fn animation_stop(path: &InspectorPath, stop: usize) -> Self {
        Self::AnimationStop {
            property: path.clone(),
            stop,
        }
    }

    fn group(path: &InspectorPath) -> Self {
        Self::Group(path.clone())
    }

    fn effect_group(effect_id: EffectInstanceId) -> Self {
        Self::EffectGroup(effect_id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PropertyTarget {
    key: InspectorPath,
    property_id: String,
    effect_id: Option<EffectInstanceId>,
    element_id: Option<PropertyElementId>,
    scalar_index: Option<usize>,
}

impl PropertyTarget {
    fn value<'a>(&self, item: &'a TimelineItem) -> Option<&'a PropertyValue> {
        self.address(item.id).value(item)
    }

    fn selected_address(
        &self,
        source: &TimelineItem,
        item: &TimelineItem,
    ) -> Option<PropertyAddress> {
        self.address(source.id).on_item(source, item)
    }

    fn selected_value(&self, source: &TimelineItem, item: &TimelineItem) -> Option<PropertyValue> {
        self.selected_address(source, item)?.value(item).cloned()
    }

    fn address(&self, item_id: ItemId) -> PropertyAddress {
        PropertyAddress {
            item_id,
            effect_id: self.effect_id,
            property_id: self.property_id.clone(),
            element_id: self.element_id,
            scalar_index: self.scalar_index,
        }
    }

    fn animation_enabled(&self, source: &TimelineItem, items: &[TimelineItem]) -> bool {
        items.iter().any(|item| {
            self.selected_address(source, item).is_some_and(|address| {
                item.animation_track(
                    address.effect_id,
                    &address.property_id,
                    address.element_id,
                    address.scalar_index,
                )
                .is_some()
            })
        })
    }
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

#[derive(Clone)]
struct PropertyBinding {
    pub item_id: ItemId,
    pub target: PropertyTarget,
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
    scope: EditScope,
    store: state::ControlStore,
    pub(super) font_names: Vec<String>,
    pub(super) effect_picker: Option<Entity<SearchPicker<EffectPickerTarget>>>,
    _subscriptions: Vec<Subscription>,
}

impl PropertyInspector {
    pub(super) const PROPERTY_LABEL_WIDTH: f32 = 90.;
    pub(super) const SCALAR_LABEL_WIDTH: f32 = 40.;
    pub(super) const DRAG_RANGE_PIXELS: f64 = 200.;
    pub(super) const MIN_STEP_MULTIPLIER: f64 = 0.1;
    pub(super) const MAX_STEP_MULTIPLIER: f64 = 2.;

    pub(super) fn property_label_column(label: impl Into<SharedString>) -> Div {
        div()
            .w(px(Self::PROPERTY_LABEL_WIDTH))
            .min_h(px(24.))
            .min_w_0()
            .flex_none()
            .flex()
            .items_center()
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_sm()
                    .whitespace_normal()
                    .child(label.into()),
            )
    }

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
                    this.scope = EditScope::Selection;
                    this.effect_picker = None;
                    this.reset_input_state();
                }
                this.sync_from_editor(&editor, window, cx);
            });
        let animation_selection_subscription =
            cx.observe(&animation_selection, |_, _, cx| cx.notify());
        let session_subscription = cx.observe(&session, |this, _, cx| {
            let session_id = this.session.read(cx).id();
            if session_id == this.session_id {
                return;
            }
            this.session_id = session_id;
            this.scope = EditScope::Selection;
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
            scope: EditScope::Selection,
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
        let editor = inspector.editor.clone();
        inspector.sync_from_editor(&editor, window, cx);
        inspector
    }
}
