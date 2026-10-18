//! File rows, drag payload preparation, and drag preview rendering.
use ::ui::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, ThemeColor,
    button::{Button, ButtonVariants as _},
};
use gpui::{
    Context, Div, MouseButton, MouseUpEvent, Render, SharedString, Stateful, Window, div,
    prelude::*, px,
};
use rust_i18n::t;
use std::{collections::HashSet, path::PathBuf, rc::Rc};

use crate::ui::explorer::{Explorer, directory::ExplorerEntry};
use crate::ui::file_import::FileImport;

#[derive(Clone)]
pub(crate) struct ExplorerFileDrag {
    files: Rc<Vec<FileImport>>,
    label: SharedString,
}

impl ExplorerFileDrag {
    fn new(files: Vec<FileImport>, label: SharedString) -> Self {
        Self {
            files: Rc::new(files),
            label,
        }
    }

    pub(crate) fn files(&self) -> &[FileImport] {
        &self.files
    }
}

impl Render for ExplorerFileDrag {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let width = px(200.);

        div()
            .h(px(30.))
            .w(width)
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .rounded_sm()
            .border_1()
            .border_color(colors.primary)
            .bg(colors.background.opacity(0.96))
            .text_sm()
            .text_color(colors.foreground)
            .shadow_lg()
            .child(Icon::new(IconName::Page).small())
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(self.label.clone()),
            )
    }
}

impl Explorer {
    fn selected_drag_files(
        entries: &[ExplorerEntry],
        selected_paths: &HashSet<PathBuf>,
    ) -> Vec<FileImport> {
        entries
            .iter()
            .filter(|entry| selected_paths.contains(&entry.path))
            .filter_map(|entry| {
                Some(FileImport::new(
                    entry.path.clone(),
                    entry.import_target.clone()?,
                ))
            })
            .collect()
    }
}

impl Explorer {
    fn toolbar(&self, colors: ThemeColor, cx: &mut Context<Self>) -> impl IntoElement {
        let has_parent = self.current_directory.parent().is_some();
        div()
            .h(px(32.))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .border_b_1()
            .border_color(colors.border)
            .child(
                Button::new("explorer-parent")
                    .icon(IconName::ChevronUp)
                    .tooltip(t!("explorer.parent_folder").to_string())
                    .small()
                    .ghost()
                    .disabled(!has_parent)
                    .on_click(cx.listener(Self::open_parent)),
            )
            .child(
                Button::new("explorer-home")
                    .icon(IconName::Home)
                    .tooltip(t!("explorer.home").to_string())
                    .small()
                    .ghost()
                    .on_click(cx.listener(Self::open_home)),
            )
            .child(
                div()
                    .h(px(24.))
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .rounded_sm()
                    .border_1()
                    .border_color(colors.border)
                    .px_2()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(self.current_directory.display().to_string()),
            )
            .child(
                Button::new("explorer-refresh")
                    .icon(IconName::Refresh)
                    .tooltip(t!("explorer.refresh").to_string())
                    .small()
                    .ghost()
                    .on_click(cx.listener(Self::refresh)),
            )
    }

    fn entry_row(
        entry: ExplorerEntry,
        selected: bool,
        selected_files: Rc<Vec<FileImport>>,
        colors: ThemeColor,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let is_directory = entry.is_directory;
        let import_target = entry.import_target.clone();
        let path = entry.path.clone();
        let click_path = entry.path.clone();
        let drag_label = entry.name.clone();
        div()
            .id(("explorer-entry", entry.element_id))
            .h(px(30.))
            .flex_none()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .rounded_sm()
            .text_sm()
            .text_color(if import_target.is_some() || is_directory {
                colors.foreground
            } else {
                colors.muted_foreground
            })
            .when(selected, |row| row.bg(colors.accent))
            .when(!selected, |row| {
                row.hover(move |style| style.bg(colors.accent.opacity(0.1)))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event, _, cx| {
                    this.select_entry(path.clone(), event, cx);
                }),
            )
            .on_click(cx.listener(move |this, event, _, cx| {
                this.open_entry(click_path.clone(), is_directory, event, cx);
            }))
            .child(
                div()
                    .w(px(16.))
                    .flex_none()
                    .when(is_directory, |this| {
                        this.child(
                            Icon::new(IconName::Folder)
                                .small()
                                .text_color(colors.primary),
                        )
                    })
                    .when(!is_directory, |this| {
                        this.child(
                            Icon::new(IconName::Page)
                                .small()
                                .text_color(colors.muted_foreground),
                        )
                    }),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(entry.name),
            )
            .when_some(import_target, move |row, target| {
                let file = FileImport::new(entry.path, target);
                let files = if selected {
                    selected_files.as_ref().clone()
                } else {
                    vec![file]
                };
                let label = if files.len() == 1 {
                    drag_label
                } else {
                    SharedString::from(t!("explorer.drag_count", count = files.len()).to_string())
                };
                let drag = ExplorerFileDrag::new(files, label);
                row.cursor_grab()
                    .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
            })
    }
}

impl Render for Explorer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let entries = self.entries.clone();
        let selected_paths = self.selected_paths.clone();
        let selected_files = Rc::new(Self::selected_drag_files(&entries, &selected_paths));
        let error = self.error.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(self.toolbar(colors, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::begin_marquee))
                    .on_mouse_move(cx.listener(Self::update_marquee))
                    .capture_any_mouse_up(cx.listener(|this, event: &MouseUpEvent, _, cx| {
                        if event.button == MouseButton::Left {
                            this.finish_marquee(event, cx);
                        }
                    }))
                    .child(
                        div()
                            .id("explorer-files-scroll")
                            .size_full()
                            .flex()
                            .flex_col()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll_handle)
                            .p_1()
                            .children(entries.into_iter().map(|entry| {
                                let selected = selected_paths.contains(&entry.path);
                                Self::entry_row(entry, selected, selected_files.clone(), colors, cx)
                            })),
                    )
                    .when_some(self.marquee_overlay(colors), |this, marquee| {
                        this.child(marquee)
                    }),
            )
            .when_some(error, |this, error| {
                this.child(
                    div()
                        .flex_none()
                        .px_2()
                        .py_1()
                        .border_t_1()
                        .border_color(colors.border)
                        .text_xs()
                        .text_color(colors.danger)
                        .child(error),
                )
            })
    }
}
