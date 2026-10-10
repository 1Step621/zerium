//! Contextual help derived from the visible UI and registered key bindings.
use std::rc::Rc;

use ::ui::{ActiveTheme as _, Kbd};
use gpui::{
    Action, AnyElement, App, AsKeystroke as _, Bounds, Context, DispatchPhase, Element, ElementId,
    Entity, Global, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement,
    Keystroke, LayoutId, Modifiers, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Render,
    SharedString, Window, WindowId, canvas, div, prelude::*, px,
};
use rust_i18n::t;

pub(crate) struct Hint {
    input: HintInput,
    label: &'static str,
}

enum HintInput {
    Action(Box<dyn Action>),
    Gesture {
        input: &'static str,
        modifiers: Modifiers,
    },
}

impl Hint {
    pub(crate) fn action(action: impl Action, label: &'static str) -> Self {
        Self {
            input: HintInput::Action(Box::new(action)),
            label,
        }
    }

    pub(super) fn gesture(input: &'static str, label: &'static str, modifiers: Modifiers) -> Self {
        Self {
            input: HintInput::Gesture { input, modifiers },
            label,
        }
    }

    fn display(&self, window: &Window, cx: &App) -> Option<DisplayedHint> {
        let (input, modifiers) = match &self.input {
            HintInput::Action(action) => {
                let focus = window.focused(cx)?;
                let binding =
                    window.highest_precedence_binding_for_action_in(action.as_ref(), &focus)?;
                let input = binding
                    .keystrokes()
                    .iter()
                    .map(|key| Kbd::format(key.as_keystroke()))
                    .collect::<Vec<_>>()
                    .join(" ");
                (
                    input,
                    binding.keystrokes().first()?.as_keystroke().modifiers,
                )
            }
            HintInput::Gesture { input, modifiers } => {
                let keys = Kbd::format(&Keystroke {
                    modifiers: *modifiers,
                    key: String::new(),
                    key_char: None,
                });
                let keys = keys.trim_end_matches('+');
                let input = t!(*input).to_string();
                (
                    if keys.is_empty() {
                        input
                    } else {
                        format!("{keys}+{input}")
                    },
                    *modifiers,
                )
            }
        };
        let active = modifiers.modified() && modifiers == window.modifiers();
        Some(DisplayedHint {
            input: input.into(),
            label: t!(self.label).to_string().into(),
            active,
        })
    }
}

type HintsProvider = Box<dyn Fn(&Window, &App) -> Vec<Hint>>;

struct Region {
    hitbox: Hitbox,
    hints: Rc<[Hint]>,
}

struct HintFrame {
    window: WindowId,
    regions: Vec<Region>,
}

impl Global for HintFrame {}

pub(super) trait OperationHintExt: IntoElement {
    fn track_operation_hints(self, bar: &Entity<OperationHintBar>) -> Self
    where
        Self: gpui::InteractiveElement + gpui::ParentElement,
    {
        let hints = bar.downgrade();
        self.on_modifiers_changed(move |_, window, cx| {
            let _ = hints.update(cx, |bar, cx| bar.refresh(window, cx));
        })
        .child(OperationHintBar::frame(bar))
    }

    /// An empty scope hides its parent's local hints.
    fn operation_hints(self, hints: impl IntoIterator<Item = Hint>) -> HintScope {
        HintScope {
            element: self.into_any_element(),
            hints: hints.into_iter().collect::<Vec<_>>().into(),
        }
    }
}

impl<T: IntoElement> OperationHintExt for T {}

/// A transparent wrapper: layout and input handling stay with the original element.
pub(super) struct HintScope {
    element: AnyElement,
    hints: Rc<[Hint]>,
}

impl IntoElement for HintScope {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for HintScope {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.element.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.element.prepaint(window, cx);
        hitbox
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(frame) = cx.try_global::<HintFrame>()
            && frame.window == window.window_handle().window_id()
        {
            cx.global_mut::<HintFrame>().regions.push(Region {
                hitbox: hitbox.clone(),
                hints: self.hints.clone(),
            });
        }
        self.element.paint(window, cx);
    }
}

#[derive(PartialEq, Eq)]
struct DisplayedHint {
    input: SharedString,
    label: SharedString,
    active: bool,
}

pub(crate) struct OperationHintBar {
    global_hints: HintsProvider,
    pressed_hints: Option<Rc<[Hint]>>,
    displayed: Vec<DisplayedHint>,
}

impl OperationHintBar {
    pub(crate) fn new(global_hints: impl Fn(&Window, &App) -> Vec<Hint> + 'static) -> Self {
        Self {
            global_hints: Box::new(global_hints),
            pressed_hints: None,
            displayed: Vec::new(),
        }
    }

    /// The registrations belong to this frame, so removed controls cannot leave
    /// stale help behind. Paint order naturally gives child controls precedence.
    fn frame(bar: &Entity<Self>) -> impl IntoElement {
        let bar = bar.downgrade();
        canvas(
            |_, _, _| (),
            move |_, _, window, cx| {
                cx.set_global(HintFrame {
                    window: window.window_handle().window_id(),
                    regions: Vec::new(),
                });
                let press_bar = bar.clone();
                window.on_mouse_event(move |_: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        // Capture the scope before the control changes focus or moves.
                        let _ = press_bar.update(cx, |bar, cx| bar.refresh(window, cx));
                    }
                });
                let mouse_bar = bar.clone();
                window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        let bar = mouse_bar.clone();
                        window.defer(cx, move |window, cx| {
                            let _ = bar.update(cx, |bar, cx| bar.refresh(window, cx));
                        });
                    }
                });
                let release_bar = bar.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        let bar = release_bar.clone();
                        window.defer(cx, move |window, cx| {
                            let _ = bar.update(cx, |bar, cx| bar.refresh(window, cx));
                        });
                    }
                });
            },
        )
        .absolute()
        .w_0()
        .h_0()
    }

    fn refresh(&mut self, window: &Window, cx: &mut Context<Self>) {
        let frame = cx.global::<HintFrame>();
        let hover = frame
            .regions
            .iter()
            .rev()
            .find(|region| region.hitbox.is_hovered(window));
        if window.pressed_mouse_button().is_none() {
            self.pressed_hints = None;
        } else if self.pressed_hints.is_none() {
            // An empty scope is also retained until the button is released.
            self.pressed_hints = Some(hover.map(|region| region.hints.clone()).unwrap_or_default());
        }
        let contextual = self
            .pressed_hints
            .as_deref()
            .or_else(|| hover.map(|region| region.hints.as_ref()))
            .unwrap_or_default();
        let global = (self.global_hints)(window, cx);
        let displayed = global
            .iter()
            .chain(contextual)
            .filter_map(|hint| hint.display(window, cx))
            .collect::<Vec<_>>();
        if displayed != self.displayed {
            self.displayed = displayed;
            cx.notify();
        }
    }
}

impl Render for OperationHintBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let bar = cx.entity().downgrade();
        div()
            .relative()
            .h(px(24.))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_4()
            .px_3()
            .overflow_hidden()
            .border_t_1()
            .border_color(colors.border)
            .bg(colors.background)
            .text_xs()
            .text_color(colors.muted_foreground)
            .children(self.displayed.iter().map(|hint| {
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_color(if hint.active {
                                colors.primary
                            } else {
                                colors.foreground
                            })
                            .child(hint.input.clone()),
                    )
                    .child(hint.label.clone())
            }))
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, cx| {
                        let bar = bar.clone();
                        window.defer(cx, move |window, cx| {
                            let _ = bar.update(cx, |bar, cx| bar.refresh(window, cx));
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}
