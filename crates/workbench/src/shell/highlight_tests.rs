use super::{FileIndex, FileKey, Location, Workspace};
use claims::{assert_ok, assert_some};
use gpui_kit::{AppContext as _, TestAppContext};
use pretty_assertions::assert_eq;

fn ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    crate::app::typst_tokens::tokens(text)
        .into_iter()
        .filter(|t| super::highlight::theme_name(t.tag).is_some())
        .map(|t| t.range)
        .collect()
}

#[gpui_kit::test]
fn test_style_reload_conflict_and_theme_keep_decorations_current(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join("style.TYP");
    let key = FileKey::Disk(path.clone());
    let original = "\u{feff}#let first = 1\r\n#let name = \"Café\"\r\n";
    let updated = "// new\n#let f(x) = x + 42\n";
    assert_ok!(std::fs::write(&path, original));
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    let panel = assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.poll = None;
        ws.apply_index(FileIndex::scan(dir.path()), window, cx);
        ws.open(key.clone(), Location::File, window, cx);
        assert_some!(ws.docs.get(&key)).clone()
    })));
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(panel.read(cx).editor.read(cx).value().to_string(), original);
        assert_eq!(
            assert_some!(&panel.read(cx).highlight).collection.get_ranges(cx),
            ranges(original)
        );
    });
    assert_ok!(std::fs::write(&path, updated));
    assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.apply_index(FileIndex::scan(dir.path()), window, cx);
        assert!(
            assert_some!(&panel.read(cx).highlight)
                .collection
                .get_ranges(cx)
                .is_empty()
        );
    })));
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(
            assert_some!(&panel.read(cx).highlight).collection.get_ranges(cx),
            ranges(updated)
        );
        // Clearing first proves the actual global observer repaints, rather than merely retaining old ranges.
        let collection = assert_some!(&panel.read(cx).highlight).collection.clone();
        collection.clear(cx);
        gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(
            assert_some!(&panel.read(cx).highlight).collection.get_ranges(cx),
            ranges(updated)
        )
    });
    assert_ok!(cx.update_window(window, |_, window, cx| panel.update(cx, |p, cx| {
        p.document.edit("#let retained = 9".into());
        p.sync(window, cx);
    })));
    cx.run_until_parked();
    assert_ok!(std::fs::write(&path, "// disk"));
    assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.apply_index(FileIndex::scan(dir.path()), window, cx);
        assert!(panel.read(cx).document.has_conflict());
        assert_eq!(
            assert_some!(&panel.read(cx).highlight).collection.get_ranges(cx),
            ranges("#let retained = 9")
        );
        panel.update(cx, |p, cx| {
            p.document.take_disk();
            p.sync(window, cx);
        });
    })));
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(
            assert_some!(&panel.read(cx).highlight).collection.get_ranges(cx),
            ranges("// disk")
        )
    });
}

#[gpui_kit::test]
fn test_restored_builtin_and_missing_style_tabs_get_highlighted(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = assert_ok!(tempfile::tempdir());
    let disk = dir.path().join("late.typ");
    let builtin = FileKey::Builtin("lib/components.typ".into());
    assert_ok!(std::fs::write(&disk, "#let x = 1"));
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    let state = assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.poll = None;
        ws.apply_index(FileIndex::scan(dir.path()), window, cx);
        for key in [builtin.clone(), FileKey::Disk(disk.clone())] {
            ws.open(key, Location::File, window, cx);
        }
        ws.dock.read(cx).dump(cx)
    })));
    cx.run_until_parked();
    assert_ok!(std::fs::remove_file(&disk));
    assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        for panel in ws.docs.values() {
            ws.dock.update(cx, |d, cx| d.remove_panel(panel.clone(), window, cx));
        }
        ws.docs.clear();
        ws.apply_index(FileIndex::scan(dir.path()), window, cx);
        ws.restore_layout(state, window, cx);
    })));
    cx.run_until_parked();
    cx.update(|cx| {
        let panel = assert_some!(workspace.read(cx).docs.get(&builtin)).read(cx);
        assert_eq!(
            assert_some!(&panel.highlight).collection.get_ranges(cx),
            ranges(panel.document.text())
        );
        let late = assert_some!(workspace.read(cx).docs.get(&FileKey::Disk(disk.clone()))).read(cx);
        assert!(assert_some!(&late.highlight).collection.get_ranges(cx).is_empty());
    });
    assert_ok!(std::fs::write(&disk, "#let late = 2"));
    assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.apply_index(FileIndex::scan(dir.path()), window, cx);
    })));
    cx.run_until_parked();
    cx.update(|cx| {
        let late = assert_some!(workspace.read(cx).docs.get(&FileKey::Disk(disk.clone()))).read(cx);
        assert_eq!(
            assert_some!(&late.highlight).collection.get_ranges(cx),
            ranges("#let late = 2")
        );
    });
}

#[gpui_kit::test]
fn test_typst_tabs_reject_keyboard_edits(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join("style.typ");
    let original = "#let x = 1";
    assert_ok!(std::fs::write(&path, original));
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    let panel = assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.poll = None;
        ws.apply_index(FileIndex::scan(dir.path()), window, cx);
        let key = FileKey::Disk(path.clone());
        ws.open(key.clone(), Location::File, window, cx);
        assert_some!(ws.docs.get(&key)).clone()
    })));
    cx.run_until_parked();
    cx.simulate_keystrokes(window, "x backspace enter");
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(panel.read(cx).editor.read(cx).value().to_string(), original));
    assert_eq!(assert_ok!(std::fs::read_to_string(path)), original);
}

#[gpui_kit::test]
fn test_closing_style_tab_during_parse_discards_the_result(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    let weak = assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.poll = None;
        let dir = assert_ok!(tempfile::tempdir());
        ws.apply_index(FileIndex::scan(dir.path()), window, cx);
        let key = FileKey::Builtin("lib/components.typ".into());
        ws.open(key.clone(), Location::File, window, cx);
        let panel = assert_some!(ws.docs.get(&key)).clone();
        let weak = panel.downgrade();
        ws.dock.update(cx, |dock, cx| dock.remove_panel(panel, window, cx));
        ws.prune_closed(cx);
        weak
    })));
    cx.run_until_parked();
    assert!(weak.upgrade().is_none());
}

#[gpui_kit::test]
fn test_only_typst_disables_soft_wrapping(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (window, _) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    assert_ok!(cx.update_window(window, |_, window, cx| {
        for extension in ["md", "toml", "typ"] {
            let panel = super::panels::DocumentPanel::new(
                FileKey::Disk(format!("file.{extension}").into()),
                "file".into(),
                super::Document::new("long paragraph with many words ".repeat(20).into()),
                window,
                cx,
            );
            let editor = panel.read(cx).editor.clone();
            editor.update(cx, |editor, cx| {
                editor.prewrap(gpui_kit::font("Courier"), gpui_kit::px(14.), gpui_kit::px(100.), cx);
                assert_eq!(editor.display_map().wrap_row_count() > 1, extension != "typ");
            });
        }
    }));
    cx.run_until_parked();
}

#[gpui_kit::test]
fn test_rapid_changes_publish_only_latest_and_markdown_is_not_typst(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (window, _) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    let panel = assert_ok!(cx.update_window(window, |_, window, cx| {
        let plain = super::panels::DocumentPanel::new(
            FileKey::Disk("resume.md".into()),
            "resume".into(),
            super::Document::new("# Ada".into()),
            window,
            cx,
        );
        assert!(plain.read(cx).highlight.is_none());
        let panel = super::panels::DocumentPanel::new(
            FileKey::Disk("style.typ".into()),
            "style".into(),
            super::Document::new("#let first = 1".into()),
            window,
            cx,
        );
        panel.update(cx, |p, cx| {
            for text in ["// intermediate", "#let last = \"é\""] {
                p.document.edit(text.into());
                p.sync(window, cx);
                assert!(assert_some!(&p.highlight).collection.get_ranges(cx).is_empty());
            }
        });
        panel
    }));
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(
            assert_some!(&panel.read(cx).highlight).collection.get_ranges(cx),
            ranges("#let last = \"é\"")
        )
    });
}
