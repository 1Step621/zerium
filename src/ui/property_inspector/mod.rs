mod animation;
mod array;
mod choice;
mod color;
mod effects;
mod fields;
mod inputs;
mod model;
mod number;
mod numeric;
mod render;
mod scalar;
mod scene;
use numeric::NumericInput;
mod scene_view;
mod string;
mod view;

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
    Context, CursorStyle, Div, DragMoveEvent, Empty, Entity, EntityId, FocusHandle, MouseButton,
    MouseDownEvent, PathPromptOptions, Render, Rgba, SharedString, Subscription, Task, Window, div,
    prelude::*, px,
};

use crate::domain::animation::{AnimationChannel, ParameterAnimationAddress};
use crate::domain::media::{MediaAsset, MediaKind};
use crate::domain::parameter::{
    ParameterSchema, ParameterType, ParameterValue, ParameterValueType, ScalarParameterType,
};
use crate::domain::plugin::{FileCapability, ItemSchema};
use crate::domain::timeline::{
    EffectInstance, EffectInstanceId, ItemId, SceneArgument, SceneArgumentPreset,
    SceneBindingOwner, SceneBindingTarget, SceneBindingValuePath, SceneId, TimelineEditor,
    TimelineItem, display_scene_expression,
};
use crate::engine::media::MediaReaderRegistry;
use crate::plugin_catalog::plugins;
use crate::ui::TimelineEditorEntityExt as _;
use crate::ui::animation_curve::{AnimationPresentation, AnimationSelection, AnimationTarget};
use crate::ui::pane::pane_header;
use crate::ui::property::PropertyPath;
use crate::ui::search_picker::{SearchPicker, SearchPickerEntry};
use crate::ui::session::{ProjectActivity, ProjectSession, ProjectSessionId, UiNotifications};

#[derive(Clone, Debug, PartialEq, Eq)]
struct PropertyTarget {
    key: PropertyPath,
    parameter_id: String,
    effect_id: Option<EffectInstanceId>,
    value_path: SceneBindingValuePath,
}

impl PropertyTarget {
    fn animation_enabled(&self, item: &TimelineItem) -> bool {
        item.animation(
            self.effect_id,
            &self.parameter_id,
            self.value_path.array_element(),
        )
        .is_some_and(|animation| animation.channel_enabled(self.animation_address().channel))
    }

    fn animation_address(&self) -> ParameterAnimationAddress {
        ParameterAnimationAddress {
            array_index: self.value_path.array_element(),
            channel: self
                .value_path
                .tuple_element()
                .map_or(AnimationChannel::Scalar, AnimationChannel::TupleElement),
        }
    }

    fn animation_target(&self, item_id: ItemId) -> AnimationTarget {
        AnimationTarget {
            item_id,
            effect_id: self.effect_id,
            parameter_id: self.parameter_id.clone(),
            address: self.animation_address(),
            property: self.key.clone(),
        }
    }
}

#[derive(Clone)]
struct NumberInputSettings {
    suffix: String,
    min: f64,
    max: f64,
    step: f64,
    display_scale: f64,
}

#[derive(Clone)]
struct NumberField {
    target: PropertyTarget,
    input: NumberInputSettings,
    label: String,
    element_label: Option<String>,
    animatable: bool,
    is_size: bool,
    scalar_type: ScalarParameterType,
    scene_bindable: bool,
}

#[derive(Clone)]
struct SceneArgumentOption {
    scene_id: SceneId,
    id: String,
    label: String,
    schema: ParameterSchema,
    binding_count: usize,
    bindings: Vec<SceneBindingTarget>,
    derived: bool,
    referenced_by_derived: bool,
}

#[derive(Clone)]
struct SceneFieldBinding {
    target: SceneBindingTarget,
    connected: Option<(String, String)>,
    compatible: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SceneArgumentSetting {
    Default,
    Min,
    Max,
}

#[derive(Clone)]
struct NumberAnimationDisplay {
    source_parameter_id: String,
    source_address: ParameterAnimationAddress,
    value_scale: f64,
    from: f64,
    to: f64,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum AnimationEndpoint {
    From,
    To,
}

#[derive(Clone)]
struct AnimationInputBinding {
    path: PropertyPath,
    endpoint: AnimationEndpoint,
}

struct BoolField {
    target: PropertyTarget,
    label: String,
    value: bool,
    mixed: bool,
    scene_bindable: bool,
}

#[derive(Clone, Copy)]
struct AspectRatioLockState {
    value: bool,
    mixed: bool,
    multiple: bool,
    disabled_by_scene_size_argument: bool,
}

impl AspectRatioLockState {
    fn checked(self) -> bool {
        self.value && !self.mixed
    }
}

struct StringField {
    target: PropertyTarget,
    label: String,
    multiline: bool,
    value: String,
    scene_bindable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArrayElementEditor {
    Scalar,
    FontFamily,
}

enum PropertyControl {
    Number(Vec<NumberField>),
    Array(ArrayField),
    String(StringField),
    Choice(ChoiceField),
    Bool(BoolField),
    Color(ColorField),
}

impl PropertyControl {
    fn parameter_id(&self) -> &str {
        match self {
            Self::Number(fields) => fields
                .first()
                .map(|field| field.target.parameter_id.as_str())
                .expect("number controls contain at least one field"),
            Self::Array(field) => &field.target.parameter_id,
            Self::String(field) => &field.target.parameter_id,
            Self::Choice(field) => &field.target.parameter_id,
            Self::Bool(field) => &field.target.parameter_id,
            Self::Color(field) => &field.target.parameter_id,
        }
    }

    fn disable_animation(&mut self) {
        match self {
            Self::Number(fields) => {
                for field in fields {
                    field.animatable = false;
                }
            }
            Self::Array(field) => field.animation_allowed = false,
            Self::Color(field) => field.animatable = false,
            Self::String(_) | Self::Choice(_) | Self::Bool(_) => {}
        }
    }
}

struct ChoiceField {
    target: PropertyTarget,
    ty: ParameterType,
    scene_bindable: bool,
    label: String,
    value: u32,
    options: Vec<(String, u32)>,
}

#[derive(Clone)]
struct ArrayField {
    target: PropertyTarget,
    parameter: Box<ParameterSchema>,
    values: Vec<ParameterValue>,
    element_editor: ArrayElementEditor,
    animation_allowed: bool,
}

#[derive(Clone)]
struct ParameterBinding {
    item_id: ItemId,
    target: PropertyTarget,
}

#[derive(Clone)]
struct ColorField {
    target: PropertyTarget,
    label: String,
    animatable: bool,
    scene_bindable: bool,
}

#[derive(Clone)]
struct PropertyValueDrag {
    inspector_id: EntityId,
    path: PropertyPath,
    animation_endpoint: Option<AnimationEndpoint>,
}

#[derive(Clone)]
struct PropertyValueDragOrigin {
    target: PropertyTarget,
    animation_endpoint: Option<AnimationEndpoint>,
    start_x: f32,
    start_value: f64,
    min: f64,
    max: f64,
    step: f64,
    sensitivity: f64,
    animation_scale: f64,
    display_scale: f64,
}

#[derive(Clone)]
struct SceneArgumentValueDrag {
    inspector_id: EntityId,
    scene_id: SceneId,
    argument_id: String,
    setting: SceneArgumentSetting,
}

impl Render for SceneArgumentValueDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

struct SceneArgumentValueDragOrigin {
    scene_id: SceneId,
    argument_id: String,
    setting: SceneArgumentSetting,
    start_x: f32,
    start_value: f64,
    number: NumericInput,
    sensitivity: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InspectorInputStructure {
    item_id: Option<ItemId>,
    effect_ids: Vec<EffectInstanceId>,
    array_lengths: Vec<(Option<u64>, String, usize)>,
    item_scene_arguments: Vec<String>,
    active_scene: Option<SceneId>,
    active_scene_arguments: Vec<String>,
}

struct InspectorRenderContext<'a> {
    colors: ThemeColor,
    inspector: Entity<PropertyInspector>,
    editor: &'a Entity<TimelineEditor>,
    focus_handle: &'a FocusHandle,
    inputs: &'a HashMap<PropertyPath, Entity<InputState>>,
    animation_inputs: &'a HashMap<PropertyPath, (Entity<InputState>, Entity<InputState>)>,
    color_pickers: &'a HashMap<PropertyPath, Entity<ColorPickerState>>,
    animation_color_pickers:
        &'a HashMap<(PropertyPath, AnimationEndpoint), Entity<ColorPickerState>>,
    font_names: &'a [String],
    active_scene_name_input: Option<Entity<InputState>>,
}

struct InspectorSelectionView {
    item: TimelineItem,
    item_label: String,
    selected_count: usize,
    aspect_ratio_lock: Option<AspectRatioLockState>,
    property_controls: Vec<PropertyControl>,
    effects: Vec<EffectInstance>,
    scene_arguments: Vec<SceneArgumentOption>,
    file_inputs: Vec<(FileCapability, Option<MediaAsset>)>,
    available_effects: Vec<SearchPickerEntry<(String, String)>>,
    hidden_effects: HashSet<EffectInstanceId>,
    multiple: bool,
    editing_scene: bool,
    has_visual: bool,
    items_hidden: bool,
    item_visibility_mixed: bool,
    kind_label: String,
}

impl Render for PropertyValueDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

pub(crate) struct PropertyInspector {
    editor: Entity<TimelineEditor>,
    animation_selection: Entity<AnimationSelection>,
    media_readers: std::sync::Arc<MediaReaderRegistry>,
    session: Entity<ProjectSession>,
    session_id: ProjectSessionId,
    notifications: Entity<UiNotifications>,
    focus_handle: FocusHandle,
    controls: InspectorControls,
    font_names: Vec<String>,
    expanded_scene_arguments: HashSet<(SceneId, String)>,
    loading_file: bool,
    file_error: Option<SharedString>,
    _file_task: Task<()>,
    _editor_subscription: Subscription,
    _session_subscription: Subscription,
}

/// Owns the lifecycle of all ephemeral controls as one unit. A structural
/// selection change replaces this value, so entities and their subscriptions
/// cannot survive independently in parallel maps.
#[derive(Default)]
pub(crate) struct InspectorControls {
    input_structure: Option<InspectorInputStructure>,
    inputs: HashMap<PropertyPath, Entity<InputState>>,
    animation_inputs: HashMap<PropertyPath, (Entity<InputState>, Entity<InputState>)>,
    animation_input_subscriptions: HashMap<PropertyPath, Vec<Subscription>>,
    color_pickers: HashMap<PropertyPath, Entity<ColorPickerState>>,
    animation_color_pickers: HashMap<(PropertyPath, AnimationEndpoint), Entity<ColorPickerState>>,
    animation_color_subscriptions: HashMap<(PropertyPath, AnimationEndpoint), Subscription>,
    scene_name_inputs: HashMap<SceneId, Entity<InputState>>,
    scene_argument_name_inputs: HashMap<(SceneId, String), Entity<InputState>>,
    scene_argument_setting_inputs:
        HashMap<(SceneId, String, SceneArgumentSetting), Entity<InputState>>,
    scene_argument_default_inputs: HashMap<(SceneId, String), Entity<InputState>>,
    scene_argument_expression_inputs: HashMap<(SceneId, String), Entity<InputState>>,
    scene_argument_color_pickers: HashMap<(SceneId, String), Entity<ColorPickerState>>,
    value_drag_origin: Option<PropertyValueDragOrigin>,
    scene_argument_value_drag_origin: Option<SceneArgumentValueDragOrigin>,
    input_subscriptions: Vec<Subscription>,
}

impl PropertyInspector {
    const PARAMETER_LABEL_WIDTH: f32 = 64.;
    const DRAG_RANGE_PIXELS: f64 = 200.;
    const MIN_STEP_MULTIPLIER: f64 = 0.1;
    const MAX_STEP_MULTIPLIER: f64 = 2.;

    fn parameter_label_column(label: impl Into<SharedString>) -> Div {
        div()
            .w(px(Self::PARAMETER_LABEL_WIDTH))
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
            controls: InspectorControls::default(),
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
