mod control;
mod edit;
mod number_drag;
mod path;
mod render;
mod rows;
mod state;
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
    Focusable as _, MouseButton, MouseDownEvent, PathPromptOptions, Render, Rgba, ScrollHandle,
    SharedString, Subscription, Task, Window, div, prelude::*, px,
};

use crate::engine::media::MediaReaderRegistry;
use crate::project_session::{ProjectActivity, ProjectSession, ProjectSessionId};
use crate::ui::TimelineEditorEntityExt as _;
use crate::ui::animation_curve::AnimationSelection;
use crate::ui::pane::pane_header;
use crate::ui::search_picker::{SearchPicker, SearchPickerEntry};
use crate::ui::session::UiNotifications;
use number_drag::PropertyValueDragOrigin;
use path::InspectorPath;
use zerium_core::plugin::ItemSchema;
use zerium_core::property::{
    PropertyElement, PropertyElementId, PropertyPath, PropertySchema, PropertyType, PropertyValue,
    PropertyValueType, ScalarPropertyType,
};
use zerium_core::timeline::{
    EffectInstance, EffectInstanceId, ItemId, PropertyAddress, SceneArgument, SceneBindingOwner,
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

    fn selected_value(&self, primary: &TimelineItem, item: &TimelineItem) -> Option<PropertyValue> {
        let mut target = self.clone();
        if let Some(id) = self.effect_id {
            let index = primary.effects.iter().position(|effect| effect.id == id)?;
            target.effect_id = Some(item.effects.get(index)?.id);
        }
        target.value(item).cloned()
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

    fn matches_address(&self, item_id: ItemId, address: &PropertyAddress) -> bool {
        address.item_id == item_id
            && address.effect_id == self.effect_id
            && address.property_id == self.property_id
            && address.element_id == self.element_id
            && address.scalar_index == self.scalar_index
    }

    fn animation_enabled(&self, item: &TimelineItem) -> bool {
        item.animation_track(
            self.effect_id,
            &self.property_id,
            self.element_id,
            self.scalar_index,
        )
        .is_some()
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

#[derive(Clone)]
pub(super) struct AnimationStopBinding {
    pub item_id: ItemId,
    pub effect_id: Option<EffectInstanceId>,
    pub property_id: String,
    pub element_id: Option<PropertyElementId>,
    pub scalar_index: Option<usize>,
    pub stop: usize,
}

impl AnimationStopBinding {
    fn new(
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        stop: &control::AnimationStopControl,
    ) -> Self {
        Self {
            item_id,
            effect_id,
            property_id: stop.property_id.clone(),
            element_id: stop.element_id,
            scalar_index: stop.scalar_index,
            stop: stop.index,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PropertySource {
    Plugin,
    SceneArguments,
}

pub(crate) struct PropertyInspector {
    source: PropertySource,
    pub(super) editor: Entity<TimelineEditor>,
    pub(super) animation_selection: Entity<AnimationSelection>,
    pub(super) media_readers: std::sync::Arc<MediaReaderRegistry>,
    pub(super) session: Entity<ProjectSession>,
    pub(super) session_id: ProjectSessionId,
    pub(super) notifications: Entity<UiNotifications>,
    pub(super) focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
    store: state::ControlStore,
    pub(super) font_names: Vec<String>,
    pub(super) loading_file: bool,
    pub(super) _file_task: Task<()>,
    pub(super) effect_picker: Option<Entity<SearchPicker<EffectPickerTarget>>>,
    pub(super) _editor_subscription: Subscription,
    pub(super) _animation_selection_subscription: Subscription,
    pub(super) _session_subscription: Subscription,
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
        media_readers: std::sync::Arc<MediaReaderRegistry>,
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let session_id = session.read(cx).id();
        let editor_subscription = cx.observe_in(&editor, window, |this, editor, window, cx| {
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
            this._file_task = Task::ready(());
            this.loading_file = false;
            this.reset_input_state();
            cx.notify();
        });

        let mut inspector = Self {
            source: PropertySource::Plugin,
            editor,
            animation_selection,
            media_readers,
            session,
            session_id,
            notifications,
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
            store: state::ControlStore::default(),
            font_names: {
                let mut names = cx.text_system().all_font_names();
                names.sort_unstable();
                names.dedup();
                names
            },
            loading_file: false,
            _file_task: Task::ready(()),
            effect_picker: None,
            _editor_subscription: editor_subscription,
            _animation_selection_subscription: animation_selection_subscription,
            _session_subscription: session_subscription,
        };
        let editor = inspector.editor.clone();
        inspector.sync_from_editor(&editor, window, cx);
        inspector
    }
}
