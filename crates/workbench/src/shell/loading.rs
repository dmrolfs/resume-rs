//! Folder transitions and serialized background scans.
use super::*;

impl Workspace {
    pub(super) fn close_window(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> bool {
        if self.transition.is_some() {
            return false;
        }
        let Some(path) = self.layout_path.clone() else {
            return true;
        };
        let state = self.dock.read(cx).dump(cx);
        if self.last_layout.as_ref() == Some(&state) && self.save.is_none() {
            return true;
        }
        let pending = self.save.take();
        self.transition = Some(cx.spawn_in(window, async move |weak, cx| {
            if let Some(pending) = pending {
                pending.await;
            }
            let saved = state.clone();
            let result = cx.background_spawn(async move { layout::save(&path, &state) }).await;
            let _ = weak.update_in(cx, |this, window, cx| {
                this.transition = None;
                cx.notify();
                match result {
                    Ok(()) => {
                        this.last_layout = Some(saved);
                        this.prune_closed(cx);
                        if this.docs.values().any(|p| p.read(cx).protected()) {
                            return;
                        }
                        if this.close_window(window, cx) {
                            window.remove_window();
                        }
                    }
                    Err(error) => {
                        this.status = format!("Window kept open: {}", snafu::Report::from_error(error));
                        this.resume_pending(window, cx);
                        cx.notify();
                    }
                }
            });
        }));
        false
    }

    pub(super) fn open_folder(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.prune_closed(cx);
        if self.docs.values().any(|d| d.read(cx).protected()) {
            self.status = "Resolve unsaved document conflicts before changing folders".into();
            cx.notify();
            return;
        }
        if self.transition.is_some() {
            return;
        }
        let old_layout = self.layout_path.clone().map(|path| (path, self.dock.read(cx).dump(cx)));
        let pending = self.save.take();
        self.status = format!("Opening {}", root.display());
        cx.notify();
        self.transition = Some(cx.spawn_in(window, async move |weak, cx| {
            if let Some(pending) = pending {
                pending.await;
            }
            let saved = old_layout.as_ref().map(|(_, state)| state.clone());
            let result = cx
                .background_spawn(async move {
                    if let Some((path, state)) = old_layout {
                        layout::save(&path, &state).map_err(|source| FolderError::Layout { source })?;
                    }
                    let root = std::fs::canonicalize(&root).map_err(|source| FolderError::Resolve { source })?;
                    let watcher = FolderWatch::new(&root);
                    Ok::<_, FolderError>((root, watcher))
                })
                .await;
            let _ = weak.update_in(cx, |this, window, cx| {
                this.transition = None;
                cx.notify();
                match result {
                    Ok((root, watcher)) => {
                        this.prune_closed(cx);
                        if this.docs.values().any(|p| p.read(cx).protected()) {
                            this.resume_pending(window, cx);
                            return;
                        }
                        if saved
                            .as_ref()
                            .is_some_and(|saved| saved != &this.dock.read(cx).dump(cx))
                        {
                            this.open_folder(root, window, cx);
                        } else {
                            this.install_folder(root, watcher, window, cx);
                        }
                    }
                    Err(error) => {
                        this.status = format!("Folder unchanged: {}", snafu::Report::from_error(error));
                        this.resume_pending(window, cx);
                        cx.notify();
                    }
                }
            });
        }));
    }

    pub(super) fn install_folder(
        &mut self,
        root: PathBuf,
        watcher: Result<FolderWatch, crate::app::watch::WatchError>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        window.set_window_title(&format!("Resume Workbench: {}", root.display()));
        self.watch_error = None;
        self.watch = match watcher {
            Ok(watch) => Some(watch),
            Err(error) => {
                self.watch_error = Some(snafu::Report::from_error(error).to_string());
                None
            }
        };
        for panel in self.docs.values() {
            self.dock
                .update(cx, |area, cx| area.remove_panel(panel.clone(), window, cx));
        }
        self.docs.clear();
        self.active_spec = None;
        self.index = FileIndex::default();
        self.files.update(cx, |p, cx| {
            p.rows.clear();
            cx.notify();
        });
        self.problems.update(cx, |p, cx| {
            p.rows.clear();
            cx.notify();
        });
        self.dock
            .update(cx, |area, cx| area.set_center(DockLayout::tabs(), window, cx));
        self.generation += 1;
        self.restore = true;
        self.last_layout = None;
        self.layout_path = None;
        self.root = Some(root);
        self.rescan(window, cx);
    }

    pub(super) fn accept_scan(
        &mut self,
        generation: u64,
        index: FileIndex,
        state: Option<Result<(PathBuf, Option<DockAreaState>), layout::LayoutError>>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.scan = None;
        if self.transition.is_some() {
            self.rescan_pending = true;
            return;
        }
        if self.generation != generation || self.rescan_pending {
            self.rescan_pending = false;
            self.rescan(window, cx);
            return;
        }
        self.restore = false;
        self.apply_index(index, window, cx);
        if let Some(state) = state {
            match state {
                Ok((path, state)) => {
                    self.layout_path = Some(path);
                    if let Some(state) = state {
                        self.restore_layout(state, window, cx);
                    }
                }
                Err(error) => {
                    self.status = format!("Layout not restored: {}", snafu::Report::from_error(error));
                    cx.notify();
                }
            }
        }
        if self.rescan_pending {
            self.rescan_pending = false;
            self.rescan(window, cx);
        }
    }

    pub(super) fn rescan(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.transition.is_some() {
            self.rescan_pending = true;
            return;
        }
        if self.scan.is_some() {
            self.rescan_pending = true;
            return;
        }
        let Some(root) = self.root.clone() else {
            return;
        };
        let generation = self.generation;
        let restore = self.restore;
        let active_spec = self.active_spec.clone();
        let watcher = self.watch.clone();
        let scan = cx.background_spawn(async move {
            let state = restore.then(|| {
                let path = layout::path(&root)?;
                Ok::<_, layout::LayoutError>((path.clone(), layout::load(&path)?))
            });
            let mut index = FileIndex::scan_active(&root, active_spec.as_deref());
            if let Some(watcher) = &watcher {
                let targets = active_spec
                    .as_ref()
                    .and_then(|p| index.specs.get(p))
                    .map(|s| {
                        std::iter::once(s.content.path.clone())
                            .chain(s.letter().map(|l| l.content.path.clone()))
                            .map(|p| resume::spec::error::tidy_path(&p))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                for error in watcher.watch_content(targets) {
                    index
                        .problems
                        .push(Problem::file(&root, snafu::Report::from_error(error).to_string()));
                }
            }
            if let Some(watcher) = watcher
                && let Err(error) = watcher.refresh_dependencies()
            {
                index
                    .problems
                    .push(Problem::file(&root, snafu::Report::from_error(error).to_string()));
            }
            (index, state)
        });
        self.scan = Some(cx.spawn_in(window, async move |weak, cx| {
            let (index, state) = scan.await;
            let _ = weak.update_in(cx, |this, window, cx| {
                this.accept_scan(generation, index, state, window, cx);
            });
        }));
    }

    pub(super) fn apply_index(&mut self, index: FileIndex, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.prune_closed(cx);
        let workspace = cx.entity().downgrade();
        for (key, panel) in &self.docs {
            let disk = index
                .entries
                .get(key)
                .and_then(|e| e.text.clone())
                .map_or(DiskState::Unavailable, DiskState::Text);
            panel.update(cx, |p, cx| {
                if p.content.cursor_subscription.is_none() {
                    p.observe_cursor(cx);
                }
                p.content.workspace = Some(workspace.clone());
                p.content.spec = index.entries.get(key).is_some_and(|e| e.kind == FileKind::Specs);
                p.document.observe(disk);
                if p.document.has_conflict() {
                    p.discard_allowed = false;
                }
                p.content.set(if p.document.is_dirty() {
                    None
                } else {
                    index.content.get(key).cloned()
                });
                p.sync(window, cx);
            });
        }
        let mut files = vec![];
        let mut problems = vec![];
        for kind in [
            FileKind::Resumes,
            FileKind::Letters,
            FileKind::Specs,
            FileKind::Profiles,
            FileKind::Styles,
        ] {
            for (key, entry) in &index.entries {
                if entry.kind != kind {
                    continue;
                }
                files.push(Row {
                    label: format!(
                        "{kind}: {}{}",
                        match key {
                            FileKey::Disk(path) => self
                                .root
                                .as_ref()
                                .and_then(|root| path.strip_prefix(root).ok())
                                .unwrap_or(path)
                                .display()
                                .to_string(),
                            FileKey::Builtin(path) => format!("{} (built-in)", path.display()),
                        },
                        if entry.problems.is_empty() { "" } else { " ⚠" }
                    ),
                    target: key.clone(),
                    location: Location::File,
                });
                problems.extend(entry.problems.iter().map(problem_row));
            }
        }
        problems.extend(index.problems.iter().map(problem_row));
        if let (Some(root), Some(error)) = (&self.root, &self.watch_error) {
            problems.push(problem_row(&Problem::file(root, error.clone())));
        }
        self.status = format!(
            "{} files · {} problems{}",
            files.len(),
            problems.len(),
            if self.watch.is_some() {
                " · watching"
            } else {
                " · watcher unavailable"
            }
        );
        self.index = index;
        let targets: Vec<_> = self
            .active_spec
            .as_ref()
            .and_then(|p| self.index.specs.get(p))
            .map(|s| {
                std::iter::once(s.content.path.clone())
                    .chain(s.letter().map(|l| l.content.path.clone()))
                    .collect()
            })
            .unwrap_or_default();
        if self.open_spec_targets && !targets.is_empty() {
            self.open_spec_targets = false;
            for path in targets {
                self.open(
                    FileKey::Disk(resume::spec::error::tidy_path(&path)),
                    Location::File,
                    window,
                    cx,
                );
            }
        }
        self.files.update(cx, |p, cx| {
            p.rows = files;
            cx.notify();
        });
        self.problems.update(cx, |p, cx| {
            p.rows = problems;
            cx.notify();
        });
        cx.notify();
    }
}
