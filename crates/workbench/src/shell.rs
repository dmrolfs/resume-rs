//! Native window and dock; policy lives in app modules.
mod highlight;
#[cfg(test)]
mod highlight_tests;
mod loading;
mod panels;
mod persistence;
#[cfg(test)]
mod tests;
use crate::app::{
    document::{DiskState, Document},
    index::{FileIndex, FileKey, FileKind},
    problem::{Location, Problem},
    watch::FolderWatch,
};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::component::{
    button::Button,
    dock::{BasePanel, DockArea, DockLayout, DockPlacement, DockSkin, Panel, PanelEvent, PanelId, panel_handle},
    input::{Editor, EditorState},
};
use gpui_kit::*;
use panels::{DocumentPanel, Row, RowsPanel};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use crate::app::layout;
use gpui_kit::component::dock::{DockAreaState, PanelInfo, PanelState, register_panel};
use std::rc::Rc;

use gpui_kit::component::command::{Command, CommandItem, CommandState};

actions!(
    workbench,
    [OpenFolder, QuickOpen, ShowCommands, ClosePalette, Refresh, NextProblem]
);
const COMMANDS: &[(&str, &str)] = &[
    ("Open Folder", "secondary-o"),
    ("Quick Open", "secondary-p"),
    ("Refresh Files", "secondary-r"),
    ("Next Problem", "f8"),
];

pub fn run() {
    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(|cx| {
        gpui_kit::init(cx);
        bind_keys(cx);
        cx.set_menus([Menu::new("File").items([
            MenuItem::action("Open Folder", OpenFolder),
            MenuItem::action("Quick Open", QuickOpen),
            MenuItem::action("Refresh Files", Refresh),
            MenuItem::action("Command Palette", ShowCommands),
        ])]);
        if let Err(error) = gpui_kit::open_window(WindowOptions::default(), cx, Workspace::new) {
            eprintln!("Could not open Resume Workbench: {error}");
        }
        cx.activate(true);
    });
}

fn bind_keys(cx: &mut App) {
    let actions: [(&str, Box<dyn Action>); 6] = [
        ("secondary-o", Box::new(OpenFolder)),
        ("secondary-p", Box::new(QuickOpen)),
        ("secondary-shift-p", Box::new(ShowCommands)),
        ("escape", Box::new(ClosePalette)),
        ("secondary-r", Box::new(Refresh)),
        ("f8", Box::new(NextProblem)),
    ];
    let bindings: Vec<_> = actions
        .into_iter()
        .map(|(key, action)| KeyBinding::load(key, action, None, false, None, cx).expect("valid built-in key binding"))
        .collect();
    cx.bind_keys(bindings);
}

#[derive(Debug, snafu::Snafu)]
enum FolderError {
    #[snafu(display("Could not save previous folder layout"))]
    Layout { source: layout::LayoutError },
    #[snafu(display("Could not resolve folder path"))]
    Resolve { source: std::io::Error },
}

pub struct Workspace {
    dock: Entity<DockArea>,
    index: FileIndex,
    root: Option<PathBuf>,
    docs: BTreeMap<FileKey, Entity<DocumentPanel>>,
    files: Entity<RowsPanel>,
    problems: Entity<RowsPanel>,
    watch: Option<FolderWatch>,
    scan: Option<Task<()>>,
    transition: Option<Task<()>>,
    focus: FocusHandle,
    poll: Option<Task<()>>,
    status: String,
    watch_error: Option<String>,
    generation: u64,
    problem_cursor: usize,
    rescan_pending: bool,
    palette: Entity<CommandState>,
    palette_open: Option<bool>,
    restore: bool,
    layout_path: Option<PathBuf>,
    last_layout: Option<DockAreaState>,
    save: Option<Task<()>>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        window.set_window_title("Resume Workbench");
        let (dock, _) = DockSkin::dock_area("workbench", Some(1), window, cx);
        let files = cx.new(|cx| RowsPanel::new("Files", cx));
        let problems = cx.new(|cx| RowsPanel::new("Problems", cx));
        dock.update(cx, |area, cx| {
            area.set_center(DockLayout::tabs(), window, cx);
            area.set_dock(
                DockPlacement::Left,
                DockLayout::tabs().panel_view(panel_handle(files.clone()), cx),
                window,
                cx,
            );
            area.set_dock_size(DockPlacement::Left, px(250.), window, cx);
            area.set_dock(
                DockPlacement::Bottom,
                DockLayout::tabs().panel_view(panel_handle(problems.clone()), cx),
                window,
                cx,
            );
            area.set_dock_size(DockPlacement::Bottom, px(180.), window, cx);
        });
        let palette = cx.new(|cx| CommandState::new(window, cx));
        let workspace = cx.new(|cx| Self {
            dock,
            index: FileIndex::default(),
            root: None,
            docs: BTreeMap::new(),
            files: files.clone(),
            problems: problems.clone(),
            watch: None,
            scan: None,
            transition: None,
            focus: cx.focus_handle(),
            poll: None,
            status: "Open a folder to begin".into(),
            watch_error: None,
            generation: 0,
            problem_cursor: 0,
            rescan_pending: false,
            palette,
            palette_open: None,
            restore: false,
            layout_path: None,
            last_layout: None,
            save: None,
        });
        files.update(cx, |p, _| p.workspace = Some(workspace.downgrade()));
        problems.update(cx, |p, _| p.workspace = Some(workspace.downgrade()));
        workspace.update(cx, |this, cx| {
            this.poll = Some(cx.spawn_in(window, async move |weak, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_millis(250)).await;
                    if weak
                        .update_in(cx, |this, window, cx| {
                            let (changed, error) = this.watch.as_ref().map_or((false, None), FolderWatch::take);
                            if let Some(error) = error {
                                this.status = format!("Watcher error: {error}");
                                cx.notify();
                            }
                            if changed {
                                this.rescan(window, cx);
                            }
                            this.save_layout(cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }));
            if let Some(root) = std::env::args_os().nth(1) {
                this.open_folder(PathBuf::from(root), window, cx);
            }
        });
        let focus = workspace.read(cx).focus.clone();
        window.focus(&focus, cx);
        let weak = workspace.downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| {
                this.prune_closed(cx);
                if this.transition.is_some() {
                    return false;
                }
                if this.docs.values().any(|panel| panel.read(cx).protected()) {
                    this.status = "Unsaved text retained: resolve conflicts before closing".into();
                    cx.notify();
                    false
                } else {
                    this.close_window(window, cx)
                }
            })
            .unwrap_or(true)
        });
        workspace
    }

    fn next_problem(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let rows = &self.problems.read(cx).rows;
        if rows.is_empty() {
            return;
        }
        let row = &rows[self.problem_cursor % rows.len()];
        let (key, location) = (row.target.clone(), row.location.clone());
        self.problem_cursor = (self.problem_cursor + 1) % rows.len();
        self.open(key, location, window, cx);
    }

    fn show_palette(&mut self, files: bool, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.palette_open = Some(files);
        self.palette.update(cx, |state, cx| state.set_query("", window, cx));
        let focus = self.palette.focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    fn choose_folder(&mut self, _: &OpenFolder, window: &mut Window, cx: &mut Context<'_, Self>) {
        let choice = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open resume folder".into()),
        });
        cx.spawn_in(window, async move |weak, cx| match choice.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = weak.update_in(cx, |this, window, cx| this.open_folder(path, window, cx));
                }
            }
            Ok(Err(error)) => {
                let _ = weak.update(cx, |this, cx| {
                    this.status = format!("Could not choose folder: {error}");
                    cx.notify();
                });
            }
            _ => {}
        })
        .detach();
    }

    fn resume_pending(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.rescan_pending {
            self.rescan_pending = false;
            self.rescan(window, cx);
        }
    }

    fn open(&mut self, key: FileKey, location: Location, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.transition.is_some() {
            return;
        }
        let panel = if let Some(panel) = self.docs.get(&key) {
            let panel = panel.clone();
            if self.dock.read(cx).panel(PanelId::from(panel.entity_id())).is_none() {
                self.dock.update(cx, |area, cx| {
                    area.add_panel_view(panel_handle(panel.clone()), DockPlacement::Center, None, window, cx)
                });
            }
            panel
        } else {
            let Some(text) = self.index.entries.get(&key).and_then(|e| e.text.clone()) else {
                self.status = format!("Cannot open {}; see Problems", key_label(&key));
                cx.notify();
                return;
            };
            let title = match &key {
                FileKey::Disk(path) | FileKey::Builtin(path) => path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
            };
            let panel = DocumentPanel::new(key.clone(), title, Document::new(text), window, cx);
            self.dock.update(cx, |area, cx| {
                area.add_panel_view(panel_handle(panel.clone()), DockPlacement::Center, None, window, cx)
            });
            self.docs.insert(key.clone(), panel.clone());
            panel
        };
        self.dock.update(cx, |area, cx| {
            area.select_panel(PanelId::from(panel.entity_id()), window, cx)
        });
        if panel.read(cx).document.is_dirty() && !matches!(location, Location::File) {
            self.status = "Problem refers to disk text; resolve the retained-text conflict before jumping".into();
            cx.notify();
            return;
        }
        panel.update(cx, |p, cx| {
            let pos = location.position(p.document.text());
            p.editor.update(cx, |e, cx| e.set_cursor_position(pos, window, cx));
            window.focus(&p.editor.focus_handle(cx), cx);
        });
    }
}

fn key_label(key: &FileKey) -> String {
    match key {
        FileKey::Disk(path) => path.display().to_string(),
        FileKey::Builtin(path) => format!("Built-in: {} (read-only)", path.display()),
    }
}
fn problem_row(p: &Problem) -> Row {
    Row {
        label: format!(
            "{} {}: {}",
            p.code.map_or_else(|| p.severity.to_string(), |c| c.to_string()),
            p.file.display(),
            p.message
        ),
        target: FileKey::Disk(resume::spec::error::tidy_path(&p.file)),
        location: p.location.clone(),
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        v_flex()
            .track_focus(&self.focus)
            .size_full()
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(Self::choose_folder))
            .on_action(cx.listener(|this, _: &QuickOpen, w, cx| this.show_palette(true, w, cx)))
            .on_action(cx.listener(|this, _: &ShowCommands, w, cx| this.show_palette(false, w, cx)))
            .on_action(cx.listener(|this, _: &NextProblem, w, cx| this.next_problem(w, cx)))
            .on_action(cx.listener(|this, _: &Refresh, w, cx| this.rescan(w, cx)))
            .on_action(cx.listener(|this, _: &ClosePalette, w, cx| {
                this.palette_open = None;
                w.focus(&this.focus, cx);
                cx.notify();
            }))
            .children(self.palette_open.map(|files| {
                let keys: Vec<_> = self.index.entries.keys().cloned().collect();
                let items: Vec<_> = if files {
                    keys.iter()
                        .map(|key| CommandItem::new().label(key_label(key)))
                        .collect()
                } else {
                    COMMANDS
                        .iter()
                        .map(|(label, key)| CommandItem::new().label(format!("{label} ({key})")))
                        .collect()
                };
                let weak = cx.entity().downgrade();
                Command::new(&self.palette)
                    .items(items)
                    .max_h(px(300.))
                    .on_confirm(move |at, w, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            this.palette_open = None;
                            if files {
                                if let Some(key) = keys.get(at.row) {
                                    this.open(key.clone(), Location::File, w, cx);
                                }
                            } else {
                                match at.row {
                                    0 => this.choose_folder(&OpenFolder, w, cx),
                                    1 => this.show_palette(true, w, cx),
                                    2 => this.rescan(w, cx),
                                    3 => this.next_problem(w, cx),
                                    _ => {}
                                }
                            }
                            cx.notify();
                        });
                    })
            }))
            .child(
                h_flex().p_2().child(
                    Button::new("open-folder")
                        .label("Open Folder")
                        .on_click(cx.listener(|this, _, w, cx| this.choose_folder(&OpenFolder, w, cx))),
                ),
            )
            .child(
                div().text_sm().child(
                    self.root
                        .as_ref()
                        .map_or_else(|| "No folder open".into(), |p| p.display().to_string()),
                ),
            )
            .child(div().flex_1().min_h_0().child(self.dock.clone()))
            .child(div().p_2().child(self.status.clone()))
    }
}
