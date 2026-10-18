//! Property rows grouped by the value being edited.
mod animation;
mod array;
mod binding;
mod choice;
mod color;
mod composition;
mod file;
mod number;
mod text;

use ::ui::ThemeColor;
use gpui::{Div, Entity, div, prelude::*};

use crate::ui::file_input::FileInputController;
use zerium_core::timeline::{ItemId, PropertyAddress, TimelineEditor};

use crate::ui::property_inspector::PropertyInspector;
use crate::ui::property_inspector::state::ControlStore;

pub(super) struct RenderCtx<'a> {
    pub colors: ThemeColor,
    pub editor: &'a Entity<TimelineEditor>,
    pub animation_address: Option<PropertyAddress>,
    pub inspector: Entity<PropertyInspector>,
    pub store: &'a ControlStore,
    pub font_names: &'a [String],
    pub item_id: ItemId,
    pub selecting_file: bool,
    pub file_input: Entity<FileInputController>,
    pub scene_overrides: std::collections::HashSet<String>,
}

pub(super) struct PropertyRow {
    label: Option<gpui::AnyElement>,
    content: Div,
}

impl PropertyRow {
    fn new(label: impl Into<Option<gpui::AnyElement>>, content: Div) -> Self {
        Self {
            label: label.into(),
            content,
        }
    }

    pub(super) fn into_cells(self) -> Vec<gpui::AnyElement> {
        let content = self.content.min_w_0().w_full();
        match self.label {
            Some(label) => vec![label, content.into_any_element()],
            None => vec![content.col_span_full().into_any_element()],
        }
    }
}

impl IntoElement for PropertyRow {
    type Element = Div;

    fn into_element(self) -> Div {
        PropertyInspector::property_grid().children(self.into_cells())
    }
}

impl PropertyInspector {
    pub(super) fn property_grid() -> Div {
        div()
            .grid()
            .grid_template_columns([
                gpui::GridTrackSize::FitContent(gpui::relative(0.3)),
                gpui::GridTrackSize::Fraction(1.),
            ])
            .min_w_0()
            .w_full()
            .items_center()
            .gap_x_3()
            .gap_y_1()
    }
}
