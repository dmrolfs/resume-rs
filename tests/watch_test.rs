//! ABOUTME: `resume watch` as a running program: its real stderr, its exit code, and a real file watcher.
//! ABOUTME: The watch logic is tested in process; this proves `main` wires it up and a kill leaves no half PDF.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use claims::assert_ok;

const DEADLINE: Duration = Duration::from_secs(30);

const RESUME: &str =
    "# Ada Lovelace\n\nLondon\n\n## Experience\n\n### Acme · Engineer · 2015 – 2017\n\n- Cut costs 40%.\n";

/// A `resume watch` process that is killed when this is dropped, so a failed assertion cannot leave it running
/// (on Windows it would keep the temporary directory open).
struct Watch {
    child: Child,
    lines: Receiver<String>,
    seen: Vec<String>,
}

impl Watch {
    fn start(dir: &Path, extra: &[&str]) -> Self {
        let mut child = assert_ok!(
            Command::new(env!("CARGO_BIN_EXE_resume"))
                .current_dir(dir)
                .args(["watch", "Resume.md", "--fonts", "bundled", "--debounce", "50ms"])
                .args(extra)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
        );
        let stderr = child.stderr.take().expect("piped");
        let (sender, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            lines,
            seen: Vec::new(),
        }
    }

    /// Waits for a line that starts with `prefix`, after those already read, and returns it.
    fn wait_for_line(&mut self, prefix: &str, deadline: Duration) -> Option<String> {
        let started = Instant::now();
        while let Some(left) = deadline.checked_sub(started.elapsed()) {
            match self.lines.recv_timeout(left.min(Duration::from_millis(200))) {
                Ok(line) => {
                    self.seen.push(line.clone());
                    if line.starts_with(prefix) {
                        return Some(line);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return None,
            }
        }
        None
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn workspace() -> (tempfile::TempDir, PathBuf) {
    let dir = assert_ok!(tempfile::tempdir());
    let real = assert_ok!(std::fs::canonicalize(dir.path()));
    assert_ok!(std::fs::write(real.join("Resume.md"), RESUME));
    (dir, real)
}

fn save(dir: &Path, text: &str) {
    let sibling = dir.join(".Resume.md.swp");
    assert_ok!(std::fs::write(&sibling, text));
    assert_ok!(std::fs::rename(&sibling, dir.join("Resume.md")));
}

#[test]
fn the_watch_builds_at_once_then_again_after_a_save_and_the_pdf_is_whole() {
    let (_guard, dir) = workspace();
    let mut watch = Watch::start(&dir, &[]);
    let first = watch.wait_for_line("ok ", DEADLINE);
    assert!(first.is_some(), "no first build: {:?}", watch.seen);
    let pdf = dir.join("out/Resume.pdf");
    let before = assert_ok!(std::fs::read(&pdf));
    // The first event after a watch is armed can be late or lost on macOS, so the save is repeated until a
    // second build is seen; the assertion is not.
    let started = Instant::now();
    let mut second = None;
    let mut round = 0;
    while second.is_none() && started.elapsed() < DEADLINE {
        round += 1;
        save(&dir, &RESUME.replace("Ada Lovelace", &format!("Ada Number{round}")));
        second = watch.wait_for_line("ok ", Duration::from_secs(3));
    }
    assert!(second.is_some(), "no second build: {:?}", watch.seen);
    let after = assert_ok!(std::fs::read(&pdf));
    assert_ne!(before, after);
    // Killed while idle; that a write is never seen half done is the Writer's property, tested in `output.rs`.
    drop(watch);
    let bytes = assert_ok!(std::fs::read(&pdf));
    assert!(bytes.starts_with(b"%PDF") && bytes.windows(5).any(|w| w == b"%%EOF"));
}

#[test]
fn a_missing_resume_exits_1_at_once() {
    let (_guard, dir) = workspace();
    let output = assert_ok!(
        Command::new(env!("CARGO_BIN_EXE_resume"))
            .current_dir(&dir)
            .args(["watch", "Nope.md", "--fonts", "bundled"])
            .output()
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot read"),
        "{output:?}"
    );
}

#[test]
fn dry_run_is_a_usage_error_with_exit_code_2() {
    let (_guard, dir) = workspace();
    for args in [["--dry-run", "watch", "Resume.md"], ["watch", "Resume.md", "--dry-run"]] {
        let output = assert_ok!(
            Command::new(env!("CARGO_BIN_EXE_resume"))
                .current_dir(&dir)
                .args(args)
                .output()
        );
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("--dry-run cannot be used with `watch`"),
            "{output:?}"
        );
    }
}
