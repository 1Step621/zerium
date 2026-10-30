//! Animation labels, timing clipboard menus, and synchronization presentation.
use crate::app::copy_buffer::CopyBuffer;
use ::ui::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Sizable as _, ThemeColor, ThemeMode,
    menu::PopupMenu, tooltip::Tooltip,
};
use gpui::{App, Entity, Hsla, Window, div, prelude::*, px};
use rust_i18n::t;
use std::rc::Rc;
use zerium_core::animation::AnimationTiming;
use zerium_core::timeline::{AnimationEdit, PropertyAddress, TimelineEditor, TimelineItem};

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

#[derive(Clone)]
pub(crate) struct AnimationSynchronization {
    editor: Entity<TimelineEditor>,
    properties: Rc<[PropertyAddress]>,
}

impl AnimationSynchronization {
    pub(crate) fn from_edit(editor: &Entity<TimelineEditor>, edit: &AnimationEdit) -> Self {
        Self {
            editor: editor.clone(),
            properties: edit.properties().cloned().collect(),
        }
    }

    pub(crate) fn is_linked(&self) -> bool {
        self.properties.len() > 1
    }

    pub(crate) fn color(&self, colors: &ThemeColor, mode: ThemeMode) -> Hsla {
        match (self.is_linked(), mode) {
            (false, _) => colors.primary,
            (true, ThemeMode::Dark) => colors.primary.lighten(0.12),
            (true, ThemeMode::Light) => colors.primary.darken(0.12),
        }
    }

    fn label(&self) -> String {
        if self.is_linked() {
            t!("curve.synchronized_count", count = self.properties.len())
        } else {
            t!("curve.independent")
        }
        .to_string()
    }

    pub(crate) fn tooltip(&self, window: &mut Window, cx: &mut App) -> gpui::AnyView {
        let label = self.label();
        // Marker rendering only needs the count. Resolve labels when a tooltip
        // is requested, rather than formatting every target for every key/frame.
        let editor = self.editor.read(cx);
        let mut properties = self
            .properties
            .iter()
            .map(|address| synchronized_property_label(editor, address))
            .collect::<Vec<_>>();
        properties.sort();
        Tooltip::element(move |_, cx| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .max_w(px(480.))
                .child(label.clone())
                .children(properties.iter().map(|label| {
                    div()
                        .text_color(cx.theme().colors.muted_foreground)
                        .child(label.clone())
                }))
        })
        .build(window, cx)
    }

    pub(crate) fn indicator(&self, cx: &App) -> gpui::AnyElement {
        let colors = cx.theme().colors;
        let color = self.color(&colors, cx.theme().mode);
        let synchronization = self.clone();
        div()
            .id("animation-synchronization")
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .rounded_md()
            .text_xs()
            .whitespace_nowrap()
            .bg(if self.is_linked() {
                colors.background.blend(color.opacity(0.12))
            } else {
                colors.background
            })
            .text_color(color)
            .child(
                Icon::new(if self.is_linked() {
                    IconName::Link
                } else {
                    IconName::LinkSlash
                })
                .small(),
            )
            .child(self.label())
            .tooltip(move |window, cx| synchronization.tooltip(window, cx))
            .into_any_element()
    }
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

fn synchronized_property_label(editor: &TimelineEditor, address: &PropertyAddress) -> String {
    let Some(item) = editor.item(address.item_id) else {
        return address.property_id.clone();
    };
    let property = animation_property_label(editor, item, address)
        .unwrap_or_else(|| address.property_id.clone());
    let property = if let Some((index, effect)) = address.effect_id.and_then(|id| {
        item.effects
            .iter()
            .enumerate()
            .find(|(_, effect)| effect.id == id)
    }) {
        format!("{}. {} › {property}", index + 1, effect.schema().label())
    } else {
        property
    };
    let item_label = t!(
        "inspector.item_option",
        name = editor.item_label(item.id).unwrap_or_default(),
        layer = editor
            .item_layer(item.id)
            .map_or(0, |layer| layer.get() + 1),
        frame = item.start().get(),
    );
    format!("{item_label} › {property}")
}
