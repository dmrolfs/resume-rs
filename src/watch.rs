//! ABOUTME: What `resume watch` listens for: which file-system events count, and when a burst of them is over.
//! ABOUTME: The decisions are plain functions over events and channels, so tests need no real watcher or timing.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::time::{Duration, Instant};

use notify::event::{AccessKind, AccessMode, EventKind};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use snafu::ResultExt;

use crate::error::{Error, ReadInputSnafu, WatchSnafu};

/// How many unprocessed signals may wait. A full channel already means a rebuild is pending, so a signal that
/// does not fit is dropped and nothing is lost; memory stays bounded however slow a build is.
const PENDING_SIGNALS: usize = 16;

/// How often a wait looks at whether it should stop.
const STOP_POLL: Duration = Duration::from_millis(100);

/// The least a burst of changes may be allowed to last before a build, however steadily they keep arriving.
const MIN_CEILING: Duration = Duration::from_secs(2);

/// Where the files `watch` cares about are, as the operating system will name them in events.
///
/// FSEvents reports real paths (`/private/var/...` for a temporary directory the user typed as `/var/...`), so
/// everything here is canonical, and an event is matched by its file name or by what it canonicalizes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watched {
    /// The directory the input is in. It is watched, not the file, because an editor that saves by renaming a
    /// new file over the old one replaces the file the watch would hold.
    input_dir: PathBuf,
    /// The input's own name.
    input_name: OsString,
    /// The input, canonical.
    input: PathBuf,
    /// The styles directory, when one was given, canonical.
    styles_dir: Option<PathBuf>,
    /// The directory the PDF goes to: its writes must not start another build, even inside `styles_dir`.
    out_dir: PathBuf,
}

impl Watched {
    /// The paths for watching `input`, `styles_dir`, and writing to `out_dir`, none of which is written.
    ///
    /// # Errors
    /// - `Error::ReadInput` - `input` does not exist or cannot be resolved
    /// - `Error::StyleRoot` - `styles_dir` cannot be resolved
    pub fn resolve(input: &Path, styles_dir: Option<&Path>, out_dir: &Path, cwd: &Path) -> Result<Self, Error> {
        let canonical = std::fs::canonicalize(input).context(ReadInputSnafu { path: input })?;
        let styles_dir = styles_dir
            .map(|dir| std::fs::canonicalize(dir).context(crate::error::StyleRootSnafu { path: dir }))
            .transpose()?;
        Ok(Self::from_parts(
            canonical,
            styles_dir,
            canonical_if_possible(&cwd.join(out_dir)),
        ))
    }

    /// The same from paths already made canonical, for tests.
    ///
    /// # Panics
    /// `input` has no parent or no file name.
    #[must_use]
    pub fn from_parts(input: PathBuf, styles_dir: Option<PathBuf>, out_dir: PathBuf) -> Self {
        let input_dir = input.parent().expect("a canonical input has a parent").to_path_buf();
        let input_name = input.file_name().expect("a canonical input has a name").to_os_string();
        Self {
            input_dir,
            input_name,
            input,
            styles_dir,
            out_dir,
        }
    }

    /// Whether `event` is a change `watch` should rebuild for.
    #[must_use]
    pub fn is_relevant(&self, event: &Event) -> bool {
        if !kind_is_a_change(&event.kind) {
            return false;
        }
        // A rescan request means events were lost, so anything may have changed.
        event.need_rescan() || event.paths.iter().any(|path| self.is_watched_file(path))
    }

    fn is_watched_file(&self, path: &Path) -> bool {
        self.is_the_input(path) || self.is_in_the_styles(path)
    }

    /// The input is found by its name in its directory, or by being the same file under another spelling
    /// (macOS may report a decomposed Unicode name for a composed one).
    fn is_the_input(&self, path: &Path) -> bool {
        let same_name = path.parent() == Some(self.input_dir.as_path())
            && path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case(&self.input_name));
        same_name
            || path == self.input
            || (path.parent() == Some(self.input_dir.as_path()) && same_file(path, &self.input))
    }

    /// A file under the styles directory counts, except the PDF being written there, and dot-files such as an
    /// editor's swap file.
    fn is_in_the_styles(&self, path: &Path) -> bool {
        let Some(styles) = &self.styles_dir else {
            return false;
        };
        let Ok(inside) = path.strip_prefix(styles) else {
            return false;
        };
        !path.starts_with(&self.out_dir)
            && !inside
                .components()
                .any(|part| matches!(part, Component::Normal(name) if name.to_string_lossy().starts_with('.')))
    }

    /// The directories to ask the operating system about: the input's directory (not recursive), and the styles
    /// directory (recursive).
    fn roots(&self) -> Vec<(&Path, RecursiveMode)> {
        let mut roots = vec![(self.input_dir.as_path(), RecursiveMode::NonRecursive)];
        if let Some(styles) = &self.styles_dir {
            roots.push((styles.as_path(), RecursiveMode::Recursive));
        }
        roots
    }
}

/// Reads of a file are not changes. Linux reports them (`OPEN`, `CLOSE_NOWRITE`) for the build's own reads,
/// which would start a build after every build, forever. A close after a write is a change.
fn kind_is_a_change(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(access) => matches!(access, AccessKind::Close(AccessMode::Write)),
        EventKind::Any | EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Other => true,
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    std::fs::canonicalize(a).is_ok_and(|a| a == b)
}

/// The canonical form of `path` when it exists; otherwise of its nearest existing parent with the rest added,
/// so a directory that will be created is still named the way events under it will be.
fn canonical_if_possible(path: &Path) -> PathBuf {
    let normalized = without_dot_dot(path);
    let path = normalized.as_path();
    let mut missing = Vec::new();
    let mut here = path;
    loop {
        if let Ok(real) = std::fs::canonicalize(here) {
            return missing.iter().rev().fold(real, |base, part| base.join(part));
        }
        match (here.parent(), here.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name.to_os_string());
                here = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// `path` with `.` dropped and each `..` cancelling the component before it, without touching the file system.
/// A `..` after a directory that does not exist yet (`styles/new/../out`) cannot be resolved by asking the
/// system, and left in, it would make the output directory look like something other than where events come from.
fn without_dot_dot(path: &Path) -> PathBuf {
    let mut parts: Vec<Component<'_>> = Vec::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if matches!(parts.last(), Some(Component::Normal(_))) => {
                parts.pop();
            }
            // The parent of the root is the root.
            Component::ParentDir if matches!(parts.last(), Some(Component::RootDir | Component::Prefix(_))) => {}
            other => parts.push(other),
        }
    }
    parts.iter().collect()
}

/// What a watch tells the loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// A file `watch` cares about changed.
    Changed,
    /// The operating system reported a problem, so a change may have been missed: rebuild to be sure.
    Uncertain(String),
}

/// What the loop is to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Rebuild; carries what the watcher complained about meanwhile, to print.
    Changed(Vec<String>),
    /// The caller asked to stop.
    Stopped,
    /// The watcher is gone: nothing more will arrive.
    WatcherGone,
}

/// Waits for a change, then for it to stop: a build starts `debounce` after the last change, not the first, so
/// the editor has finished writing the file. A change that keeps arriving cannot hold a build off for longer
/// than `ceiling`. A signal that arrived during the previous build is answered at once, then debounced.
///
/// `stop` is asked about every [`STOP_POLL`] while nothing has happened.
pub fn next_change(
    signals: &Receiver<Signal>,
    debounce: Duration,
    ceiling: Duration,
    stop: &mut dyn FnMut() -> bool,
) -> Next {
    let mut notes = Vec::new();
    let first = loop {
        match signals.recv_timeout(STOP_POLL) {
            Ok(signal) => break signal,
            Err(RecvTimeoutError::Timeout) if stop() => return Next::Stopped,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Next::WatcherGone,
        }
    };
    note(&mut notes, first);
    let started = Instant::now();
    loop {
        let left = ceiling.saturating_sub(started.elapsed());
        match signals.recv_timeout(debounce.min(left)) {
            Ok(signal) => {
                note(&mut notes, signal);
                if left.is_zero() {
                    return Next::Changed(notes);
                }
            }
            // Quiet for the debounce, or the watcher went away: build what is there. A closed channel is
            // reported by the next call.
            Err(_) => return Next::Changed(notes),
        }
    }
}

fn note(notes: &mut Vec<String>, signal: Signal) {
    if let Signal::Uncertain(text) = signal
        && !notes.contains(&text)
    {
        notes.push(text);
    }
}

/// How long a burst of changes may defer a build: at least two seconds, and twice the debounce.
#[must_use]
pub fn ceiling_for(debounce: Duration) -> Duration {
    MIN_CEILING.max(debounce.saturating_mul(2))
}

/// The watches and the signals they send, kept alive for as long as this is.
pub struct Watching {
    _watcher: RecommendedWatcher,
    signals: Receiver<Signal>,
}

impl Watching {
    /// Starts watching, before the caller builds anything, so a save made during the first build is not lost.
    ///
    /// # Errors
    /// `Error::Watch` - the operating system would not watch one of the paths.
    pub fn start(watched: &Watched) -> Result<Self, Error> {
        let (sender, signals) = sync_channel(PENDING_SIGNALS);
        let filter = Arc::new(watched.clone());
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            relay(&filter, &sender, event);
        })
        .context(WatchSnafu {
            path: watched.input_dir.clone(),
        })?;
        for (path, mode) in watched.roots() {
            watcher.watch(path, mode).context(WatchSnafu { path })?;
        }
        Ok(Self {
            _watcher: watcher,
            signals,
        })
    }

    /// The signals the watch has sent.
    #[must_use]
    pub fn signals(&self) -> &Receiver<Signal> {
        &self.signals
    }
}

/// Decides in the watcher's own thread, so an event that does not matter never queues; a full channel drops the
/// signal, since a rebuild is already waiting.
fn relay(watched: &Watched, sender: &SyncSender<Signal>, event: notify::Result<Event>) {
    let signal = match event {
        Ok(event) if watched.is_relevant(&event) => Signal::Changed,
        Ok(_) => return,
        Err(error) => Signal::Uncertain(format!("the file watcher reported a problem: {error}")),
    };
    let _ = sender.try_send(signal);
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, Flag, MetadataKind, ModifyKind, RemoveKind, RenameMode};
    use pretty_assertions::assert_eq;
    use std::sync::mpsc;

    fn watched() -> Watched {
        Watched::from_parts(
            PathBuf::from("/work/Resume.md"),
            Some(PathBuf::from("/work/styles")),
            PathBuf::from("/work/styles/out"),
        )
    }

    fn event(kind: EventKind, path: &str) -> Event {
        Event::new(kind).add_path(PathBuf::from(path))
    }

    fn modify(path: &str) -> Event {
        event(EventKind::Modify(ModifyKind::Data(DataChange::Any)), path)
    }

    // ------------------------------------------------------------------------------------------
    // what counts as a change
    // ------------------------------------------------------------------------------------------

    #[test]
    fn a_save_of_the_input_counts_however_the_system_reports_it() {
        let w = watched();
        for kind in [
            EventKind::Create(CreateKind::File),
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)),
            EventKind::Modify(ModifyKind::Name(RenameMode::To)),
            EventKind::Modify(ModifyKind::Any),
            EventKind::Remove(RemoveKind::File),
            EventKind::Access(AccessKind::Close(AccessMode::Write)),
            EventKind::Any,
            EventKind::Other,
        ] {
            assert!(w.is_relevant(&event(kind, "/work/Resume.md")), "{kind:?}");
        }
    }

    #[test]
    fn reads_of_the_input_do_not_count_or_every_build_would_start_the_next() {
        // inotify reports the build's own reads on the file it reads.
        let w = watched();
        for kind in [
            EventKind::Access(AccessKind::Open(AccessMode::Read)),
            EventKind::Access(AccessKind::Open(AccessMode::Any)),
            EventKind::Access(AccessKind::Close(AccessMode::Read)),
            EventKind::Access(AccessKind::Close(AccessMode::Any)),
            EventKind::Access(AccessKind::Read),
            EventKind::Access(AccessKind::Any),
            EventKind::Access(AccessKind::Other),
        ] {
            assert!(!w.is_relevant(&event(kind, "/work/Resume.md")), "{kind:?}");
            assert!(
                !w.is_relevant(&event(kind, "/work/styles/pragmatic/style.typ")),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn other_files_in_the_input_directory_do_not_count() {
        let w = watched();
        for path in [
            "/work/Other.md",
            "/work/.Resume.md.swp",
            "/work/Resume.md~",
            "/work/Resume.md.tmp",
            "/work/.resume-abc123.tmp",
            "/work/sub/Resume.md",
            "/Resume.md",
        ] {
            assert!(!w.is_relevant(&modify(path)), "{path}");
        }
    }

    #[test]
    fn the_input_is_matched_ignoring_ascii_case() {
        let w = watched();
        assert!(w.is_relevant(&modify("/work/resume.md")));
        assert!(w.is_relevant(&modify("/work/RESUME.MD")));
        assert!(!w.is_relevant(&modify("/work/resumes.md")));
    }

    #[test]
    fn a_rename_reports_both_paths_and_either_can_be_the_input() {
        let w = watched();
        let both = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(PathBuf::from("/work/.Resume.md.swp"))
            .add_path(PathBuf::from("/work/Resume.md"));
        assert!(w.is_relevant(&both));
        let neither = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(PathBuf::from("/work/a"))
            .add_path(PathBuf::from("/work/b"));
        assert!(!w.is_relevant(&neither));
    }

    #[test]
    fn an_event_with_no_path_counts_only_when_it_asks_for_a_rescan() {
        let w = watched();
        assert!(!w.is_relevant(&Event::new(EventKind::Modify(ModifyKind::Any))));
        assert!(w.is_relevant(&Event::new(EventKind::Other).set_flag(Flag::Rescan)));
    }

    #[test]
    fn style_files_count_at_any_depth_but_dot_files_and_the_output_do_not() {
        let w = watched();
        for path in [
            "/work/styles/pragmatic/theme.typ",
            "/work/styles/lib/components.typ",
            "/work/styles/mine/deep/er/part.typ",
            "/work/styles/mine",
        ] {
            assert!(w.is_relevant(&modify(path)), "{path}");
        }
        for path in [
            "/work/styles/mine/.theme.typ.swp",
            "/work/styles/.git/index",
            "/work/styles/out/Resume.pdf",
            "/work/styles/out/.resume-xyz.tmp",
            "/work/stylesheet/x.typ",
            "/other/styles/x.typ",
        ] {
            assert!(!w.is_relevant(&modify(path)), "{path}");
        }
    }

    #[test]
    fn without_a_styles_directory_nothing_but_the_input_counts() {
        let w = Watched::from_parts(PathBuf::from("/work/Resume.md"), None, PathBuf::from("/work/out"));
        assert!(!w.is_relevant(&modify("/work/styles/pragmatic/theme.typ")));
        assert!(w.is_relevant(&modify("/work/Resume.md")));
    }

    #[test]
    fn the_watched_roots_are_the_input_directory_flat_and_the_styles_directory_deep() {
        let w = watched();
        let roots = w.roots();
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0].0, Path::new("/work"));
        assert!(matches!(roots[0].1, RecursiveMode::NonRecursive));
        assert_eq!(roots[1].0, Path::new("/work/styles"));
        assert!(matches!(roots[1].1, RecursiveMode::Recursive));
        let alone = Watched::from_parts(PathBuf::from("/work/Resume.md"), None, PathBuf::from("/work/out"));
        assert_eq!(alone.roots().len(), 1);
    }

    #[test]
    fn the_input_is_found_under_another_spelling_of_the_same_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real = std::fs::canonicalize(dir.path()).expect("canonical");
        std::fs::write(real.join("Resume.md"), "x").expect("write");
        std::fs::write(real.join("other.md"), "x").expect("write");
        #[cfg(unix)]
        std::os::unix::fs::symlink(real.join("Resume.md"), real.join("alias.md")).expect("link");
        let w = Watched::from_parts(real.join("Resume.md"), None, real.join("out"));
        #[cfg(unix)]
        assert!(w.is_relevant(&modify(&real.join("alias.md").to_string_lossy())));
        assert!(!w.is_relevant(&modify(&real.join("other.md").to_string_lossy())));
    }

    // ------------------------------------------------------------------------------------------
    // paths
    // ------------------------------------------------------------------------------------------

    #[test]
    fn resolve_makes_every_path_canonical_and_names_an_output_that_does_not_exist_yet() {
        // `typed` is what the user wrote (on macOS `/var/...`); `real` is what the system reports (`/private/var/...`).
        let dir = tempfile::tempdir().expect("tempdir");
        let typed = dir.path().to_path_buf();
        let real = std::fs::canonicalize(&typed).expect("canonical");
        std::fs::write(typed.join("Resume.md"), "x").expect("write");
        std::fs::create_dir(typed.join("styles")).expect("styles");
        let w = Watched::resolve(
            &typed.join("Resume.md"),
            Some(&typed.join("styles")),
            Path::new("styles/out/deeper"),
            &typed,
        )
        .expect("resolves");
        assert_eq!(w.input, real.join("Resume.md"));
        assert_eq!(w.styles_dir, Some(real.join("styles")));
        assert_eq!(w.out_dir, real.join("styles/out/deeper"));
        assert!(!w.is_relevant(&modify(&real.join("styles/out/deeper/Resume.pdf").to_string_lossy())));
    }

    #[test]
    fn an_output_directory_with_dot_dot_under_a_missing_directory_is_still_found() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real = std::fs::canonicalize(dir.path()).expect("canonical");
        assert_eq!(
            canonical_if_possible(&real.join("styles/new/../out")),
            real.join("styles/out")
        );
        assert_eq!(
            canonical_if_possible(&real.join("styles/./out")),
            real.join("styles/out")
        );
        assert_eq!(canonical_if_possible(&real.join("a/b/../../c")), real.join("c"));
        assert_eq!(without_dot_dot(Path::new("/a/../..")), PathBuf::from("/"));
        assert_eq!(without_dot_dot(Path::new("../a")), PathBuf::from("../a"));
    }

    #[test]
    fn a_complaint_that_arrives_when_the_ceiling_is_reached_is_not_lost() {
        let (sender, receiver) = mpsc::channel();
        sender.send(Signal::Uncertain("late".to_string())).expect("send");
        let next = next_change(&receiver, Duration::from_secs(1), Duration::ZERO, &mut never());
        assert_eq!(next, Next::Changed(vec!["late".to_string()]));
    }

    #[test]
    fn resolve_fails_for_a_missing_input_and_a_missing_styles_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real = std::fs::canonicalize(dir.path()).expect("canonical");
        let missing =
            Watched::resolve(&real.join("nope.md"), None, Path::new("out"), &real).expect_err("missing input");
        assert!(matches!(missing, Error::ReadInput { .. }), "{missing:?}");
        std::fs::write(real.join("Resume.md"), "x").expect("write");
        let styles = Watched::resolve(
            &real.join("Resume.md"),
            Some(&real.join("none")),
            Path::new("out"),
            &real,
        )
        .expect_err("missing styles");
        assert!(matches!(styles, Error::StyleRoot { .. }), "{styles:?}");
    }

    #[test]
    fn an_absolute_output_directory_is_used_as_it_is() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real = std::fs::canonicalize(dir.path()).expect("canonical");
        let out = real.join("elsewhere/out");
        assert_eq!(canonical_if_possible(&real.join(&out)), out);
        assert_eq!(canonical_if_possible(&real.join("a/b/c")), real.join("a/b/c"));
    }

    // ------------------------------------------------------------------------------------------
    // relaying events
    // ------------------------------------------------------------------------------------------

    #[test]
    fn relay_sends_a_change_signal_for_a_relevant_event_and_nothing_for_the_rest() {
        let w = watched();
        let (sender, receiver) = sync_channel(PENDING_SIGNALS);
        relay(&w, &sender, Ok(modify("/work/Other.md")));
        assert!(receiver.try_recv().is_err());
        relay(&w, &sender, Ok(modify("/work/Resume.md")));
        assert_eq!(receiver.try_recv(), Ok(Signal::Changed));
    }

    #[test]
    fn relay_turns_a_watcher_error_into_an_uncertain_signal() {
        let (sender, receiver) = sync_channel(PENDING_SIGNALS);
        relay(&watched(), &sender, Err(notify::Error::generic("buffer overflow")));
        let Ok(Signal::Uncertain(text)) = receiver.try_recv() else {
            panic!("expected an uncertain signal")
        };
        assert!(text.contains("buffer overflow"), "{text}");
    }

    #[test]
    fn relay_drops_a_signal_when_the_queue_is_full_instead_of_blocking_or_growing() {
        let w = watched();
        let (sender, receiver) = sync_channel(PENDING_SIGNALS);
        for _ in 0..PENDING_SIGNALS * 10 {
            relay(&w, &sender, Ok(modify("/work/Resume.md")));
        }
        assert_eq!(receiver.try_iter().count(), PENDING_SIGNALS);
    }

    // ------------------------------------------------------------------------------------------
    // waiting for a change to be over
    // ------------------------------------------------------------------------------------------

    fn never() -> impl FnMut() -> bool {
        || false
    }

    #[test]
    fn a_burst_of_signals_is_one_change() {
        let (sender, receiver) = mpsc::channel();
        for _ in 0..5 {
            sender.send(Signal::Changed).expect("send");
        }
        let next = next_change(
            &receiver,
            Duration::from_millis(20),
            Duration::from_secs(5),
            &mut never(),
        );
        assert_eq!(next, Next::Changed(Vec::new()));
        // Nothing is left over to rebuild for again.
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn the_wait_is_for_quiet_after_the_last_signal_not_after_the_first() {
        let (sender, receiver) = mpsc::channel();
        let (done, finished) = mpsc::channel::<()>();
        let sender_thread = std::thread::spawn(move || {
            for _ in 0..6 {
                sender.send(Signal::Changed).expect("send");
                std::thread::sleep(Duration::from_millis(30));
            }
            // Keep the channel open, as a live watcher does, until the wait is over.
            let _ = finished.recv();
        });
        let started = Instant::now();
        let next = next_change(
            &receiver,
            Duration::from_millis(300),
            Duration::from_secs(10),
            &mut never(),
        );
        let waited = started.elapsed();
        drop(done);
        sender_thread.join().expect("join");
        assert_eq!(next, Next::Changed(Vec::new()));
        // The sixth signal comes at about 150ms; the change is reported a debounce after it, not after the first.
        assert!(waited >= Duration::from_millis(440), "{waited:?}");
    }

    #[test]
    fn a_change_that_never_stops_still_builds_by_the_ceiling() {
        let (sender, receiver) = mpsc::channel();
        let keep_going = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let flag = Arc::clone(&keep_going);
        let sender_thread = std::thread::spawn(move || {
            while flag.load(std::sync::atomic::Ordering::SeqCst) {
                if sender.send(Signal::Changed).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let started = Instant::now();
        let next = next_change(
            &receiver,
            Duration::from_millis(250),
            Duration::from_millis(1000),
            &mut never(),
        );
        let waited = started.elapsed();
        keep_going.store(false, std::sync::atomic::Ordering::SeqCst);
        sender_thread.join().expect("join");
        assert_eq!(next, Next::Changed(Vec::new()));
        assert!(waited >= Duration::from_millis(900), "{waited:?}");
        assert!(waited < Duration::from_secs(5), "{waited:?}");
    }

    #[test]
    fn stop_is_asked_about_while_nothing_happens() {
        let (_sender, receiver) = mpsc::channel::<Signal>();
        let mut asked = 0;
        let mut stop = || {
            asked += 1;
            asked >= 3
        };
        let next = next_change(&receiver, Duration::from_millis(10), Duration::from_secs(1), &mut stop);
        assert_eq!(next, Next::Stopped);
        assert_eq!(asked, 3);
    }

    #[test]
    fn a_dropped_sender_is_reported_as_the_watcher_gone() {
        let (sender, receiver) = mpsc::channel::<Signal>();
        drop(sender);
        let next = next_change(
            &receiver,
            Duration::from_millis(10),
            Duration::from_secs(1),
            &mut never(),
        );
        assert_eq!(next, Next::WatcherGone);
    }

    #[test]
    fn a_signal_waiting_from_the_last_build_is_answered_after_the_debounce_and_not_dropped() {
        let (sender, receiver) = mpsc::channel();
        sender.send(Signal::Changed).expect("send");
        let started = Instant::now();
        let next = next_change(
            &receiver,
            Duration::from_millis(50),
            Duration::from_secs(5),
            &mut never(),
        );
        assert_eq!(next, Next::Changed(Vec::new()));
        assert!(
            started.elapsed() >= Duration::from_millis(40),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn what_the_watcher_complained_about_is_reported_once_each() {
        let (sender, receiver) = mpsc::channel();
        for text in ["lost events", "lost events", "other trouble"] {
            sender.send(Signal::Uncertain(text.to_string())).expect("send");
        }
        sender.send(Signal::Changed).expect("send");
        let next = next_change(
            &receiver,
            Duration::from_millis(20),
            Duration::from_secs(5),
            &mut never(),
        );
        assert_eq!(
            next,
            Next::Changed(vec!["lost events".to_string(), "other trouble".to_string()])
        );
    }

    #[test]
    fn a_sender_that_goes_away_after_a_change_still_gets_that_change_built() {
        let (sender, receiver) = mpsc::channel();
        sender.send(Signal::Changed).expect("send");
        drop(sender);
        let next = next_change(
            &receiver,
            Duration::from_millis(20),
            Duration::from_secs(5),
            &mut never(),
        );
        assert_eq!(next, Next::Changed(Vec::new()));
        assert_eq!(
            next_change(
                &receiver,
                Duration::from_millis(20),
                Duration::from_secs(5),
                &mut never()
            ),
            Next::WatcherGone
        );
    }

    #[test]
    fn the_ceiling_is_two_seconds_or_twice_the_debounce() {
        assert_eq!(ceiling_for(Duration::from_millis(150)), Duration::from_secs(2));
        assert_eq!(ceiling_for(Duration::ZERO), Duration::from_secs(2));
        assert_eq!(ceiling_for(Duration::from_secs(5)), Duration::from_secs(10));
    }

    #[test]
    fn a_zero_debounce_builds_on_the_first_signal() {
        let (sender, receiver) = mpsc::channel();
        sender.send(Signal::Changed).expect("send");
        let started = Instant::now();
        let next = next_change(&receiver, Duration::ZERO, Duration::from_secs(2), &mut never());
        assert_eq!(next, Next::Changed(Vec::new()));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
