//! Animation markers, labels, and timing clipboard menus.
use crate::app::copy_buffer::CopyBuffer;
use ::ui::{Icon, IconName, ThemeColor, menu::PopupMenu};
use gpui::{AnyElement, App, Context, PathBuilder, Window, canvas, div, point, prelude::*, px};
use rust_i18n::t;
use std::rc::Rc;
use zerium_core::animation::{AnimationTiming, StopValueSource};
use zerium_core::timeline::{PropertyAddress, TimelineEditor, TimelineItem};

pub(crate) fn animation_stop_marker(fixed: bool, selected: bool, colors: ThemeColor) -> AnyElement {
    if fixed {
        div()
            .size(px(6.))
            .rounded_sm()
            .bg(colors.muted_foreground)
            .into_any_element()
    } else {
        let background = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let position = |x: f32, y: f32| {
                    bounds.origin
                        + point(
                            bounds.size.width * (x / 24.),
                            bounds.size.height * (y / 24.),
                        )
                };
                // Fill the icon's interior; the original SVG still draws its outline.
                let mut path = PathBuilder::fill();
                path.move_to(position(20.777, 13.3453));
                path.line_to(position(13.4799, 21.3721));
                path.cubic_bezier_to(
                    position(10.5201, 21.3721),
                    position(12.6864, 22.245),
                    position(11.3136, 22.245),
                );
                path.line_to(position(3.22304, 13.3453));
                path.cubic_bezier_to(
                    position(3.22304, 10.6547),
                    position(2.52955, 12.5825),
                    position(2.52955, 11.4175),
                );
                path.line_to(position(10.5201, 2.62787));
                path.cubic_bezier_to(
                    position(13.4799, 2.62787),
                    position(11.3136, 1.755),
                    position(12.6864, 1.755),
                );
                path.line_to(position(20.777, 10.6547));
                path.cubic_bezier_to(
                    position(20.777, 13.3453),
                    position(21.4705, 11.4175),
                    position(21.4705, 12.5825),
                );
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, colors.background);
                }
            },
        )
        .absolute()
        .size_full();
        div()
            .relative()
            .size(px(11.))
            .flex_none()
            .child(background)
            .child(
                Icon::new(IconName::Keyframe)
                    .size(px(11.))
                    .text_color(if selected {
                        colors.primary
                    } else {
                        colors.foreground
                    }),
            )
            .into_any_element()
    }
}

pub(crate) fn animation_timing_menu(
    menu: PopupMenu,
    editor: &TimelineEditor,
    address: Option<PropertyAddress>,
    on_paste: impl Fn(&AnimationTiming, &mut App) + 'static,
    cx: &App,
) -> PopupMenu {
    let copy = address
        .as_ref()
        .and_then(|address| editor.animation_track(address))
        .map(|track| Rc::new(track.timing()));
    let paste = cx
        .try_global::<CopyBuffer>()
        .and_then(|buffer| buffer.animation_timing.clone());
    let paste_disabled = paste.is_none()
        || address
            .as_ref()
            .is_none_or(|address| !editor.can_animate_property(address));
    menu.menu_handler_with_icon_and_disabled(
        t!("clipboard.copy_animation_timing").to_string(),
        IconName::Copy,
        copy.is_none(),
        move |_, cx| {
            if let Some(copy) = &copy {
                cx.default_global::<CopyBuffer>().animation_timing = Some(copy.clone());
            }
        },
    )
    .menu_handler_with_icon_and_disabled(
        t!("clipboard.paste_animation_timing").to_string(),
        IconName::PasteClipboard,
        paste_disabled,
        move |_, cx| {
            if let Some(paste) = &paste {
                on_paste(paste, cx);
            }
        },
    )
}

pub(crate) fn animation_property_label(
    editor: &TimelineEditor,
    item: &TimelineItem,
    address: &PropertyAddress,
) -> Option<String> {
    let property = editor.property_schema(address)?;
    let element_index = address.element_id.and_then(|id| {
        item.property_overrides(address.effect_id)?
            .resolve(property)
            .element_index(id)
    });
    let label = element_index.map_or_else(
        || property.label().to_owned(),
        |index| format!("{} {}", property.label(), index + 1),
    );
    Some(match property.configuration_label(address.scalar_index) {
        Some(scalar_label) => format!("{label} {scalar_label}"),
        None => label,
    })
}

/// Shared insertion choices for the timeline and both curve-editor menus.
pub(crate) fn animation_stop_add_menu(
    menu: PopupMenu,
    enabled: bool,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    on_add: impl Fn(StopValueSource, &mut App) + 'static,
) -> PopupMenu {
    let on_add = Rc::new(on_add);
    menu.submenu_with_icon_with_disabled(
        Some(Icon::new(IconName::Plus)),
        t!("curve.add_stop").to_string(),
        !enabled,
        window,
        cx,
        move |menu, _, _| {
            [
                (StopValueSource::Previous, "curve.stop_from_previous"),
                (StopValueSource::Next, "curve.stop_from_next"),
            ]
            .into_iter()
            .fold(menu, |menu, (source, label)| {
                let on_add = on_add.clone();
                menu.menu_handler(t!(label).to_string(), move |_, cx| on_add(source, cx))
            })
        },
    )
}
