//! Dock panels render shared workspace state and forward user actions.
use super::*;

pub(super) struct Row {
    pub(super) label: String,
    pub(super) target: FileKey,
    pub(super) location: Location,
}
pub(super) struct RowsPanel {
    name: &'static str,
    pub(super) rows: Vec<Row>,
    focus: FocusHandle,
    pub(super) workspace: Option<WeakEntity<Workspace>>,
}
impl RowsPanel {
    pub(super) fn new(name: &'static str, cx: &mut Context<'_, Self>) -> Self {
        Self {
            name,
            rows: vec![],
            focus: cx.focus_handle(),
            workspace: None,
        }
    }
}
impl EventEmitter<PanelEvent> for RowsPanel {}
impl Focusable for RowsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl BasePanel for RowsPanel {
    fn panel_name(&self) -> &'static str {
        self.name
    }
    fn closable(&self, _: &App) -> bool {
        false
    }
}
impl Panel for RowsPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        self.name
    }
}
impl Render for RowsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let entity = cx.entity();
        uniform_list(self.name, self.rows.len(), move |range, _, cx| {
            let this = entity.read(cx);
            range
                .map(|i| {
                    let row = &this.rows[i];
                    let workspace = this.workspace.clone();
                    let key = row.target.clone();
                    let location = row.location.clone();
                    div()
                        .h(px(32.))
                        .child(
                            Button::new(("row", i))
                                .label(row.label.clone())
                                .on_click(move |_, window, cx| {
                                    if let Some(workspace) = &workspace {
                                        let _ = workspace
                                            .update(cx, |ws, cx| ws.open(key.clone(), location.clone(), window, cx));
                                    }
                                }),
                        )
                })
                .collect()
        })
        .size_full()
    }
}

pub(super) struct DocumentPanel {
    pub(super) key: FileKey,
    pub(super) title: String,
    pub(super) document: Document,
    pub(super) discard_allowed: bool,
    pub(super) editor: Entity<EditorState>,
    pub(super) highlight: Option<super::highlight::Highlight>,
}
impl DocumentPanel {
    pub(super) fn new(
        key: FileKey,
        title: String,
        document: Document,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let path = match &key {
            FileKey::Disk(path) | FileKey::Builtin(path) => path,
        };
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let language = match extension.as_str() {
            "md" => "markdown",
            "toml" => "toml",
            _ => "text",
        };
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language)
                .line_number(true)
                .soft_wrap(extension != "typ")
                .default_value(document.text().to_owned())
        });
        let panel = cx.new(|cx| {
            let highlight = (extension == "typ").then(|| {
                let collection = editor.update(cx, |editor, cx| editor.create_decorations_collection(vec![], cx));
                super::highlight::Highlight::new(collection, cx)
            });
            Self {
                key,
                title,
                document,
                discard_allowed: false,
                editor,
                highlight,
            }
        });
        panel.update(cx, |panel, cx| panel.rehighlight(cx));
        panel
    }
    pub(super) fn protected(&self) -> bool {
        self.document.is_dirty() && !self.discard_allowed
    }
    pub(super) fn sync(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let before = self.editor.read(cx).value().to_string();
        if before != self.document.text() && !self.document.has_conflict() {
            self.editor
                .update(cx, |e, cx| e.set_value(self.document.text().to_owned(), window, cx));
            self.rehighlight(cx);
        }
        cx.notify();
    }
}
impl EventEmitter<PanelEvent> for DocumentPanel {}
impl Focusable for DocumentPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.focus_handle(cx)
    }
}
impl BasePanel for DocumentPanel {
    fn panel_name(&self) -> &'static str {
        "Document"
    }
    fn dump(&self, _: &App) -> PanelState {
        PanelState {
            panel_name: "Document".into(),
            children: vec![],
            info: PanelInfo::Panel(serde_json::to_value(&self.key).expect("file key serializes")),
        }
    }
    fn closable(&self, _: &App) -> bool {
        !self.protected()
    }
}
impl Panel for DocumentPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        self.title.clone()
    }
}
impl Render for DocumentPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let mut view = v_flex().size_full();
        if self.document.has_conflict() || self.document.is_dirty() {
            view = view.child(
                h_flex()
                    .child("Changed on disk")
                    .child(
                        Button::new("reload")
                            .label("Reload")
                            .on_click(cx.listener(|p, _, w, cx| {
                                p.document.take_disk();
                                p.sync(w, cx);
                            })),
                    )
                    .child(
                        Button::new("keep")
                            .label("Keep mine")
                            .on_click(cx.listener(|p, _, _, cx| {
                                p.document.keep_mine();
                                cx.notify();
                            })),
                    ),
            );
            view = view.child(
                Button::new("discard-retained")
                    .label("Allow discarding retained text on close")
                    .on_click(cx.listener(|p, _, _, cx| {
                        p.discard_allowed = true;
                        cx.notify();
                    })),
            );
        }
        view.child(Editor::new(&self.editor).readonly(true).bordered(false).h_full())
    }
}
