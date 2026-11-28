use super::{FileIndex, FileKey, Location, Workspace};
use claims::{assert_ok, assert_some};
use gpui_kit::{AppContext as _, TestAppContext};
use pretty_assertions::assert_eq;

#[gpui_kit::test]
fn problem_jump_and_dirty_watch_reload_use_real_panels(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(super::bind_keys);
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join("broken.toml");
    assert_ok!(std::fs::write(
        &path,
        "[build]\ncontent = 'a.md'\n[select]\nwrong = true\n"
    ));
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    assert_ok!(cx.update_window(window, |_, window, cx| {
        workspace.update(cx, |ws, cx| {
            ws.poll = None;
            ws.apply_index(FileIndex::scan(dir.path()), window, cx);
            let key = FileKey::Disk(path.clone());
            let row = assert_some!(ws.problems.read(cx).rows.iter().find(|r| r.target == key));
            let location = row.location.clone();
            assert!(matches!(location, Location::LineColumn { line: 4, .. }));
            ws.open(key.clone(), location, window, cx);
            let panel = assert_some!(ws.docs.get(&key)).clone();
            assert_eq!(
                panel.read(cx).editor.read(cx).cursor_position(),
                lsp_types::Position::new(3, 0)
            );
            panel.update(cx, |p, cx| {
                p.document.edit("unsaved".into());
                p.sync(window, cx);
            });
            assert_ok!(std::fs::write(&path, "changed on disk"));
            ws.apply_index(FileIndex::scan(dir.path()), window, cx);
            assert_eq!(panel.read(cx).document.text(), "unsaved");
            assert!(panel.read(cx).document.has_conflict());
            assert_eq!(panel.read(cx).editor.read(cx).value().to_string(), "unsaved");
            panel.update(cx, |p, cx| {
                assert!(p.document.take_disk());
                p.sync(window, cx);
            });
            assert_eq!(panel.read(cx).editor.read(cx).value().to_string(), "changed on disk");
        });
    }));
}

#[gpui_kit::test]
fn palette_is_reachable_by_platform_key_and_escape_returns_focus(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(super::bind_keys);
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    assert_ok!(cx.update_window(window, |_, _, cx| workspace.update(cx, |ws, _| ws.poll = None)));
    cx.run_until_parked();
    cx.simulate_keystrokes(
        window,
        if cfg!(target_os = "macos") {
            "cmd-shift-p"
        } else {
            "ctrl-shift-p"
        },
    );
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(workspace.read(cx).palette_open, Some(false)));
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(workspace.read(cx).palette_open, None));
}

#[gpui_kit::test]
fn closed_deleted_tabs_and_side_docks_do_not_leak_into_next_folder(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let a = assert_ok!(tempfile::tempdir());
    let b = assert_ok!(tempfile::tempdir());
    let path = a.path().join("resume.md");
    assert_ok!(std::fs::write(&path, "# Ada\n"));
    let key = FileKey::Disk(path.clone());
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.poll = None;
        ws.apply_index(FileIndex::scan(a.path()), window, cx);
        ws.open(key.clone(), Location::File, window, cx);
        let panel = assert_some!(ws.docs.get(&key)).clone();
        ws.dock.update(cx, |dock, cx| dock.remove_panel(panel, window, cx));
        assert_ok!(std::fs::remove_file(&path));
        ws.apply_index(FileIndex::scan(a.path()), window, cx);
        assert!(ws.docs.is_empty());
        assert_ok!(std::fs::write(&path, "# Ada\n"));
        ws.apply_index(FileIndex::scan(a.path()), window, cx);
        ws.open(key.clone(), Location::File, window, cx);
        let panel = assert_some!(ws.docs.get(&key)).clone();
        ws.dock.update(cx, |dock, cx| {
            dock.remove_panel(panel.clone(), window, cx);
            dock.add_panel_view(
                gpui_kit::component::dock::panel_handle(panel.clone()),
                gpui_kit::component::dock::DockPlacement::Right,
                None,
                window,
                cx,
            );
        });
        ws.install_folder(b.path().to_owned(), super::FolderWatch::new(b.path()), window, cx);
        assert!(ws.docs.is_empty());
        assert!(ws.index.entries.is_empty());
        assert!(ws.files.read(cx).rows.is_empty());
        assert!(
            ws.dock
                .read(cx)
                .panel(gpui_kit::component::dock::PanelId::from(panel.entity_id()))
                .is_none()
        );
        // The same running scan must survive repeated refresh requests.
        let task = ws.scan.as_ref().map(|t| t as *const _);
        ws.rescan(window, cx);
        assert_eq!(ws.scan.as_ref().map(|t| t as *const _), task);
        assert!(ws.restore);
        assert!(ws.rescan_pending);
        ws.scan = None;
        ws.watch = None;
    })));
}

#[gpui_kit::test]
fn inherited_profile_problem_opens_normalized_source_path(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::create_dir(dir.path().join("positions")));
    assert_ok!(std::fs::create_dir(dir.path().join("profiles")));
    let profile = dir.path().join("profiles/shared.toml");
    assert_ok!(std::fs::write(&profile, "[select]\nwrong = true\n"));
    assert_ok!(std::fs::write(
        dir.path().join("positions/job.toml"),
        "extends = ['shared']\n[build]\ncontent = 'resume.md'\n"
    ));
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.poll = None;
        ws.apply_index(FileIndex::scan(&dir.path().join("positions")), window, cx);
        let rows = &ws.problems.read(cx).rows;
        assert!(
            rows.iter()
                .filter(|r| r.target == FileKey::Disk(profile.clone()))
                .count()
                >= 2
        );
        let row = assert_some!(rows.iter().find(|r| r.target == FileKey::Disk(profile.clone())));
        ws.open(row.target.clone(), row.location.clone(), window, cx);
        let panel = assert_some!(ws.docs.get(&FileKey::Disk(profile.clone())));
        assert_eq!(
            panel.read(cx).editor.read(cx).cursor_position(),
            lsp_types::Position::new(1, 0)
        );
    })));
}

#[gpui_kit::test]
fn stale_scan_cannot_replace_new_folder_and_pending_refresh_is_scheduled(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let a = assert_ok!(tempfile::tempdir());
    let b = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::write(a.path().join("a.md"), "# A"));
    assert_ok!(std::fs::write(b.path().join("b.md"), "# B"));
    let stale = FileIndex::scan(a.path());
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.poll = None;
        ws.root = Some(b.path().to_owned());
        ws.generation = 2;
        ws.restore = true;
        ws.accept_scan(1, stale, Some(Ok((a.path().join("old-layout"), None))), window, cx);
        assert!(ws.index.entries.is_empty());
        assert!(ws.layout_path.is_none());
        assert!(ws.scan.is_some());
        assert!(ws.restore);
        ws.scan = None;
        ws.rescan_pending = true;
        ws.accept_scan(
            2,
            FileIndex::scan(b.path()),
            Some(Ok((b.path().join("layout"), None))),
            window,
            cx,
        );
        assert!(ws.index.entries.is_empty());
        assert!(ws.layout_path.is_none());
        assert!(ws.scan.is_some());
        assert!(ws.restore);
        ws.scan = None;
        ws.accept_scan(
            2,
            FileIndex::scan(b.path()),
            Some(Ok((b.path().join("layout"), None))),
            window,
            cx,
        );
        assert!(ws.index.entries.contains_key(&FileKey::Disk(b.path().join("b.md"))));
        assert!(!ws.index.entries.contains_key(&FileKey::Disk(a.path().join("a.md"))));
        assert_eq!(ws.layout_path.as_deref(), Some(b.path().join("layout").as_path()));
        assert!(!ws.restore);
        ws.scan = None;
    })));
}

#[gpui_kit::test]
fn retained_text_blocks_window_and_folder_close_until_explicit_discard(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let a = assert_ok!(tempfile::tempdir());
    let b = assert_ok!(tempfile::tempdir());
    let path = a.path().join("resume.md");
    let key = FileKey::Disk(path.clone());
    assert_ok!(std::fs::write(&path, "# Ada"));
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    let panel = assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.poll = None;
        ws.root = Some(a.path().to_owned());
        ws.apply_index(FileIndex::scan(a.path()), window, cx);
        ws.open(key.clone(), Location::File, window, cx);
        let panel = assert_some!(ws.docs.get(&key)).clone();
        panel.update(cx, |p, _| p.document.edit("retained edit".into()));
        ws.open_folder(b.path().to_owned(), window, cx);
        assert_eq!(ws.root.as_deref(), Some(a.path()));
        assert_eq!(panel.read(cx).document.text(), "retained edit");
        assert!(!gpui_kit::component::dock::BasePanel::closable(panel.read(cx), cx));
        panel
    })));
    let mut visual = gpui_kit::VisualTestContext::from_window(window, cx);
    assert!(!visual.simulate_close());
    visual.update(|_, cx| panel.update(cx, |p, _| p.discard_allowed = true));
    assert!(visual.simulate_close());
    assert_ok!(std::fs::write(&path, "new external change"));
    visual.update(|window, cx| workspace.update(cx, |ws, cx| ws.apply_index(FileIndex::scan(a.path()), window, cx)));
    assert!(!visual.simulate_close());
    visual.update(|window, cx| {
        panel.update(cx, |p, _| p.discard_allowed = true);
        workspace.update(cx, |ws, cx| {
            ws.install_folder(b.path().to_owned(), super::FolderWatch::new(b.path()), window, cx);
            ws.scan = None;
            ws.watch = None;
        });
    });
    assert!(visual.simulate_close());
}

#[gpui_kit::test]
async fn final_layout_is_saved_before_switching_folder(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let a = assert_ok!(tempfile::tempdir());
    let b = assert_ok!(tempfile::tempdir());
    let saved = a.path().join("layout.json");
    assert_ok!(std::fs::write(a.path().join("resume.md"), "# Ada"));
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    let expected = assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.poll = None;
        ws.apply_index(FileIndex::scan(a.path()), window, cx);
        ws.open(FileKey::Disk(a.path().join("resume.md")), Location::File, window, cx);
        ws.layout_path = Some(saved.clone());
        let expected = ws.dock.read(cx).dump(cx);
        ws.open_folder(b.path().to_owned(), window, cx);
        assert!(ws.transition.is_some());
        expected
    })));
    cx.condition(&workspace, |ws, _| ws.transition.is_none()).await;
    let actual = assert_some!(assert_ok!(crate::app::layout::load(&saved)));
    assert_eq!(actual.center.children, expected.center.children);
    assert_eq!(actual.version, expected.version);
    cx.update(|cx| {
        workspace.update(cx, |ws, _| {
            ws.scan = None;
            ws.watch = None;
        })
    });
}

#[gpui_kit::test]
async fn final_layout_is_saved_before_window_close(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = assert_ok!(tempfile::tempdir());
    let saved = dir.path().join("layout.json");
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    let expected = assert_ok!(cx.update_window(window, |_, _, cx| workspace.update(cx, |ws, cx| {
        ws.poll = None;
        ws.layout_path = Some(saved.clone());
        ws.dock.read(cx).dump(cx)
    })));
    let mut visual = gpui_kit::VisualTestContext::from_window(window, cx);
    assert!(!visual.simulate_close());
    cx.condition(&workspace, |ws, _| ws.transition.is_none()).await;
    assert_eq!(assert_some!(assert_ok!(crate::app::layout::load(&saved))), expected);
}

#[gpui_kit::test]
fn keyboard_problem_action_opens_the_actual_target(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(super::bind_keys);
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join("bad.toml");
    assert_ok!(std::fs::write(&path, "[select]\nwrong = true\n"));
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.poll = None;
        ws.apply_index(FileIndex::scan(dir.path()), window, cx);
    })));
    cx.run_until_parked();
    cx.simulate_keystrokes(window, "f8");
    cx.run_until_parked();
    cx.update(|cx| {
        let panel = assert_some!(workspace.read(cx).docs.get(&FileKey::Disk(path)));
        assert_eq!(
            panel.read(cx).editor.read(cx).cursor_position(),
            lsp_types::Position::new(1, 0)
        );
    });
}

#[gpui_kit::test]
async fn layout_drain_includes_late_dock_change_and_failure_resumes_scanning(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let a = assert_ok!(tempfile::tempdir());
    let b = assert_ok!(tempfile::tempdir());
    let path = a.path().join("r.md");
    assert_ok!(std::fs::write(&path, "# Ada"));
    let saved = a.path().join("layout.json");
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.poll = None;
        ws.apply_index(FileIndex::scan(a.path()), window, cx);
        ws.open(FileKey::Disk(path.clone()), Location::File, window, cx);
        ws.layout_path = Some(saved.clone());
        ws.save_layout(cx);
        ws.open_folder(b.path().to_owned(), window, cx);
        let panel = assert_some!(ws.docs.get(&FileKey::Disk(path.clone()))).clone();
        ws.dock.update(cx, |dock, cx| dock.remove_panel(panel, window, cx));
    })));
    cx.condition(&workspace, |ws, _| ws.transition.is_none()).await;
    let actual = assert_some!(assert_ok!(crate::app::layout::load(&saved)));
    assert!(!serde_json::to_string(&actual).unwrap().contains("r.md"));
    assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.scan = None;
        ws.watch = None;
        ws.layout_path = Some(a.path().to_owned());
        ws.rescan_pending = true;
        ws.open_folder(a.path().to_owned(), window, cx);
    })));
    cx.condition(&workspace, |ws, _| ws.transition.is_none()).await;
    cx.update(|cx| {
        workspace.update(cx, |ws, _| {
            assert!(ws.scan.is_some() || !ws.index.entries.is_empty());
            assert!(!ws.rescan_pending);
            ws.scan = None;
            ws.watch = None;
        })
    });
}

#[gpui_kit::test]
fn dock_restores_document_identity_and_layout(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join("resume.md");
    assert_ok!(std::fs::write(&path, "# Ada\n"));
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    assert_ok!(cx.update_window(window, |_, window, cx| {
        workspace.update(cx, |ws, cx| {
            ws.poll = None;
            ws.apply_index(FileIndex::scan(dir.path()), window, cx);
            ws.open(FileKey::Disk(path.clone()), Location::File, window, cx);
            let state = ws.dock.read(cx).dump(cx);
            let saved = dir.path().join("layout.json");
            assert_ok!(crate::app::layout::save(&saved, &state));
            let restored = assert_some!(assert_ok!(crate::app::layout::load(&saved)));
            ws.restore_layout(restored, window, cx);
            assert_eq!(ws.dock.read(cx).dump(cx), state);
            assert!(ws.docs.contains_key(&FileKey::Disk(path.clone())));
            let key = FileKey::Disk(path.clone());
            let panel = assert_some!(ws.docs.get(&key)).clone();
            ws.dock
                .update(cx, |dock, cx| dock.remove_panel(panel.clone(), window, cx));
            ws.open(key.clone(), Location::File, window, cx);
            assert!(
                ws.dock
                    .read(cx)
                    .panel(gpui_kit::component::dock::PanelId::from(panel.entity_id()))
                    .is_some()
            );
            assert_ok!(std::fs::remove_file(&path));
            ws.docs.clear();
            ws.apply_index(FileIndex::scan(dir.path()), window, cx);
            ws.restore_layout(state, window, cx);
            assert_ok!(std::fs::write(&path, "returned"));
            ws.apply_index(FileIndex::scan(dir.path()), window, cx);
            let panel = assert_some!(ws.docs.get(&key));
            assert_eq!(panel.read(cx).document.text(), "returned");
            assert_ok!(std::fs::remove_file(&path));
            ws.apply_index(FileIndex::scan(dir.path()), window, cx);
            let panel = assert_some!(ws.docs.get(&key)).clone();
            assert!(panel.read(cx).protected());
            panel.update(cx, |p, _| p.discard_allowed = true);
            assert!(!panel.read(cx).protected());
        });
    }));
}
