//! Dock state persistence and restoration.
use super::*;

impl Workspace {
    pub(super) fn prune_closed(&mut self, cx: &App) {
        self.docs
            .retain(|_, panel| self.dock.read(cx).panel(PanelId::from(panel.entity_id())).is_some());
    }

    pub(super) fn save_layout(&mut self, cx: &mut Context<'_, Self>) {
        self.prune_closed(cx);
        if self.transition.is_some() {
            return;
        }
        let Some(path) = self.layout_path.clone() else {
            return;
        };
        let state = self.dock.read(cx).dump(cx);
        if self.last_layout.as_ref() == Some(&state) || self.save.is_some() {
            return;
        }
        let generation = self.generation;
        let saved = state.clone();
        let save = cx.background_spawn(async move { layout::save(&path, &state) });
        self.save = Some(cx.spawn(async move |weak, cx| {
            let result = save.await;
            let _ = weak.update(cx, |this, cx| {
                this.save = None;
                if this.generation != generation {
                    return;
                }
                if let Err(error) = result {
                    this.status = format!("Layout not saved: {}", snafu::Report::from_error(error));
                    cx.notify();
                } else {
                    this.last_layout = Some(saved);
                }
            });
        }));
    }

    pub(super) fn restore_layout(&mut self, state: DockAreaState, window: &mut Window, cx: &mut Context<'_, Self>) {
        if state.version != Some(1) {
            self.status = "Unsupported saved layout version; using default".into();
            return;
        }
        for (name, panel) in [("Files", self.files.clone()), ("Problems", self.problems.clone())] {
            register_panel(cx, name, move |_, _, _| panel_handle(panel.clone()));
        }
        let mut panels = BTreeMap::new();
        pub(super) fn keys(state: &PanelState, out: &mut Vec<FileKey>) {
            if state.panel_name == "Document"
                && let PanelInfo::Panel(value) = &state.info
                && let Ok(key) = serde_json::from_value(value.clone())
            {
                out.push(key);
            }
            for child in &state.children {
                keys(child, out);
            }
        }
        let mut paths = vec![];
        keys(&state.center, &mut paths);
        for dock in [&state.left_dock, &state.right_dock, &state.bottom_dock]
            .into_iter()
            .flatten()
        {
            keys(dock.panel(), &mut paths);
        }
        for key in paths {
            if !self.index.entries.contains_key(&key) {
                let editor = cx.new(|cx| EditorState::new(window, cx));
                let panel = cx.new(|_| DocumentPanel {
                    content: content::ContentView::default(),
                    title: format!("{} (unavailable)", key_label(&key)),
                    key: key.clone(),
                    document: Document::unavailable(),
                    discard_allowed: false,
                    editor,
                });
                self.docs.insert(key.clone(), panel);
            }
            self.open(key.clone(), Location::File, window, cx);
            if let Some(panel) = self.docs.get(&key) {
                panels.insert(key, panel.clone());
            }
        }
        let panels: BTreeMap<_, _> = panels
            .into_iter()
            .map(|(key, panel)| (key, panel.downgrade()))
            .collect();
        let panels = Rc::new(panels);
        register_panel(cx, "Document", move |context, window, cx| {
            let key: Option<FileKey> = match context.info() {
                PanelInfo::Panel(value) => serde_json::from_value(value.clone()).ok(),
                _ => None,
            };
            if let Some(panel) = key
                .as_ref()
                .and_then(|key| panels.get(key))
                .and_then(WeakEntity::upgrade)
            {
                return panel_handle(panel.clone());
            }
            let editor = cx.new(|cx| EditorState::new(window, cx).default_value("File unavailable; see Problems"));
            panel_handle(cx.new(|_| DocumentPanel {
                content: content::ContentView::default(),
                title: "Unavailable document".into(),
                key: key.unwrap_or_else(|| FileKey::Disk(PathBuf::new())),
                document: Document::new("File unavailable; see Problems".into()),
                discard_allowed: false,
                editor,
            }))
        });
        match self.dock.update(cx, |area, cx| area.load(state, window, cx)) {
            Ok(()) => self.last_layout = Some(self.dock.read(cx).dump(cx)),
            Err(error) => self.status = format!("Could not restore dock: {error}"),
        }
    }
}
