mod control;
mod edit;
mod model;
mod numeric;
mod render;
mod rows;
mod scene_args;
mod state;
use numeric::NumericInput;

use std::collections::{HashMap, HashSet};

use ::ui::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _, ThemeColor,
    button::{Button, ButtonVariants as _},
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    input::{Input, InputEvent, InputState, NumberInput, NumberInputEvent, StepAction},
    menu::{PopupMenuItem, popup_menu::PopupMenuExt as _},
    popover::Popover,
    switch::Switch,
};
use gpui::{
    App, Context, CursorStyle, Div, DragMoveEvent, Empty, Entity, EntityId, FocusHandle,
    Focusable as _, MouseButton, MouseDownEvent, PathPromptOptions, Render, Rgba, SharedString,
    Subscription, Task, Window, div, prelude::*, px,
};

use crate::domain::media::{MediaAsset, MediaKind};
use crate::domain::plugin::{FileCapability, ItemSchema};
use crate::domain::property::{
    PropertyElement, PropertyElementId, PropertySchema, PropertyType, PropertyValue,
    PropertyValueType, ScalarPropertyType,
};
use crate::domain::timeline::{
    EffectInstance, EffectInstanceId, ItemId, SceneArgument, SceneArgumentPreset,
    SceneBindingOwner, SceneBindingTarget, SceneId, TimelineEditor, TimelineItem, TimelineTime,
    display_scene_expression,
};
use crate::engine::media::MediaReaderRegistry;
use crate::plugin::plugins;
use crate::ui::TimelineEditorEntityExt as _;
use crate::ui::animation_curve::{AnimationPresentation, AnimationSelection, AnimationTarget};
use crate::ui::inspector_path::InspectorPath;
use crate::ui::pane::pane_header;
use crate::ui::search_picker::{SearchPicker, SearchPickerEntry};
use crate::ui::session::{ProjectActivity, ProjectSession, ProjectSessionId, UiNotifications};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum ControlId {
    Property(InspectorPath),
    AnimationStop {
        property: InspectorPath,
        stop: usize,
    },
    Group(InspectorPath),
    EffectGroup(EffectInstanceId),
    SceneName(SceneId),
    SceneArgumentName {
        scene_id: SceneId,
        argument_id: String,
    },
    SceneArgumentExpression {
        scene_id: SceneId,
        argument_id: String,
    },
    SceneArgumentDefault {
        scene_id: SceneId,
        argument_id: String,
    },
    SceneArgumentSetting {
        scene_id: SceneId,
        argument_id: String,
        setting: SceneArgumentSetting,
    },
    SceneArgumentColor {
        scene_id: SceneId,
        argument_id: String,
    },
}

impl ControlId {
    pub(super) fn property(path: &InspectorPath) -> Self {
        Self::Property(path.clone())
    }

    pub(super) fn animation_stop(path: &InspectorPath, stop: usize) -> Self {
        Self::AnimationStop {
            property: path.clone(),
            stop,
        }
    }

    pub(super) fn group(path: &InspectorPath) -> Self {
        Self::Group(path.clone())
    }

    pub(super) fn scene_name(scene_id: SceneId) -> Self {
        Self::SceneName(scene_id)
    }

    pub(super) fn effect_group(effect_id: EffectInstanceId) -> Self {
        Self::EffectGroup(effect_id)
    }

    pub(super) fn scene_argument_name(scene_id: SceneId, argument_id: &str) -> Self {
        Self::SceneArgumentName {
            scene_id,
            argument_id: argument_id.to_owned(),
        }
    }

    pub(super) fn scene_argument_expression(scene_id: SceneId, argument_id: &str) -> Self {
        Self::SceneArgumentExpression {
            scene_id,
            argument_id: argument_id.to_owned(),
        }
    }

    pub(super) fn scene_argument_default(scene_id: SceneId, argument_id: &str) -> Self {
        Self::SceneArgumentDefault {
            scene_id,
            argument_id: argument_id.to_owned(),
        }
    }

    pub(super) fn scene_argument_setting(
        scene_id: SceneId,
        argument_id: &str,
        setting: SceneArgumentSetting,
    ) -> Self {
        Self::SceneArgumentSetting {
            scene_id,
            argument_id: argument_id.to_owned(),
            setting,
        }
    }

    pub(super) fn scene_argument_color(scene_id: SceneId, argument_id: &str) -> Self {
        Self::SceneArgumentColor {
            scene_id,
            argument_id: argument_id.to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PropertyTarget {
    pub key: InspectorPath,
    pub property_id: String,
    pub effect_id: Option<EffectInstanceId>,
    pub path: InspectorPath,
}

impl PropertyTarget {
    pub(super) fn animation_enabled(&self, item: &TimelineItem) -> bool {
        item.animation_track(
            self.effect_id,
            &self.property_id,
            self.path.element_id(),
            self.path.scalar_index(),
        )
        .is_some()
    }

    pub(super) fn animation_target(&self, item: &TimelineItem) -> AnimationTarget {
        AnimationTarget {
            item_id: item.id,
            effect_id: self.effect_id,
            property_id: self.property_id.clone(),
            element_id: self.path.element_id(),
            scalar_index: self.path.scalar_index(),
            property: self.key.clone(),
        }
    }
}

#[derive(Clone)]
pub(super) struct SceneArgumentOption {
    pub scene_id: SceneId,
    pub id: String,
    pub label: String,
    pub schema: PropertySchema,
    pub binding_count: usize,
    pub bindings: Vec<SceneBindingTarget>,
    pub expression: Option<String>,
    pub referenced_by_expression: bool,
}

#[derive(Clone)]
pub(super) struct SceneFieldBinding {
    pub target: SceneBindingTarget,
    pub connected: Option<(String, String)>,
    pub compatible: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum SceneArgumentSetting {
    Default,
    Min,
    Max,
}

#[derive(Clone)]
pub(super) struct NumberAnimationSource {
    pub property_id: String,
    pub element_id: Option<PropertyElementId>,
    pub scalar_index: Option<usize>,
    pub value_factor: f64,
}

#[derive(Clone, Copy)]
pub(super) struct AspectRatioLockState {
    pub value: bool,
    pub mixed: bool,
    pub multiple: bool,
    pub disabled_by_scene_size_argument: bool,
}

impl AspectRatioLockState {
    pub(super) fn checked(self) -> bool {
        self.value && !self.mixed
    }
}

#[derive(Clone)]
pub(super) struct PropertyBinding {
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
    pub value_factor: f64,
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
            value_factor: stop.value_factor,
        }
    }
}

#[derive(Clone)]
pub(super) struct PropertyValueDrag {
    pub inspector_id: EntityId,
    pub input_id: ControlId,
}

#[derive(Clone)]
pub(super) struct PropertyValueDragOrigin {
    pub target: PropertyTarget,
    pub input_id: ControlId,
    pub animation_stop: Option<AnimationStopBinding>,
    pub start_x: f32,
    pub start_value: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub sensitivity: f64,
}

#[derive(Clone)]
pub(super) struct SceneArgumentValueDrag {
    pub inspector_id: EntityId,
    pub scene_id: SceneId,
    pub argument_id: String,
    pub setting: SceneArgumentSetting,
}

impl Render for SceneArgumentValueDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

pub(super) struct SceneArgumentValueDragOrigin {
    pub scene_id: SceneId,
    pub argument_id: String,
    pub setting: SceneArgumentSetting,
    pub start_x: f32,
    pub start_value: f64,
    number: numeric::NumericInput,
    pub sensitivity: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct InspectorInputStructure {
    pub item_id: Option<ItemId>,
    pub effect_ids: Vec<EffectInstanceId>,
    pub array_lengths: Vec<(Option<u64>, String, usize)>,
    pub item_scene_arguments: Vec<String>,
    pub active_scene: Option<SceneId>,
    pub active_scene_arguments: Vec<String>,
}

impl Render for PropertyValueDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

pub(crate) struct PropertyInspector {
    pub(super) editor: Entity<TimelineEditor>,
    pub(super) animation_selection: Entity<AnimationSelection>,
    pub(super) media_readers: std::sync::Arc<MediaReaderRegistry>,
    pub(super) session: Entity<ProjectSession>,
    pub(super) session_id: ProjectSessionId,
    pub(super) notifications: Entity<UiNotifications>,
    pub(super) focus_handle: FocusHandle,
    store: state::ControlStore,
    pub(super) font_names: Vec<String>,
    pub(super) expanded_scene_arguments: HashSet<(SceneId, String)>,
    pub(super) loading_file: bool,
    pub(super) file_error: Option<SharedString>,
    pub(super) _file_task: Task<()>,
    pub(super) _editor_subscription: Subscription,
    pub(super) _session_subscription: Subscription,
}

impl PropertyInspector {
    pub(super) const PROPERTY_LABEL_WIDTH: f32 = 64.;
    pub(super) const ANIMATION_STOP_INPUT_MIN_WIDTH: f32 = 112.;
    pub(super) const DRAG_RANGE_PIXELS: f64 = 200.;
    pub(super) const MIN_STEP_MULTIPLIER: f64 = 0.1;
    pub(super) const MAX_STEP_MULTIPLIER: f64 = 2.;

    pub(super) fn property_label_column(label: impl Into<SharedString>) -> Div {
        div()
            .w(px(Self::PROPERTY_LABEL_WIDTH))
            .h(px(24.))
            .flex_none()
            .flex()
            .items_center()
            .text_sm()
            .child(label.into())
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
        let session_subscription = cx.observe(&session, |this, _, cx| {
            let session_id = this.session.read(cx).id();
            if session_id == this.session_id {
                return;
            }
            this.session_id = session_id;
            this._file_task = Task::ready(());
            this.loading_file = false;
            this.file_error = None;
            this.reset_input_state();
            cx.notify();
        });

        let mut inspector = Self {
            editor,
            animation_selection,
            media_readers,
            session,
            session_id,
            notifications,
            focus_handle: cx.focus_handle(),
            store: state::ControlStore::default(),
            font_names: {
                let mut names = cx.text_system().all_font_names();
                names.sort_unstable();
                names.dedup();
                names
            },
            expanded_scene_arguments: HashSet::new(),
            loading_file: false,
            file_error: None,
            _file_task: Task::ready(()),
            _editor_subscription: editor_subscription,
            _session_subscription: session_subscription,
        };
        let editor = inspector.editor.clone();
        inspector.sync_from_editor(&editor, window, cx);
        inspector
    }
}
