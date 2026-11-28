//! Coalesced notifications bound memory independently of filesystem event volume.
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use snafu::{ResultExt, Snafu};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Debug, Snafu)]
pub enum WatchError {
    #[snafu(display("Could not resolve watched folder {}", path.display()))]
    Resolve {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("Could not start folder watching"))]
    Start { source: notify::Error },
    #[snafu(display("Could not watch {}", path.display()))]
    Folder {
        path: std::path::PathBuf,
        source: notify::Error,
    },
}

#[derive(Default)]
struct Pending {
    changed: bool,
    error: Option<notify::Error>,
}

#[derive(Clone)]
pub struct FolderWatch {
    watcher: Arc<Mutex<RecommendedWatcher>>,
    profiles: Option<std::path::PathBuf>,
    pending: Arc<Mutex<Pending>>,
}
impl FolderWatch {
    pub fn new(path: &Path) -> Result<Self, WatchError> {
        let pending = Arc::new(Mutex::new(Pending::default()));
        let callback = pending.clone();
        let root = std::fs::canonicalize(path).context(ResolveSnafu { path })?;
        let profiles = root.parent().map(|parent| parent.join("profiles"));
        let profiles_callback = profiles.clone();
        let mut watcher = notify::recommended_watcher(move |event: Result<Event, notify::Error>| {
            let mut state = callback.lock().unwrap_or_else(|e| e.into_inner());
            match event {
                Ok(event)
                    if (event.paths.is_empty()
                        || event.paths.iter().any(|p| {
                            relevant(&root, p)
                                || profiles_callback.as_ref().is_some_and(|profiles| relevant(profiles, p))
                        }))
                        && matches!(
                            event.kind,
                            EventKind::Create(_)
                                | EventKind::Modify(_)
                                | EventKind::Remove(_)
                                | EventKind::Any
                                | EventKind::Other
                        ) =>
                {
                    state.changed = true
                }
                Err(error) => {
                    state.changed = true;
                    state.error = Some(error);
                }
                _ => {}
            }
        })
        .context(StartSnafu)?;
        watcher
            .watch(path, RecursiveMode::Recursive)
            .context(FolderSnafu { path })?;
        if let Some(parent) = path.parent() {
            watcher
                .watch(parent, RecursiveMode::NonRecursive)
                .context(FolderSnafu { path: parent })?;
        }
        if let Some(profiles) = profiles.as_ref().filter(|profiles| profiles.is_dir()) {
            watcher
                .watch(profiles, RecursiveMode::Recursive)
                .context(FolderSnafu { path: profiles.clone() })?;
        }
        Ok(Self {
            watcher: Arc::new(Mutex::new(watcher)),
            profiles,
            pending,
        })
    }

    /// Refresh dependency subscriptions on the background scanner, including newly created folders.
    pub fn refresh_dependencies(&self) -> Result<(), WatchError> {
        if let Some(profiles) = self.profiles.as_ref().filter(|p| p.is_dir()) {
            self.watcher
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .watch(profiles, RecursiveMode::Recursive)
                .context(FolderSnafu { path: profiles })?;
        }
        Ok(())
    }

    /// A complete bounded rescan handles rename pairs and missed/coalesced events alike.
    pub fn take(&self) -> (bool, Option<notify::Error>) {
        let state = std::mem::take(&mut *self.pending.lock().unwrap_or_else(|e| e.into_inner()));
        (state.changed, state.error)
    }
}

fn relevant(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|relative| {
        !relative.components().any(|c| {
            let name = c.as_os_str().to_string_lossy();
            name.starts_with('.') || matches!(name.as_ref(), "out" | "target" | "node_modules")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::FolderWatch;
    use claims::assert_ok;
    #[test]
    fn ignored_directories_do_not_schedule_scans() {
        let root = std::path::Path::new("job");
        for path in ["job/out/a.pdf", "job/.git/index", "job/target/a", "job/node_modules/a"] {
            assert!(!super::relevant(root, std::path::Path::new(path)));
        }
        assert!(super::relevant(root, std::path::Path::new("job/resume.md")));
    }

    #[test]
    fn profiles_created_after_startup_are_discovered_and_subscribed() {
        let dir = assert_ok!(tempfile::tempdir());
        let root = dir.path().join("positions");
        assert_ok!(std::fs::create_dir(&root));
        let watch = assert_ok!(FolderWatch::new(&root));
        let profiles = dir.path().join("profiles");
        assert_ok!(std::fs::create_dir(&profiles));
        wait_changed(&watch);
        assert_ok!(watch.refresh_dependencies());
        watch.take();
        assert_ok!(std::fs::write(profiles.join("shared.toml"), "[select]"));
        wait_changed(&watch);
    }

    fn wait_changed(watch: &FolderWatch) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let (changed, error) = watch.take();
            assert!(error.is_none(), "{error:?}");
            if changed {
                return;
            }
            assert!(std::time::Instant::now() < deadline, "watch notification timed out");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn real_file_change_arrives_without_unbounded_event_queue() {
        let dir = assert_ok!(tempfile::tempdir());
        let watch = assert_ok!(FolderWatch::new(dir.path()));
        assert_ok!(std::fs::write(dir.path().join("test.md"), "changed"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let (changed, error) = watch.take();
            assert!(error.is_none(), "{error:?}");
            if changed {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "watch notification timed out");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}
