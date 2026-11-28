//! Theme-dependent decoration state; parsing is serialized per open style tab.
use super::panels::DocumentPanel;
use crate::app::typst_tokens::{Token, tokens};
use gpui_kit::component::{
    ActiveTheme as _, Theme,
    input::{TextDecoration, TextDecorationCollection},
};
use gpui_kit::{App, AppContext as _, Context, Subscription, Task};
use std::sync::Arc;
use typst_syntax::Tag;

pub(super) struct Highlight {
    pub(super) collection: TextDecorationCollection,
    pub(super) tokens: Vec<Token>,
    task: Option<Task<()>>,
    pending: Option<Arc<str>>,
    _theme: Subscription,
}

impl Highlight {
    pub(super) fn new(collection: TextDecorationCollection, cx: &mut Context<'_, DocumentPanel>) -> Self {
        Self {
            collection,
            tokens: vec![],
            task: None,
            pending: None,
            _theme: cx.observe_global::<Theme>(|panel, cx| panel.paint(cx)),
        }
    }
}

impl DocumentPanel {
    pub(super) fn rehighlight(&mut self, cx: &mut Context<'_, Self>) {
        let Some(highlight) = &mut self.highlight else { return };
        highlight.tokens.clear();
        highlight.collection.clear(cx);
        let text: Arc<str> = self.editor.read(cx).value().to_string().into();
        if highlight.task.is_some() {
            // Dropping a future cannot interrupt synchronous parsing; retain only the latest waiting text.
            highlight.pending = Some(text);
        } else {
            self.start_highlight(text, cx);
        }
    }

    fn start_highlight(&mut self, text: Arc<str>, cx: &mut Context<'_, Self>) {
        let snapshot = text.clone();
        let work = cx.background_spawn(async move { tokens(&snapshot) });
        let task = cx.spawn(async move |weak, cx| {
            let tokens = work.await;
            let _ = weak.update(cx, |panel, cx| panel.accept_highlight(text, tokens, cx));
        });
        if let Some(highlight) = &mut self.highlight {
            highlight.task = Some(task);
        }
    }

    pub(super) fn accept_highlight(&mut self, text: Arc<str>, tokens: Vec<Token>, cx: &mut Context<'_, Self>) {
        let Some(highlight) = &mut self.highlight else { return };
        highlight.task = None;
        if self.editor.read(cx).value().to_string().as_str() == text.as_ref() {
            highlight.tokens = tokens;
            highlight.pending = None;
            self.paint(cx);
        } else if let Some(next) = highlight.pending.take() {
            self.start_highlight(next, cx);
        }
    }

    pub(super) fn paint(&self, cx: &mut App) {
        let Some(highlight) = &self.highlight else { return };
        let theme = &cx.theme().highlight_theme;
        let decorations = highlight
            .tokens
            .iter()
            .filter_map(|token| {
                Some(TextDecoration::new(
                    token.range.clone(),
                    theme.style(theme_name(token.tag)?)?,
                ))
            })
            .collect();
        highlight.collection.set(decorations, cx);
    }
}

pub(super) fn theme_name(tag: Tag) -> Option<&'static str> {
    Some(match tag {
        Tag::Comment => "comment",
        Tag::Keyword => "keyword",
        Tag::String | Tag::Raw => "string",
        Tag::Number => "number",
        Tag::Function => "function",
        // The default themes omit operator and label; these names have colours in both palettes.
        Tag::Operator | Tag::MathOperator => "property",
        Tag::Label | Tag::Ref => "constant",
        Tag::Heading => "title",
        Tag::Strong => "emphasis.strong",
        Tag::Emph => "emphasis",
        Tag::Link => "link_uri",
        Tag::Escape => "string.escape",
        // Dark's variable slot is empty; property gives interpolated names a visible colour in both palettes.
        Tag::Interpolated => "property",
        Tag::Error
        | Tag::Punctuation
        | Tag::ListMarker
        | Tag::ListTerm
        | Tag::MathDelimiter
        | Tag::MathGroupingParens => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::theme_name;
    use gpui_kit::component::highlighter::HighlightTheme;
    use typst_syntax::Tag;

    #[gpui_kit::test]
    fn test_running_parse_is_retained_and_only_latest_text_waits(cx: &mut gpui_kit::TestAppContext) {
        use claims::{assert_ok, assert_some};
        use gpui_kit::AppContext as _;
        cx.update(gpui_kit::init);
        let (window, _) = cx.update(|cx| {
            assert_ok!(gpui_kit::open_window(
                Default::default(),
                cx,
                crate::shell::Workspace::new
            ))
        });
        let panel = assert_ok!(cx.update_window(window, |_, window, cx| {
            let panel = super::DocumentPanel::new(
                crate::app::index::FileKey::Disk("style.typ".into()),
                "style".into(),
                crate::app::document::Document::new("#let first = 1".into()),
                window,
                cx,
            );
            panel.update(cx, |p, cx| {
                let running = assert_some!(&p.highlight).task.as_ref().map(|task| task as *const _);
                for text in ["// second", "#let third = 3", "#let first = 1"] {
                    p.document.edit(text.into());
                    p.sync(window, cx);
                    let h = assert_some!(&p.highlight);
                    assert_eq!(h.task.as_ref().map(|task| task as *const _), running);
                    assert_eq!(h.pending.as_deref(), Some(text));
                }
            });
            panel
        }));
        cx.run_until_parked();
        cx.update(|cx| {
            let h = assert_some!(&panel.read(cx).highlight);
            assert!(h.task.is_none());
            assert!(h.pending.is_none());
            assert_eq!(h.tokens, crate::app::typst_tokens::tokens("#let first = 1"));
        });
    }

    #[test]
    fn test_every_mapped_tag_has_a_style_in_both_default_themes() {
        for theme in [HighlightTheme::default_light(), HighlightTheme::default_dark()] {
            for tag in Tag::LIST {
                if let Some(name) = theme_name(*tag) {
                    assert!(theme.style(name).is_some(), "{tag:?}: {name}");
                }
            }
        }
    }
}
