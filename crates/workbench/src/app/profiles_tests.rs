use super::*;
use claims::{assert_err, assert_ok};
use pretty_assertions::assert_eq;

#[test]
fn budget_rejection_retains_dependency_identity() {
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join("shared.toml");
    assert_ok!(std::fs::write(&path, "[select]"));
    let snapshots = Snapshots::default();
    snapshots
        .text
        .borrow_mut()
        .insert(dir.path().join("full.md"), Arc::from("x".repeat(32 * 1024 * 1024)));
    let source = Profiles {
        directory: DirSource::new(dir.path().to_owned()),
        snapshots: &snapshots,
    };
    let at = Located {
        path: dir.path().join("job.toml").into(),
        line: 1,
        column: 1,
    };
    assert_err!(source.find(&assert_ok!(ProfileName::try_new("shared")), &at));
    assert!(snapshots.failures.borrow().contains_key(&path));
}

#[cfg(unix)]
#[test]
fn dangling_external_alias_retains_dependency_identity() {
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join("shared.toml");
    assert_ok!(std::os::unix::fs::symlink("absent.toml", &path));
    let snapshots = Snapshots::default();
    let source = Profiles {
        directory: DirSource::new(dir.path().to_owned()),
        snapshots: &snapshots,
    };
    let at = Located {
        path: dir.path().join("job.toml").into(),
        line: 1,
        column: 1,
    };
    assert_err!(source.find(&assert_ok!(ProfileName::try_new("shared")), &at));
    assert!(snapshots.failures.borrow().contains_key(&path));
}

#[test]
fn profile_resolution_reuses_inventory_bytes_even_after_disk_changes() {
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join("shared.toml");
    assert_ok!(std::fs::write(&path, "[select]\ntags = ['valid']\n"));
    let snapshots = Snapshots::default();
    snapshots
        .text
        .borrow_mut()
        .insert(path.clone(), Arc::from("[select]\nwrong = true\n"));
    let source = Profiles {
        directory: DirSource::new(dir.path().to_owned()),
        snapshots: &snapshots,
    };
    let name = assert_ok!(ProfileName::try_new("shared"));
    let at = Located {
        path: dir.path().join("job.toml").into(),
        line: 1,
        column: 1,
    };
    let error = assert_err!(source.find(&name, &at));
    assert!(matches!(error, SpecError::UnknownKey { .. }));
    assert_eq!(&*snapshots.text.borrow()[&path], "[select]\nwrong = true\n");
}
