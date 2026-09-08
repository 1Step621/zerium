use std::rc::Rc;

use ::ui::{
    ActiveTheme as _, IndexPath,
    list::{List, ListDelegate, ListEvent, ListItem},
};
use gpui::{
    App, Context, DismissEvent, Edges, Entity, EventEmitter, FocusHandle, Render, SharedString,
    Subscription, Task, Window, div, prelude::*, px,
};

use crate::domain::plugin::PluginCatalogEntry;

#[derive(Clone)]
pub(crate) struct SearchPickerEntry<T> {
    label: SharedString,
    category: SharedString,
    searchable_text: String,
    value: T,
}

impl<T> SearchPickerEntry<T> {
    pub(crate) fn new(
        label: impl Into<SharedString>,
        category: impl Into<SharedString>,
        value: T,
    ) -> Self {
        let label = label.into();
        let category = category.into();
        let searchable_text = format!("{label}\n{category}").to_lowercase();
        Self {
            label,
            category,
            searchable_text,
            value,
        }
    }

    pub(crate) fn search_terms<S>(mut self, terms: impl IntoIterator<Item = S>) -> Self
    where
        S: AsRef<str>,
    {
        for term in terms {
            self.searchable_text.push('\n');
            self.searchable_text.push_str(&term.as_ref().to_lowercase());
        }
        self
    }

    pub(crate) fn from_plugin_schema(
        plugin_id: &str,
        schema: &impl PluginCatalogEntry,
        value: T,
    ) -> Self {
        Self::new(
            schema.label().to_owned(),
            schema.category().to_owned(),
            value,
        )
        .search_terms(
            [plugin_id, schema.id()]
                .into_iter()
                .chain(schema.tags().iter().map(String::as_str)),
        )
    }

    fn matches(&self, query: &str) -> bool {
        query
            .split_whitespace()
            .map(str::to_lowercase)
            .all(|token| self.searchable_text.contains(&token))
    }
}

struct SearchPickerSection {
    category: SharedString,
    entry_indices: Vec<usize>,
}

struct SearchPickerDelegate<T> {
    entries: Vec<SearchPickerEntry<T>>,
    visible_sections: Vec<SearchPickerSection>,
    selected_index: Option<IndexPath>,
}

impl<T> SearchPickerDelegate<T> {
    fn new(mut entries: Vec<SearchPickerEntry<T>>) -> Self {
        entries.sort_by(|left, right| {
            left.category
                .to_lowercase()
                .cmp(&right.category.to_lowercase())
                .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
        });
        let visible_sections = Self::matching_sections(&entries, "");
        Self {
            entries,
            visible_sections,
            selected_index: None,
        }
    }

    fn matching_sections(
        entries: &[SearchPickerEntry<T>],
        query: &str,
    ) -> Vec<SearchPickerSection> {
        let mut sections = Vec::<SearchPickerSection>::new();
        for (index, entry) in entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.matches(query))
        {
            if sections
                .last()
                .is_none_or(|section| section.category != entry.category)
            {
                sections.push(SearchPickerSection {
                    category: entry.category.clone(),
                    entry_indices: Vec::new(),
                });
            }
            sections
                .last_mut()
                .expect("a category section was created")
                .entry_indices
                .push(index);
        }
        sections
    }

    fn has_visible_entries(&self) -> bool {
        self.visible_sections
            .iter()
            .any(|section| !section.entry_indices.is_empty())
    }

    fn selected_value(&self) -> Option<&T> {
        let selected = self.selected_index?;
        let entry_index = self
            .visible_sections
            .get(selected.section)?
            .entry_indices
            .get(selected.row)?;
        self.entries.get(*entry_index).map(|entry| &entry.value)
    }
}

impl<T: Clone + 'static> ListDelegate for SearchPickerDelegate<T> {
    type Item = ListItem;

    fn sections_count(&self, _: &App) -> usize {
        self.visible_sections.len()
    }

    fn items_count(&self, section: usize, _: &App) -> usize {
        self.visible_sections
            .get(section)
            .map_or(0, |section| section.entry_indices.len())
    }

    fn render_item(
        &self,
        ix: IndexPath,
        _: &mut Window,
        _: &mut Context<List<Self>>,
    ) -> Option<Self::Item> {
        let entry_index = *self
            .visible_sections
            .get(ix.section)?
            .entry_indices
            .get(ix.row)?;
        let entry = self.entries.get(entry_index)?;
        Some(
            ListItem::new(ix).h(px(28.)).px_2().py_0().text_sm().child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(entry.label.clone()),
            ),
        )
    }

    fn render_section_header(
        &self,
        section: usize,
        _: &mut Window,
        cx: &mut Context<List<Self>>,
    ) -> Option<impl IntoElement> {
        let category = self.visible_sections.get(section)?.category.clone();
        (!category.is_empty()).then(|| {
            div()
                .w_full()
                .h(px(24.))
                .flex()
                .items_end()
                .px_2()
                .pb_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(category)
        })
    }

    fn render_empty(&self, _: &mut Window, cx: &mut Context<List<Self>>) -> impl IntoElement {
        div()
            .px_3()
            .py_6()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child("該当する候補がありません")
    }

    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        _: &mut Context<List<Self>>,
    ) -> Task<()> {
        self.visible_sections = Self::matching_sections(&self.entries, query);
        self.selected_index = self.has_visible_entries().then(IndexPath::default);
        Task::ready(())
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<List<Self>>,
    ) {
        self.selected_index = ix;
    }
}

type ConfirmHandler<T> = Rc<dyn Fn(T, &mut Window, &mut App)>;

fn claim_confirmation(confirmed: &mut bool) -> bool {
    if *confirmed {
        return false;
    }
    *confirmed = true;
    true
}

pub(crate) struct SearchPicker<T: Clone + 'static> {
    list: Entity<List<SearchPickerDelegate<T>>>,
    on_confirm: ConfirmHandler<T>,
    confirmed: bool,
    _list_subscription: Subscription,
}

impl<T: Clone + 'static> SearchPicker<T> {
    pub(crate) fn new(
        entries: Vec<SearchPickerEntry<T>>,
        placeholder: impl Into<SharedString>,
        on_confirm: impl Fn(T, &mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let list = cx.new(|cx| {
            List::new(SearchPickerDelegate::new(entries), window, cx)
                .max_h(px(360.))
                .paddings(Edges::all(px(4.)))
        });
        if list.read(cx).delegate().has_visible_entries() {
            list.update(cx, |list, cx| {
                list.set_selected_index(Some(IndexPath::default()), window, cx);
            });
        }
        if let Some(input) = list.read(cx).query_input().cloned() {
            input.update(cx, |input, cx| {
                input.set_placeholder(placeholder, window, cx);
            });
        }
        let list_subscription = cx.subscribe_in(
            &list,
            window,
            |this, list, event: &ListEvent, window, cx| match event {
                ListEvent::Confirm(_) => {
                    let value = list.read(cx).delegate().selected_value().cloned();
                    if let Some(value) = value
                        && claim_confirmation(&mut this.confirmed)
                    {
                        (this.on_confirm)(value, window, cx);
                        cx.emit(DismissEvent);
                    }
                }
                ListEvent::Cancel => cx.emit(DismissEvent),
                ListEvent::Select(_) => {}
            },
        );
        Self {
            list,
            on_confirm: Rc::new(on_confirm),
            confirmed: false,
            _list_subscription: list_subscription,
        }
    }
}

impl<T: Clone + 'static> EventEmitter<DismissEvent> for SearchPicker<T> {}

impl<T: Clone + 'static> gpui::Focusable for SearchPicker<T> {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.list.focus_handle(cx)
    }
}

impl<T: Clone + 'static> Render for SearchPicker<T> {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(320.)).child(self.list.clone())
    }
}
