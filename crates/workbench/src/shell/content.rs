//! Content cards and saved-spec controls keep the source editor's coordinates unchanged.
use super::*;
use crate::app::content::{Content, Filter};
use gpui_kit::component::{Disableable as _, Selectable as _, text::TextView};
use gpui_kit::prelude::FluentBuilder as _;
use std::sync::Arc;

pub(super) struct ContentView {
    pub model: Option<Arc<Content>>,
    pub workspace: Option<WeakEntity<Workspace>>,
    pub spec: bool,
    filter: Filter,
    rendered: bool,
    selected: usize,
    cursor: Option<usize>,
    pub(super) cursor_subscription: Option<Subscription>,
    list: ListState,
}
impl Default for ContentView {
    fn default() -> Self {
        Self {
            model: None,
            workspace: None,
            spec: false,
            filter: Filter::All,
            rendered: false,
            selected: 0,
            cursor: None,
            cursor_subscription: None,
            list: ListState::new(0, ListAlignment::Top, px(200.)),
        }
    }
}
impl ContentView {
    pub(super) fn follow_source(&mut self, key: &FileKey, offset: usize) {
        let Some(model) = &self.model else {
            return;
        };
        let item = model
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| {
                let span = item.origin.span();
                (FileKey::Disk(span.file) == *key && span.range.contains(&offset)).then_some((i, span.range.len()))
            })
            .min_by_key(|(_, len)| *len)
            .map(|(i, _)| i);
        if let Some(item) = item {
            if !model.visible(self.filter).iter().any(|(i, _)| *i == item) {
                self.filter = Filter::All;
                self.list.reset(model.items.len());
            }
            if let Some(index) = model.visible(self.filter).iter().position(|(i, _)| *i == item) {
                self.selected = index;
                self.list.scroll_to_reveal_item(index);
            }
        }
    }

    fn set_filter(&mut self, filter: Filter) {
        let previous = self
            .model
            .as_ref()
            .and_then(|m| m.visible(self.filter).get(self.selected).copied())
            .map(|(i, _)| i);
        self.filter = filter;
        let visible = self.model.as_ref().map(|m| m.visible(filter)).unwrap_or_default();
        self.selected = previous
            .and_then(|item| visible.iter().position(|(i, _)| *i == item))
            .unwrap_or(0);
        self.list.reset(visible.len());
        if !visible.is_empty() {
            self.list.scroll_to_reveal_item(self.selected);
        }
    }

    pub(super) fn set(&mut self, model: Option<Arc<Content>>) {
        if self.model == model {
            return;
        }
        self.model = model;
        if !self.model.as_ref().is_some_and(|m| m.assessed) {
            self.filter = Filter::All;
        }
        self.list
            .reset(self.model.as_ref().map_or(0, |m| m.visible(self.filter).len()));
        self.selected = 0;
    }

    fn step(&mut self, forward: bool) {
        let count = self.model.as_ref().map_or(0, |m| m.visible(self.filter).len());
        self.selected = if forward {
            self.selected.saturating_add(1).min(count.saturating_sub(1))
        } else {
            self.selected.saturating_sub(1)
        };
        self.list.scroll_to_reveal_item(self.selected);
    }
}
impl DocumentPanel {
    pub(super) fn observe_cursor(&mut self, cx: &mut Context<'_, Self>) {
        self.content.cursor_subscription = Some(cx.observe(&self.editor, |p, editor, cx| {
            let pos = editor.read(cx).cursor();
            if p.content.cursor == Some(pos) || p.document.is_dirty() {
                return;
            }
            p.content.cursor = Some(pos);
            p.content.follow_source(&p.key, pos);
            cx.notify();
        }));
    }

    pub(super) fn content_view(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let Some(model) = &self.content.model else {
            return Editor::new(&self.editor)
                .readonly(true)
                .bordered(false)
                .h_full()
                .into_any_element();
        };
        let controls = h_flex()
            .gap_2()
            .child(
                Button::new("source-view")
                    .label("Source + Items")
                    .selected(!self.content.rendered)
                    .on_click(cx.listener(|p, _, _, cx| {
                        p.content.rendered = false;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("rendered-view")
                    .label("Rendered")
                    .selected(self.content.rendered)
                    .on_click(cx.listener(|p, _, _, cx| {
                        p.content.rendered = true;
                        cx.notify();
                    })),
            );
        if self.content.rendered {
            let text = model
                .rendered
                .clone()
                .unwrap_or_else(|| "Assessment unavailable; see Problems.".into());
            return v_flex()
                .size_full()
                .child(controls)
                .child(
                    div()
                        .id("rendered-scroll")
                        .flex_1()
                        .overflow_y_scroll()
                        .p_4()
                        .child(TextView::markdown("rendered-content", text)),
                )
                .into_any_element();
        }
        let filters = h_flex().gap_2().children(
            [Filter::All, Filter::Kept, Filter::Dropped]
                .into_iter()
                .enumerate()
                .map(|(i, filter)| {
                    Button::new(("filter", i))
                        .label(filter.to_string())
                        .selected(filter == self.content.filter)
                        .disabled(filter != Filter::All && !model.assessed)
                        .on_click(cx.listener(move |p, _, _, cx| {
                            p.content.set_filter(filter);
                            cx.notify();
                        }))
                }),
        );
        let navigation = h_flex()
            .gap_2()
            .child(
                Button::new("previous-item")
                    .label("Previous item")
                    .on_click(cx.listener(|p, _, _, cx| {
                        p.content.step(false);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("next-item")
                    .label("Next item")
                    .on_click(cx.listener(|p, _, _, cx| {
                        p.content.step(true);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("reveal-item")
                    .label("Open source")
                    .on_click(cx.listener(|p, _, window, cx| {
                        let Some(model) = &p.content.model else {
                            return;
                        };
                        let visible = model.visible(p.content.filter);
                        let Some((i, _)) = visible.get(p.content.selected) else {
                            return;
                        };
                        let span = model.items[*i].origin.span();
                        if let Some(workspace) = p.content.workspace.clone() {
                            window.defer(cx, move |window, cx| {
                                let _ = workspace.update(cx, |ws, cx| {
                                    ws.open(FileKey::Disk(span.file), Location::Bytes(span.range), window, cx)
                                });
                            });
                        }
                    })),
            );
        let visible = model.visible(self.content.filter);
        let model = model.clone();
        let selected = self.content.selected;
        let workspace = self.content.workspace.clone();
        let panel = cx.entity().downgrade();
        let cards = list(self.content.list.clone(), move |index, _, cx| {
            let (i, context) = visible[index];
            let item = &model.items[i];
            let span = item.origin.span();
            let workspace = workspace.clone();
            let panel = panel.clone();
            let name = span.file.file_name().unwrap_or(span.file.as_os_str()).to_string_lossy();
            let position = workspace.as_ref().and_then(WeakEntity::upgrade).and_then(|ws| {
                ws.read(cx)
                    .index
                    .entries
                    .get(&FileKey::Disk(span.file.clone()))
                    .and_then(|e| e.text.as_ref())
                    .map(|text| Location::Bytes(span.range.clone()).position(text))
            });
            let label = position.map_or_else(
                || name.to_string(),
                |p| format!("{name}:{}:{}", p.line + 1, p.character + 1),
            );
            v_flex()
                .gap_2()
                .p_3()
                .border_b_1()
                .child(
                    div()
                        .h(px(24.))
                        .child(if selected == index { "Current item" } else { "" }),
                )
                .when(context, |view| view.child("Context heading"))
                .child(
                    h_flex().flex_wrap().gap_2().children(
                        item.chips()
                            .into_iter()
                            .map(|chip| div().px_2().border_1().rounded_md().child(chip)),
                    ),
                )
                .child(TextView::markdown(("item", i), item.text.clone()))
                .child(Button::new(("source", i)).label(label).on_click(move |_, window, cx| {
                    let _ = panel.update(cx, |p, cx| {
                        p.content.selected = index;
                        cx.notify();
                    });
                    if let Some(workspace) = &workspace {
                        let _ = workspace.update(cx, |ws, cx| {
                            ws.open(
                                FileKey::Disk(span.file.clone()),
                                Location::Bytes(span.range.clone()),
                                window,
                                cx,
                            )
                        });
                    }
                }))
                .into_any_element()
        })
        .flex_1()
        .min_h_0();
        v_flex()
            .size_full()
            .child(controls)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .w_1_2()
                            .min_w_0()
                            .h_full()
                            .child(Editor::new(&self.editor).readonly(true).bordered(false).h_full()),
                    )
                    .child(
                        v_flex()
                            .w_1_2()
                            .min_w_0()
                            .h_full()
                            .child(filters)
                            .child(navigation)
                            .child(cards),
                    ),
            )
            .into_any_element()
    }
}

impl Workspace {
    pub(super) fn activate_spec(&mut self, path: Option<PathBuf>, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.active_spec = path;
        self.open_spec_targets = self.active_spec.is_some();
        self.generation += 1;
        for panel in self.docs.values() {
            panel.update(cx, |p, cx| {
                p.content.set(None);
                cx.notify();
            });
        }
        self.rescan(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::ContentView;
    use crate::app::{
        content::{Content, ContentItem, Filter, Outcome},
        index::FileKey,
    };
    use pretty_assertions::assert_eq;
    use resume::diagnostics::ItemOrigin;
    use std::sync::Arc;

    fn model() -> Arc<Content> {
        Arc::new(Content {
            problems: vec![],
            rendered: None,
            assessed: true,
            items: (0..3)
                .map(|i| ContentItem {
                    origin: ItemOrigin::Baseline {
                        file: "r.md".into(),
                        range: i * 10..i * 10 + 5,
                    },
                    annotations: Default::default(),
                    inherited_tags: Default::default(),
                    group_tags: Default::default(),
                    depth: 0,
                    text: i.to_string(),
                    outcome: if i == 1 { Outcome::NotChosen } else { Outcome::Kept },
                    promoted: None,
                    mandatory: false,
                })
                .collect(),
        })
    }
    #[test]
    fn filtering_preserves_selected_identity_and_hidden_source_reveals_all() {
        let mut view = ContentView::default();
        view.set(Some(model()));
        view.selected = 2;
        view.set_filter(Filter::Kept);
        assert_eq!(view.selected, 1);
        view.follow_source(&FileKey::Disk("r.md".into()), 11);
        assert_eq!(view.filter, Filter::All);
        assert_eq!(view.selected, 1);
        view.set_filter(Filter::Kept);
        assert_eq!(view.selected, 0);
    }
}
