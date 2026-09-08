use std::{
    collections::HashSet,
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    rc::Rc,
};

use ::ui::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, ThemeColor,
    button::{Button, ButtonVariants as _},
};
use directories::UserDirs;
use gpui::{
    Bounds, ClickEvent, Context, Div, Entity, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Render, ScrollHandle, SharedString, Stateful, Subscription, Task, Window,
    div, point, prelude::*, px, size,
};

use crate::{
    plugin_catalog::plugins,
    ui::session::{ProjectSession, ProjectSessionId, UiNotifications},
};

#[derive(Clone)]
struct FileImportTarget {
    plugin_id: String,
    item_id: String,
    input_id: String,
}

#[derive(Clone)]
pub(crate) struct ExplorerDraggedFile {
    path: PathBuf,
    target: FileImportTarget,
}

impl ExplorerDraggedFile {
    fn new(path: PathBuf, target: FileImportTarget) -> Self {
        Self { path, target }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn plugin_id(&self) -> &str {
        &self.target.plugin_id
    }

    pub(crate) fn item_id(&self) -> &str {
        &self.target.item_id
    }

    pub(crate) fn input_id(&self) -> &str {
        &self.target.input_id
    }
}

#[derive(Clone)]
pub(crate) struct ExplorerFileDrag {
    files: Rc<Vec<ExplorerDraggedFile>>,
    label: SharedString,
}

impl ExplorerFileDrag {
    fn new(files: Vec<ExplorerDraggedFile>, label: SharedString) -> Self {
        Self {
            files: Rc::new(files),
            label,
        }
    }

    pub(crate) fn files(&self) -> &[ExplorerDraggedFile] {
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
            .child(Icon::new(IconName::Page).xsmall())
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

#[derive(Clone)]
struct ExplorerEntry {
    element_id: u64,
    path: PathBuf,
    name: SharedString,
    is_directory: bool,
    import_target: Option<FileImportTarget>,
}

#[derive(Clone, Debug)]
struct ExplorerMarquee {
    origin: [f32; 2],
    current: [f32; 2],
    baseline: HashSet<PathBuf>,
    active: bool,
}

impl ExplorerMarquee {
    const ACTIVATION_DISTANCE: f32 = 4.;

    fn update(&mut self, position: gpui::Point<Pixels>) {
        self.current = [f32::from(position.x), f32::from(position.y)];
        self.active |= (self.current[0] - self.origin[0]).hypot(self.current[1] - self.origin[1])
            >= Self::ACTIVATION_DISTANCE;
    }

    fn bounds(&self) -> Bounds<Pixels> {
        Bounds {
            origin: point(
                px(self.origin[0].min(self.current[0])),
                px(self.origin[1].min(self.current[1])),
            ),
            size: size(
                px((self.origin[0] - self.current[0]).abs()),
                px((self.origin[1] - self.current[1]).abs()),
            ),
        }
    }
}

pub(crate) struct Explorer {
    session: Entity<ProjectSession>,
    session_id: ProjectSessionId,
    notifications: Entity<UiNotifications>,
    current_directory: PathBuf,
    entries: Vec<ExplorerEntry>,
    selected_paths: HashSet<PathBuf>,
    scroll_handle: ScrollHandle,
    marquee: Option<ExplorerMarquee>,
    error: Option<SharedString>,
    load_generation: u64,
    _load_task: Task<()>,
    _session_subscription: Subscription,
}

impl Explorer {
    pub(crate) fn new(
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
        cx: &mut Context<Self>,
    ) -> Self {
        let session_id = session.read(cx).id();
        let initial_directory = UserDirs::new()
            .map(|directories| directories.home_dir().to_path_buf())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let session_subscription = cx.observe(&session, |this, _, cx| {
            let session_id = this.session.read(cx).id();
            if session_id == this.session_id {
                return;
            }
            this.session_id = session_id;
            this.load_generation = this.load_generation.saturating_add(1);
            this._load_task = Task::ready(());
            cx.notify();
        });
        let mut explorer = Self {
            session,
            session_id,
            notifications,
            current_directory: initial_directory.clone(),
            entries: Vec::new(),
            selected_paths: HashSet::new(),
            scroll_handle: ScrollHandle::new(),
            marquee: None,
            error: None,
            load_generation: 0,
            _load_task: Task::ready(()),
            _session_subscription: session_subscription,
        };
        explorer.load_directory(initial_directory, cx);
        explorer
    }

    fn import_target(path: &Path) -> Option<FileImportTarget> {
        let extension = path.extension()?.to_str()?;
        plugins().items().find_map(|(plugin_id, item)| {
            item.files().iter().find_map(|input| {
                (input.extensions().is_empty()
                    || input
                        .extensions()
                        .iter()
                        .any(|allowed| allowed.eq_ignore_ascii_case(extension)))
                .then(|| FileImportTarget {
                    plugin_id: plugin_id.to_owned(),
                    item_id: item.id().to_owned(),
                    input_id: input.id().to_owned(),
                })
            })
        })
    }

    fn selected_drag_files(
        entries: &[ExplorerEntry],
        selected_paths: &HashSet<PathBuf>,
    ) -> Vec<ExplorerDraggedFile> {
        entries
            .iter()
            .filter(|entry| selected_paths.contains(&entry.path))
            .filter_map(|entry| {
                Some(ExplorerDraggedFile::new(
                    entry.path.clone(),
                    entry.import_target.clone()?,
                ))
            })
            .collect()
    }

    fn read_directory(path: &Path) -> Result<Vec<(PathBuf, String, bool)>, String> {
        let directory = fs::read_dir(path)
            .map_err(|error| format!("'{}'を開けません: {error}", path.display()))?;
        let mut entries = Vec::new();
        for entry in directory {
            let entry = entry
                .map_err(|error| format!("'{}'の項目を読み取れません: {error}", path.display()))?;
            let entry_path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let is_directory = entry
                .file_type()
                .map_err(|error| {
                    format!("'{}'の種類を取得できません: {error}", entry_path.display())
                })?
                .is_dir();
            entries.push((entry_path, name, is_directory));
        }
        entries.sort_by(|left, right| {
            right
                .2
                .cmp(&left.2)
                .then_with(|| left.1.to_lowercase().cmp(&right.1.to_lowercase()))
        });
        Ok(entries)
    }

    fn load_directory(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.load_generation = self.load_generation.saturating_add(1);
        let generation = self.load_generation;
        let session_id = self.session.read(cx).id();
        let session = self.session.clone();
        self.error = None;
        self._load_task = cx.spawn(async move |explorer, cx| {
            let input = path.clone();
            let result = cx
                .background_spawn(async move { Self::read_directory(&input) })
                .await;
            if !session.update(cx, |session, _| session.is_current(session_id)) {
                return;
            }
            explorer
                .update(cx, |explorer, cx| {
                    if explorer.load_generation != generation {
                        return;
                    }
                    match result {
                        Ok(raw_entries) => {
                            explorer.current_directory = path;
                            explorer.entries = raw_entries
                                .into_iter()
                                .map(|(path, name, is_directory)| {
                                    let mut hasher = DefaultHasher::new();
                                    path.hash(&mut hasher);
                                    ExplorerEntry {
                                        element_id: hasher.finish(),
                                        import_target: (!is_directory)
                                            .then(|| Self::import_target(&path))
                                            .flatten(),
                                        path,
                                        name: name.into(),
                                        is_directory,
                                    }
                                })
                                .collect();
                            explorer.selected_paths.clear();
                            explorer.marquee = None;
                            explorer.error = None;
                        }
                        Err(error) => {
                            explorer.error = Some(error.clone().into());
                            explorer.notifications.update(cx, |notifications, cx| {
                                notifications.push(error, cx);
                            });
                        }
                    }
                    cx.notify();
                })
                .ok();
        });
    }

    fn open_parent(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(parent) = self.current_directory.parent().map(Path::to_path_buf) else {
            return;
        };
        self.load_directory(parent, cx);
        cx.notify();
    }

    fn open_home(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(home) = UserDirs::new().map(|directories| directories.home_dir().to_path_buf())
        else {
            return;
        };
        self.load_directory(home, cx);
        cx.notify();
    }

    fn refresh(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.load_directory(self.current_directory.clone(), cx);
        cx.notify();
    }

    fn select_entry(&mut self, path: PathBuf, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let additive = event.modifiers.shift || event.modifiers.control || event.modifiers.platform;
        if additive {
            if !self.selected_paths.insert(path.clone()) {
                self.selected_paths.remove(&path);
            }
        } else if !self.selected_paths.contains(&path) {
            self.selected_paths.clear();
            self.selected_paths.insert(path);
        }
        cx.notify();
        cx.stop_propagation();
    }

    fn open_entry(
        &mut self,
        path: PathBuf,
        is_directory: bool,
        event: &ClickEvent,
        cx: &mut Context<Self>,
    ) {
        if is_directory && event.click_count() >= 2 {
            self.load_directory(path, cx);
            cx.notify();
        }
    }

    fn begin_marquee(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let additive = event.modifiers.shift || event.modifiers.control || event.modifiers.platform;
        let baseline = if additive {
            self.selected_paths.clone()
        } else {
            self.selected_paths.clear();
            HashSet::new()
        };
        let position = [f32::from(event.position.x), f32::from(event.position.y)];
        self.marquee = Some(ExplorerMarquee {
            origin: position,
            current: position,
            baseline,
            active: false,
        });
        cx.notify();
    }

    fn update_marquee(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !event.dragging() || self.marquee.is_none() {
            return;
        }
        self.marquee
            .as_mut()
            .expect("explorer marquee was checked above")
            .update(event.position);
        if !self.marquee.as_ref().is_some_and(|marquee| marquee.active) {
            return;
        }
        self.apply_marquee();
        cx.notify();
    }

    fn apply_marquee(&mut self) {
        let Some(marquee) = self.marquee.as_ref().filter(|marquee| marquee.active) else {
            return;
        };
        let selection_bounds = marquee.bounds();
        let viewport = self.scroll_handle.bounds();
        let scroll_offset = self.scroll_handle.offset();
        let mut selected = marquee.baseline.clone();
        for (index, entry) in self.entries.iter().enumerate() {
            let Some(mut row) = self.scroll_handle.bounds_for_item(index) else {
                continue;
            };
            row.origin.x += scroll_offset.x;
            row.origin.y += scroll_offset.y;
            if row.intersects(&viewport) && row.intersects(&selection_bounds) {
                selected.insert(entry.path.clone());
            }
        }
        self.selected_paths = selected;
    }

    fn finish_marquee(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(mut marquee) = self.marquee.take() else {
            return;
        };
        marquee.update(event.position);
        if marquee.active {
            self.marquee = Some(marquee);
            self.apply_marquee();
            self.marquee = None;
        }
        cx.notify();
    }

    fn marquee_overlay(&self, colors: ThemeColor) -> Option<Div> {
        let marquee = self.marquee.as_ref().filter(|marquee| marquee.active)?;
        let viewport = self.scroll_handle.bounds();
        let bounds = marquee.bounds();
        let left = f32::from(bounds.origin.x).max(f32::from(viewport.origin.x));
        let right = f32::from(bounds.origin.x + bounds.size.width)
            .min(f32::from(viewport.origin.x + viewport.size.width));
        let top = f32::from(bounds.origin.y).max(f32::from(viewport.origin.y));
        let bottom = f32::from(bounds.origin.y + bounds.size.height)
            .min(f32::from(viewport.origin.y + viewport.size.height));
        if right <= left || bottom <= top {
            return None;
        }
        Some(
            div()
                .absolute()
                .left(px(left - f32::from(viewport.origin.x)))
                .top(px(top - f32::from(viewport.origin.y)))
                .w(px(right - left))
                .h(px(bottom - top))
                .border_1()
                .border_color(colors.primary)
                .bg(colors.primary.opacity(0.12)),
        )
    }

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
                    .tooltip("親フォルダー")
                    .xsmall()
                    .ghost()
                    .disabled(!has_parent)
                    .on_click(cx.listener(Self::open_parent)),
            )
            .child(
                Button::new("explorer-home")
                    .icon(IconName::Home)
                    .tooltip("ホーム")
                    .xsmall()
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
                    .tooltip("更新")
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(Self::refresh)),
            )
    }

    fn entry_row(
        entry: ExplorerEntry,
        selected: bool,
        selected_files: Rc<Vec<ExplorerDraggedFile>>,
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
                                .xsmall()
                                .text_color(colors.primary),
                        )
                    })
                    .when(!is_directory, |this| {
                        this.child(
                            Icon::new(IconName::Page)
                                .xsmall()
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
                let file = ExplorerDraggedFile::new(entry.path, target);
                let files = if selected {
                    selected_files.as_ref().clone()
                } else {
                    vec![file]
                };
                let label = if files.len() == 1 {
                    drag_label
                } else {
                    SharedString::from(format!("{}個のファイル", files.len()))
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
