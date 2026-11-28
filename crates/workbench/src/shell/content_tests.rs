use super::{FileIndex, FileKey, Location, Workspace};
use claims::{assert_ok, assert_some};
use gpui_kit::{AppContext as _, TestAppContext};
use pretty_assertions::assert_eq;

#[gpui_kit::test]
fn source_jump_after_emoji_uses_editor_scalar_columns(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join("r.md");
    let text = "# Ada\n\n## Work\n\nA 😀 marker.\n";
    assert_ok!(std::fs::write(&path, text));
    let offset = assert_some!(text.find("marker"));
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.poll = None;
        ws.apply_index(FileIndex::scan(dir.path()), window, cx);
        ws.open(
            FileKey::Disk(path.clone()),
            Location::Bytes(offset..offset + 6),
            window,
            cx,
        );
        let panel = assert_some!(ws.docs.get(&FileKey::Disk(path.clone())));
        assert_eq!(panel.read(cx).editor.read(cx).cursor(), offset);
    })));
}

#[gpui_kit::test]
fn activation_rejects_old_results_and_opens_targets_with_engine_outcomes(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = assert_ok!(tempfile::tempdir());
    let root = dir.path();
    let path = root.join("r.md");
    let spec = root.join("job.toml");
    assert_ok!(std::fs::write(
        &path,
        "# Ada\n\n## Work\n\n- Core <!-- @core -->\n- Extra <!-- @extra -->\n"
    ));
    assert_ok!(std::fs::write(
        &spec,
        "[build]\ncontent = 'r.md'\n[select]\ncore-only = true\n"
    ));
    let before = FileIndex::scan(root);
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.poll = None;
        ws.root = Some(root.to_owned());
        ws.apply_index(before, window, cx);
        ws.open(FileKey::Disk(path.clone()), Location::File, window, cx);
        ws.activate_spec(Some(spec.clone()), window, cx);
        let panel = assert_some!(ws.docs.get(&FileKey::Disk(path.clone()))).clone();
        assert!(panel.read(cx).content.model.is_none());
        ws.scan = None;
        ws.accept_scan(
            ws.generation,
            FileIndex::scan_active(root, Some(&spec)),
            None,
            window,
            cx,
        );
        let model = assert_some!(panel.read(cx).content.model.as_ref());
        assert!(model.assessed);
        assert_eq!(
            model.items[2].outcome,
            crate::app::content::Outcome::Dropped(resume::select::DropReason::CoreOnly)
        );
        assert_eq!(
            panel.read(cx).document.text(),
            "# Ada\n\n## Work\n\n- Core <!-- @core -->\n- Extra <!-- @extra -->\n"
        );
        let stale = FileIndex::scan_active(root, Some(&spec));
        let old_generation = ws.generation;
        ws.activate_spec(None, window, cx);
        ws.scan = None;
        ws.accept_scan(old_generation, stale, None, window, cx);
        assert!(panel.read(cx).content.model.is_none());
        assert!(ws.scan.is_some());
        ws.scan = None;
        ws.accept_scan(ws.generation, FileIndex::scan(root), None, window, cx);
        assert!(!assert_some!(panel.read(cx).content.model.as_ref()).assessed);
    })));
}

#[gpui_kit::test]
fn deleted_active_spec_is_unavailable_and_recovery_keeps_navigation(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = assert_ok!(tempfile::tempdir());
    let root = dir.path();
    let spec = root.join("job.toml");
    let path = root.join("r.md");
    assert_ok!(std::fs::write(&path, "# Ada\n\n## Work\n\nExperience.\n"));
    let text = "[build]\ncontent = 'r.md'\n";
    assert_ok!(std::fs::write(&spec, text));
    let (window, workspace) = cx.update(|cx| assert_ok!(gpui_kit::open_window(Default::default(), cx, Workspace::new)));
    assert_ok!(cx.update_window(window, |_, window, cx| workspace.update(cx, |ws, cx| {
        ws.poll = None;
        ws.root = Some(root.to_owned());
        ws.active_spec = Some(spec.clone());
        ws.apply_index(FileIndex::scan_active(root, Some(&spec)), window, cx);
        ws.open(FileKey::Disk(path.clone()), Location::File, window, cx);
        assert_ok!(std::fs::remove_file(&spec));
        ws.apply_index(FileIndex::scan_active(root, Some(&spec)), window, cx);
        assert!(!ws.index.specs.contains_key(&spec));
        assert!(
            ws.problems
                .read(cx)
                .rows
                .iter()
                .any(|r| r.label.contains("Active spec is unavailable"))
        );
        let panel = assert_some!(ws.docs.get(&FileKey::Disk(path.clone()))).clone();
        assert!(
            assert_some!(panel.read(cx).content.model.as_ref())
                .items
                .iter()
                .all(|i| i.outcome == crate::app::content::Outcome::Unavailable)
        );
        assert_ok!(std::fs::write(&spec, text));
        ws.apply_index(FileIndex::scan_active(root, Some(&spec)), window, cx);
        assert!(assert_some!(panel.read(cx).content.model.as_ref()).assessed);
        assert!(panel.read(cx).content.workspace.is_some());
    })));
}
