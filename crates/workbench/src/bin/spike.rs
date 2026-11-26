//! ABOUTME: Throwaway spike (rsm-yjj.1): proves GPUI, the dock, and the code editor run, before the app is built on them.
//! ABOUTME: Grows step by step; each step is one of the bead's acceptance points.

use gpui_kit::component::dock::{
    BasePanel, DockArea, DockLayout, DockPlacement, DockSkin, Panel, PanelEvent, PanelId, panel_handle,
};

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr as _;

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{
    DefinitionProvider, Editor, EditorState, RangeDecoration, RangeDecorationStyle, Rope, TextDecoration,
};
use gpui_kit::component::label::Label;
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::*;
use resume::model::StyleName;
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Side, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World, WorldExt};
use typst_ide::{Definition, IdeWorld};
use typst_layout::PagedDocument;

const BASELINE: &str = include_str!("../../spike-fixtures/baseline.md");
const STYLE_TYP: &str = include_str!("../../spike-fixtures/style.typ");
const COMPONENTS_TYP: &str = include_str!("../../spike-fixtures/components.typ");
/// The style file's path inside the world, as typst sees it.
const STYLE_FILE: &str = "/pragmatic/style.typ";

/// The highlight name the editor's theme knows for a Typst tag, or `None` for one the spike leaves uncoloured.
///
/// The names are the vocabulary of the tree-sitter highlight theme (`keyword`, `string`, ...); the editor resolves them
/// against the active theme at paint time, so a theme switch recolours without any work here.
fn theme_name(tag: typst_syntax::Tag) -> Option<&'static str> {
    use typst_syntax::Tag;
    Some(match tag {
        Tag::Comment => "comment",
        Tag::Keyword => "keyword",
        Tag::String => "string",
        Tag::Number => "number",
        Tag::Function => "function",
        // The default light theme has no colour for `operator`, `label` or `error`, and an uncoloured token is dropped
        // without a message, so these borrow names it does colour (`property`, `constant`, `type`) (found by a test, rsm-yjj.1).
        Tag::Operator => "property",
        Tag::Label | Tag::Ref => "constant",
        Tag::Heading => "title",
        Tag::Strong => "emphasis.strong",
        Tag::Emph => "emphasis",
        Tag::Link => "link_uri",
        Tag::Raw => "string",
        Tag::Escape => "string.escape",
        Tag::Interpolated => "variable",
        Tag::Error => "type",
        Tag::Punctuation
        | Tag::ListMarker
        | Tag::ListTerm
        | Tag::MathDelimiter
        | Tag::MathOperator
        | Tag::MathGroupingParens => return None,
    })
}

/// Every highlighted leaf of `text` as (byte range, theme name), sorted by start. Leaves never overlap.
fn typst_tokens(text: &str) -> Vec<(Range<usize>, &'static str)> {
    fn walk(node: &typst_syntax::LinkedNode<'_>, out: &mut Vec<(Range<usize>, &'static str)>) {
        if let Some(name) = typst_syntax::highlight(node).and_then(theme_name) {
            out.push((node.range(), name));
            return;
        }
        for child in node.children() {
            walk(&child, out);
        }
    }
    let root = typst_syntax::parse(text);
    let mut out = Vec::new();
    walk(&typst_syntax::LinkedNode::new(&root), &mut out);
    out
}

/// What an annotated item says about its priority, in the one way the spike colours it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Core,
    Extra,
}

/// The byte range of each `<!-- ... -->` comment in `text` that carries `@core` or `@extra`, with its mark.
///
/// Spike only: the real app reads the parsed model (`Annotations`) and its node ranges, not the text. The published
/// gpui-component 0.11.0 has no per-row decoration (it is on the repository's main branch only), so the spike paints
/// a range fill over the comment, which is also what a chip in the content view needs.
fn marked_comments(text: &str) -> Vec<(Range<usize>, Mark)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find("<!--") {
        let start = from + open;
        let Some(close) = text[start..].find("-->") else { break };
        let end = start + close + "-->".len();
        let words: Vec<&str> = text[start + 4..end - 3].split_whitespace().collect();
        if words.contains(&"@core") {
            found.push((start..end, Mark::Core));
        } else if words.contains(&"@extra") {
            found.push((start..end, Mark::Extra));
        }
        from = end;
    }
    found
}

/// A minimal Typst `World` over the built-in style files, so `typst_ide` can resolve a name to where it is defined.
///
/// Spike only. The real app needs `resume`'s own world (the same files, `/resume.json`, fonts); that type is
/// `pub(crate)` today, which is what rsm-yjj.3 opens up. This one has no fonts and an empty library, which is enough to
/// ask where something is defined.
struct StyleWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    main: FileId,
    files: HashMap<FileId, String>,
}

impl StyleWorld {
    /// The built-in style `name`: its own files and the shared `lib/`, all under `/`, with `main` as the entry.
    fn builtin(name: &str, main: &str) -> Self {
        let style = StyleName::try_new(name.to_string()).expect("a built-in style name is valid");
        let mut files = HashMap::new();
        for (path, bytes) in resume::style::builtin_files(&style).expect("the style is built in") {
            let slash = format!("/{}", path.to_string_lossy().replace('\\', "/"));
            let id = RootedPath::new(
                VirtualRoot::Project,
                VirtualPath::new(&slash).expect("a style path is valid"),
            )
            .intern();
            files.insert(id, String::from_utf8_lossy(bytes).into_owned());
        }
        let main = RootedPath::new(VirtualRoot::Project, VirtualPath::new(main).expect("main is valid")).intern();
        Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(FontBook::new()),
            main,
            files,
        }
    }
}

impl World for StyleWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }
    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }
    fn main(&self) -> FileId {
        self.main
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        self.files
            .get(&id)
            .map(|text| Source::new(id, text.clone()))
            .ok_or_else(|| FileError::NotFound(id.vpath().get_with_slash().into()))
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files
            .get(&id)
            .map(|text| Bytes::new(text.clone().into_bytes()))
            .ok_or_else(|| FileError::NotFound(id.vpath().get_with_slash().into()))
    }
    fn font(&self, _: usize) -> Option<Font> {
        None
    }
    fn today(&self, _: Option<Duration>) -> Option<Datetime> {
        None
    }
}

impl IdeWorld for StyleWorld {
    fn upcast(&self) -> &dyn World {
        self
    }
}

/// Where the name at `cursor` in `file` of the style is defined: the file and the 1-based line, if it resolves to a span.
fn definition_site(world: &StyleWorld, file: &str, cursor: usize) -> Option<(String, usize)> {
    let id = RootedPath::new(VirtualRoot::Project, VirtualPath::new(file).ok()?).intern();
    let source = world.source(id).ok()?;
    match typst_ide::definition(world, None::<&PagedDocument>, &source, cursor, Side::After)? {
        Definition::Span(span) => {
            let target = world.source(span.id()?).ok()?;
            let range = world.range(span)?;
            let line = target.lines().byte_to_line(range.start)? + 1;
            Some((target.id().vpath().get_with_slash().to_string(), line))
        }
        Definition::File(id) => Some((id.vpath().get_with_slash().to_string(), 1)),
        Definition::Std(_) => None,
    }
}

/// Offers "go to definition" for `style.typ` through the editor's own hook, using the tested `definition_site`.
///
/// The target is a `file:///lib/components.typ`-style URI plus a line; the host decides how to show it (see
/// `show_document` in `read_only_editor`). Spike only: it builds a world per request.
struct StyleDefinitions;

impl DefinitionProvider for StyleDefinitions {
    fn definitions(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Vec<lsp_types::LocationLink>>> {
        let (cursor, text) = (offset, text.to_string());
        cx.background_spawn(async move {
            debug_assert_eq!(text, STYLE_TYP);
            let world = StyleWorld::builtin("pragmatic", STYLE_FILE);
            let Some((file, line)) = definition_site(&world, STYLE_FILE, cursor) else {
                return Ok(Vec::new());
            };
            let at = lsp_types::Range::new(
                lsp_types::Position::new(line.saturating_sub(1) as u32, 0),
                lsp_types::Position::new(line.saturating_sub(1) as u32, 0),
            );
            Ok(vec![lsp_types::LocationLink {
                origin_selection_range: None,
                target_uri: lsp_types::Uri::from_str(&format!("typst:{file}")).expect("a typst: URI is valid"),
                target_range: at,
                target_selection_range: at,
            }])
        })
    }
}

/// The PDF the spike opens: `SPIKE_PDF` if set, else the one the `resume` CLI wrote into `target/spike-out` for the spike.
fn pdf_path() -> PathBuf {
    std::env::var_os("SPIKE_PDF")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/spike-out/Resume.pdf"))
}

/// Two buttons that open the PDF in the system viewer, one through `opener` and one through GPUI, and a line that says
/// what each reported. Both are tried because the spike must find out which one works on Windows.
struct OpenPdf {
    last: SharedString,
}

impl Render for OpenPdf {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        v_flex()
            .p_3()
            .gap_2()
            .child(Label::new(format!("PDF: {}", pdf_path().display())).id("pdf-path"))
            .child(
                gpui_kit::component::h_flex()
                    .gap_2()
                    .child(
                        Button::new("open-opener")
                            .label("Open with opener")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let path = pdf_path();
                                this.last = match opener::open(&path) {
                                    Ok(()) => format!("opener::open ok: {}", path.display()),
                                    Err(e) => format!("opener::open FAILED: {e}"),
                                }
                                .into();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("open-gpui")
                            .label("Open with GPUI")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let path = pdf_path();
                                cx.open_with_system(&path);
                                this.last = format!("cx.open_with_system called: {}", path.display()).into();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("reveal")
                            .label("Reveal")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let path = pdf_path();
                                cx.reveal_path(&path);
                                this.last = format!("cx.reveal_path called: {}", path.display()).into();
                                cx.notify();
                            })),
                    ),
            )
            .child(Label::new(self.last.clone()).id("pdf-result"))
    }
}

/// One dock panel with a fixed title and a body that the caller builds.
struct SpikePanel {
    name: &'static str,
    title: &'static str,
    body: AnyView,
    focus: FocusHandle,
}

impl SpikePanel {
    fn new(name: &'static str, title: &'static str, body: impl Into<AnyView>, cx: &mut App) -> Entity<Self> {
        let body = body.into();
        cx.new(|cx| Self {
            name,
            title,
            body,
            focus: cx.focus_handle(),
        })
    }
}

impl EventEmitter<PanelEvent> for SpikePanel {}

impl Focusable for SpikePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BasePanel for SpikePanel {
    fn panel_name(&self) -> &'static str {
        self.name
    }
}

impl Panel for SpikePanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        self.title
    }
}

impl Render for SpikePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .size_full()
            .text_color(cx.theme().foreground)
            .child(self.body.clone())
    }
}

/// A body made of a title and some lines of text.
struct TextBody(Vec<&'static str>);

impl Render for TextBody {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        // `Label::new` takes its element id from the source line, so labels built in a loop need their own ids.
        v_flex().p_3().gap_1().children(
            self.0
                .iter()
                .enumerate()
                .map(|(i, line)| Label::new(*line).id(("line", i))),
        )
    }
}

/// A read-only code editor over `text`, with the given language.
struct ReadOnlyEditor {
    state: Entity<EditorState>,
}

impl Render for ReadOnlyEditor {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        Editor::new(&self.state).readonly(true).bordered(false).h_full()
    }
}

fn read_only_editor(language: &str, text: &'static str, window: &mut Window, cx: &mut App) -> Entity<ReadOnlyEditor> {
    let state = cx.new(|cx| {
        EditorState::new(window, cx)
            .language(language.to_string())
            .line_number(true)
            .soft_wrap(false)
            .default_value(text)
    });
    if language == "typst" {
        // The editor draws semantic tokens only in a code editor that has a tree-sitter highlighter, and Typst has no
        // grammar here. Text decorations paint any style on any byte range with no grammar, so they carry the colours.
        // Found in the spike (rsm-yjj.1): see the record for why the semantic-token hook is not used.
        let theme = cx.theme().highlight_theme.clone();
        let decorations: Vec<TextDecoration> = typst_tokens(text)
            .into_iter()
            .filter_map(|(range, name)| Some(TextDecoration::new(range, theme.style(name)?)))
            .collect();
        state.update(cx, |editor, cx| {
            editor.create_decorations_collection(decorations, cx);
        });
    }
    if language == "markdown" {
        let theme_colors = (cx.theme().success.opacity(0.35), cx.theme().warning.opacity(0.4));
        let fills: Vec<RangeDecoration> = marked_comments(text)
            .into_iter()
            .map(|(range, mark)| {
                let color = if mark == Mark::Core {
                    theme_colors.0
                } else {
                    theme_colors.1
                };
                RangeDecoration::new(range)
                    .with_style(RangeDecorationStyle::Fill)
                    .with_color(color)
            })
            .collect();
        state.update(cx, |editor, cx| {
            editor.create_range_decorations_collection(fills, cx);
        });
    }
    cx.new(|_| ReadOnlyEditor { state })
}

struct Spike {
    dock: Entity<DockArea>,
}

impl Spike {
    fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let (dock, _skin) = DockSkin::dock_area("spike-dock", Some(1), window, cx);
        let files = SpikePanel::new(
            "SpikeFiles",
            "Files",
            cx.new(|_| TextBody(vec!["Resumes", "  baseline.md", "Styles", "  pragmatic"])),
            cx,
        );
        let editor_view = read_only_editor("markdown", BASELINE, window, cx);
        let content = SpikePanel::new("SpikeContent", "baseline.md", editor_view, cx);
        let metadata = SpikePanel::new(
            "SpikeMetadata",
            "Metadata",
            cx.new(|_| TextBody(vec!["Profiles", "Priority", "Tags", "Wordings"])),
            cx,
        );
        let style_view = read_only_editor("typst", STYLE_TYP, window, cx);
        let components_view = read_only_editor("typst", COMPONENTS_TYP, window, cx);
        let style = SpikePanel::new("SpikeStyle", "style.typ", style_view.clone(), cx);
        let components = SpikePanel::new("SpikeComponents", "/lib/components.typ", components_view.clone(), cx);
        let components_panel = PanelId::from(components.entity_id());
        let components_state = components_view.read(cx).state.clone();
        let dock_for_jump = dock.downgrade();
        style_view.read(cx).state.clone().update(cx, |editor, _| {
            editor.lsp_mut().definition_provider = Some(Rc::new(StyleDefinitions));
            // Called before the editor's own jump. A `typst:` target is another style file, shown as another tab.
            editor.lsp_mut().show_document = Some(Rc::new(move |params, window, cx| {
                if params.uri.scheme().map(|s| s.as_str()) != Some("typst") {
                    return false;
                }
                let line = params.selection.map_or(0, |range| range.start.line);
                let _ = dock_for_jump.update(cx, |area, cx| area.select_panel(components_panel, window, cx));
                components_state.update(cx, |editor, cx| {
                    editor.set_cursor_position(lsp_types::Position::new(line, 0), window, cx);
                });
                true
            }));
        });
        let pdf = SpikePanel::new(
            "SpikePdf",
            "PDF",
            cx.new(|_| OpenPdf {
                last: "not opened yet".into(),
            }),
            cx,
        );
        dock.update(cx, |area, cx| {
            area.set_dock(
                DockPlacement::Bottom,
                DockLayout::tabs().panel_view(panel_handle(pdf), cx),
                window,
                cx,
            );
            area.set_dock_size(DockPlacement::Bottom, px(130.), window, cx);
            area.set_center(
                DockLayout::h_split()
                    .child(DockLayout::tabs().panel_view(panel_handle(files), cx), Some(px(200.)))
                    .child(
                        DockLayout::tabs()
                            .panel_view(panel_handle(content), cx)
                            .panel_view(panel_handle(style), cx)
                            .panel_view(panel_handle(components), cx)
                            // SPIKE_TAB=1 opens on style.typ, so a screenshot of that tab needs no mouse.
                            .active_index(std::env::var("SPIKE_TAB").ok().and_then(|v| v.parse().ok()).unwrap_or(0)),
                        None,
                    )
                    .child(
                        DockLayout::tabs().panel_view(panel_handle(metadata), cx),
                        Some(px(260.)),
                    ),
                window,
                cx,
            );
        });
        cx.new(|_| Self { dock })
    }
}

impl Render for Spike {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        div().size_full().child(self.dock.clone())
    }
}

fn main() {
    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, Spike::new).expect("open window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    // Not `super::*`: `gpui_kit::*` brings GPUI's own `test` macro, which would replace the built-in `#[test]`.
    use super::{BASELINE, Mark, STYLE_TYP, StyleWorld, definition_site, marked_comments, theme_name, typst_tokens};

    #[test]
    fn marks_only_the_comments_that_say_core_or_extra() {
        let found = marked_comments(BASELINE);
        let marks: Vec<Mark> = found.iter().map(|(_, m)| *m).collect();
        assert_eq!(marks, [Mark::Core, Mark::Extra]);
        let texts: Vec<&str> = found.iter().map(|(r, _)| &BASELINE[r.clone()]).collect();
        assert_eq!(texts, ["<!-- @core #ai -->", "<!-- @extra #data -->"]);
    }

    #[test]
    fn text_outside_a_comment_is_not_an_annotation() {
        assert!(marked_comments("prose that says @core and @extra\n").is_empty());
    }

    #[test]
    fn an_unclosed_comment_is_ignored_and_does_not_loop() {
        assert!(marked_comments("a <!-- @core never closed").is_empty());
    }

    #[test]
    fn typst_tokens_cover_the_style_with_valid_sorted_disjoint_ranges() {
        let tokens = typst_tokens(STYLE_TYP);
        assert!(tokens.len() > 20, "a real style has many tokens, got {}", tokens.len());
        let mut previous_end = 0;
        for (range, name) in &tokens {
            assert!(
                STYLE_TYP.is_char_boundary(range.start) && STYLE_TYP.is_char_boundary(range.end),
                "{name}"
            );
            assert!(range.start >= previous_end, "{range:?} overlaps the token before it");
            assert!(range.end > range.start, "empty token {name}");
            previous_end = range.end;
        }
    }

    #[test]
    fn typst_tokens_name_the_import_keyword_the_path_string_and_a_comment() {
        let tokens = typst_tokens("// note\n#import \"/lib/x.typ\": *\n");
        let names: Vec<&str> = tokens.iter().map(|(_, n)| *n).collect();
        assert!(names.contains(&"comment"), "{names:?}");
        assert!(names.contains(&"keyword"), "{names:?}");
        assert!(names.contains(&"string"), "{names:?}");
    }

    /// Every name `theme_name` returns must have a colour in the default light theme, or a token is silently dropped.
    #[test]
    fn the_default_theme_colours_every_name_a_typst_tag_maps_to() {
        use gpui_kit::component::highlighter::HighlightTheme;
        use typst_syntax::Tag;
        let theme = HighlightTheme::default_light();
        let missing: Vec<(Tag, &str)> = Tag::LIST
            .iter()
            .copied()
            .filter_map(|tag| theme_name(tag).map(|name| (tag, name)))
            .filter(|(_, name)| theme.style(name).is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "no colour in the default light theme for {missing:?}"
        );
    }

    /// The style's own entry file inside the world, as typst sees it.
    const STYLE_FILE: &str = "/pragmatic/style.typ";

    #[test]
    fn names_the_style_takes_from_the_shared_library_resolve_into_it() {
        let world = StyleWorld::builtin("pragmatic", STYLE_FILE);
        // Each of these is called in style.typ and defined by a `#let` in lib/components.typ.
        for name in [
            "index-roles(json",
            "contact-parts(resume.contact",
            "font-stack(theme",
            "render-sections(theme",
        ] {
            let use_site = STYLE_TYP
                .find(name)
                .unwrap_or_else(|| panic!("{name} is used in style.typ"));
            let (file, line) =
                definition_site(&world, STYLE_FILE, use_site + 1).unwrap_or_else(|| panic!("{name} does not resolve"));
            assert_eq!(file, "/lib/components.typ", "{name}");
            assert!(line > 1, "{name}: defined on a real line, got {line}");
        }
    }

    #[test]
    fn a_local_name_resolves_inside_the_same_file() {
        let world = StyleWorld::builtin("pragmatic", STYLE_FILE);
        // `#header(theme, resume)` near the end uses `header`, defined by `#let header` in this file.
        let use_site = STYLE_TYP.rfind("#header(").expect("header is used") + 2;
        let (file, line) = definition_site(&world, STYLE_FILE, use_site).expect("header resolves");
        assert_eq!(file, "/pragmatic/style.typ");
        assert_eq!(
            line,
            STYLE_TYP.lines().position(|l| l.starts_with("#let header(")).unwrap() + 1
        );
    }

    #[test]
    fn a_name_that_is_not_defined_anywhere_resolves_to_nothing() {
        let world = StyleWorld::builtin("pragmatic", STYLE_FILE);
        assert_eq!(definition_site(&world, STYLE_FILE, 0), None);
    }
}
