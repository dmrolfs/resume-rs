//! ABOUTME: `resume watch`: builds the PDF, then again whenever the resume or a style file changes.
//! ABOUTME: A build that fails is printed and the watch goes on; a watcher that cannot start or dies, or a closed stderr, ends it.

use std::path::Path;
use std::sync::mpsc::Receiver;

use super::build::{BuiltFile, Job};
use super::emit::Reporter;
use super::{Env, Io, Outcome};
use crate::cli::{Cli, WatchArgs};
use crate::error::Error;
use crate::typeset::Typesetter;
use crate::watch::{Next, Signal, Watched, Watching, ceiling_for, next_change};

/// How many builds back Typst keeps its memoized layout work, which is what makes the next rebuild fast.
const KEEP_BUILDS: usize = 10;

/// Opens a file with the program the system chooses for it.
type Opener<'a> = &'a mut dyn FnMut(&Path) -> std::io::Result<()>;

/// Runs until the process is stopped. An error here is a watch that could not start; a watcher that dies later, or a
/// stderr nobody reads any more, ends it from inside the loop.
pub(super) fn run(cli: &Cli, args: &WatchArgs, env: &Env, io: &mut Io<'_>) -> Outcome {
    let mut out = Reporter::new(cli.format, &env.cwd, io);
    let mut never = || false;
    let mut open = |path: &Path| opener::open(path).map_err(std::io::Error::other);
    match watch(cli, args, env, &mut out, &mut never, &mut open) {
        Ok(outcome) => outcome,
        Err(error) => {
            out.error(&error);
            Outcome::Failure
        }
    }
}

/// Starts the watches and runs the loop, with how to stop and how to open the PDF passed in so a test can end it
/// and see what was opened. Ends with `Success` when asked to stop and `Failure` when the watcher died.
pub(super) fn watch(
    cli: &Cli,
    args: &WatchArgs,
    env: &Env,
    out: &mut Reporter<'_, '_>,
    stop: &mut dyn FnMut() -> bool,
    open: Opener<'_>,
) -> Result<Outcome, Error> {
    let mut job = Job::open(cli, &args.out, &args.render, env)?;
    let watched = Watched::resolve(&args.file, args.render.styles_dir.as_deref(), &args.out, &env.cwd)?;
    // The watches come before the first build, so a save made while the fonts are being scanned is not lost, and
    // the first summary line means the watch is ready.
    let watching = Watching::start(&watched)?;
    let outcome = serve(&mut job, args, watching.signals(), out, stop, open);
    if outcome == Outcome::Failure {
        // `serve` fails only when the watcher is gone; its drop must not run (see `Watching::abandon`).
        watching.abandon();
    }
    Ok(outcome)
}

/// Builds, then waits for a change and builds again, until told to stop or the signals end. A failed build is
/// printed and the loop goes on; nothing but the end of the signals ends it otherwise.
fn serve(
    job: &mut Job<'_>,
    args: &WatchArgs,
    signals: &Receiver<Signal>,
    out: &mut Reporter<'_, '_>,
    stop: &mut dyn FnMut() -> bool,
    open: Opener<'_>,
) -> Outcome {
    let mut opened = args.open.then_some(false);
    let ceiling = ceiling_for(args.debounce);
    loop {
        // A directory style can appear, change, or go while watching; the built-in ones cannot.
        if args.render.styles_dir.is_some()
            && let Err(error) = job.refresh_style()
        {
            out.error(&error);
        }
        let built = job.build_one(&args.file, out);
        Typesetter::evict_cache(KEEP_BUILDS);
        open_once(&mut opened, &built, open, out);
        // The process's streams are buffered until it exits, which a watch never does. Nobody is reading a closed
        // stderr, so a watch that went on would rebuild unseen for as long as it lived: end it, as a reader that
        // closed the pipe (`| head`) asked, the way `product` treats a closed stdout.
        if !out.flush() {
            return Outcome::Success;
        }
        match next_change(signals, args.debounce, ceiling, stop) {
            Next::Changed(notes) => notes.iter().for_each(|note| out.line(&format!("warning: {note}"))),
            Next::Stopped => return Outcome::Success,
            Next::WatcherGone => {
                out.line("error: the file watcher stopped; nothing more can be seen, restart `resume watch`");
                out.flush();
                return Outcome::Failure;
            }
        }
    }
}

/// Opens the PDF the first time one has been written, even one with layout errors in it, which is when the
/// author most wants to see it. Failing to open it is a warning.
fn open_once(opened: &mut Option<bool>, built: &BuiltFile, open: Opener<'_>, out: &mut Reporter<'_, '_>) {
    let (Some(false), Some(pdf)) = (*opened, built.wrote.as_deref()) else {
        return;
    };
    *opened = Some(true);
    if let Err(error) = open(pdf) {
        out.line(&format!("warning: cannot open {}: {error}", pdf.display()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Command;
    use crate::diagnostics::termcolor::NoColor;
    use claims::assert_ok;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// How long a test waits for something the watch should do. Generous, because CI machines stall.
    const DEADLINE: Duration = Duration::from_secs(20);

    const RESUME: &str =
        "# Ada Lovelace\n\nLondon\n\n## Experience\n\n### Acme · Engineer · 2015 – 2017\n\n- Cut costs 40%.\n";

    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lock").extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Shared {
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().expect("lock")).into_owned()
        }

        /// How many files have been built, or failed to be: one summary line each, `ok ...` or `error <file> ...`.
        fn builds(&self) -> usize {
            self.text()
                .lines()
                .filter(|line| line.starts_with("ok ") || line.starts_with("error "))
                .count()
        }

        fn wait_for(&self, what: &str, mut done: impl FnMut(&str) -> bool) {
            let started = Instant::now();
            while started.elapsed() < DEADLINE {
                if done(&self.text()) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            panic!("timed out waiting for {what}; stderr so far:\n{}", self.text());
        }
    }

    /// A watch running on its own thread, in a directory of its own.
    struct Running {
        dir: tempfile::TempDir,
        stderr: Shared,
        stop: Arc<AtomicBool>,
        opened: Arc<Mutex<Vec<std::path::PathBuf>>>,
        thread: Option<std::thread::JoinHandle<Result<Outcome, String>>>,
    }

    impl Running {
        fn start(extra: &[&str]) -> Self {
            let dir = assert_ok!(tempfile::tempdir());
            let out = assert_ok!(std::fs::canonicalize(dir.path())).join("out");
            Self::start_in(dir, extra, &out)
        }

        /// A watch of `Resume.md` in `dir`, writing its PDF to `out`.
        fn start_in(dir: tempfile::TempDir, extra: &[&str], out: &Path) -> Self {
            let real = assert_ok!(std::fs::canonicalize(dir.path()));
            assert_ok!(std::fs::write(real.join("Resume.md"), RESUME));
            let mut args = watch_args(&real, &["--out", &out.display().to_string()]);
            args.extend(extra.iter().map(|a| (*a).to_string()));
            let cli = assert_ok!(Cli::try_parse_validated(args));
            let stderr = Shared::default();
            let stop = Arc::new(AtomicBool::new(false));
            let opened = Arc::new(Mutex::new(Vec::new()));
            let (thread_stderr, thread_stop, thread_opened) = (stderr.clone(), Arc::clone(&stop), Arc::clone(&opened));
            let cwd = real.clone();
            let thread = std::thread::spawn(move || {
                let Command::Watch(args) = &cli.command else {
                    return Err("not watch".to_string());
                };
                let env = Env {
                    cwd,
                    resume_font_path: None,
                };
                let mut stdout = Vec::new();
                let mut stderr = NoColor::new(thread_stderr);
                let mut io = Io {
                    stdout: &mut stdout,
                    stderr: &mut stderr,
                };
                let mut out = Reporter::new(cli.format, &env.cwd, &mut io);
                let mut stop = || thread_stop.load(Ordering::SeqCst);
                let mut open = |path: &Path| {
                    thread_opened.lock().expect("lock").push(path.to_path_buf());
                    Ok(())
                };
                watch(&cli, args, &env, &mut out, &mut stop, &mut open).map_err(|e| e.to_string())
            });
            Self {
                dir,
                stderr,
                stop,
                opened,
                thread: Some(thread),
            }
        }

        fn path(&self, name: &str) -> std::path::PathBuf {
            assert_ok!(std::fs::canonicalize(self.dir.path())).join(name)
        }

        /// Saves the way an editor does: a sibling file renamed over the original.
        fn save(&self, text: &str) {
            let sibling = self.path(".Resume.md.swp");
            assert_ok!(std::fs::write(&sibling, text));
            assert_ok!(std::fs::rename(&sibling, self.path("Resume.md")));
        }

        /// Saves until the watch has built `count` times; an event right after a watch is armed can be late or
        /// lost on macOS, so the stimulus is repeated, never the assertion.
        fn save_until_builds(&self, count: usize, text: &dyn Fn(usize) -> String) {
            let started = Instant::now();
            let mut round = 0;
            let mut last_poke = Instant::now() - Duration::from_secs(10);
            while started.elapsed() < DEADLINE {
                // The first pass always saves, so a build already under way cannot end this before the text it
                // was asked to save has been written.
                if round > 0 && self.stderr.builds() >= count {
                    return;
                }
                if last_poke.elapsed() >= Duration::from_secs(2) {
                    round += 1;
                    self.save(&text(round));
                    last_poke = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            panic!("no build number {count}; stderr:\n{}", self.stderr.text());
        }

        fn finish(mut self) -> Result<Outcome, String> {
            self.stop.store(true, Ordering::SeqCst);
            self.thread
                .take()
                .expect("running")
                .join()
                .expect("the watch thread did not panic")
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// True when the PDF's text holds `name`: the build was of the text that has it.
    fn pdf_names(pdf: &Path, name: &str) -> bool {
        let Ok(document) = lopdf::Document::load(pdf) else {
            return false;
        };
        let pages: Vec<u32> = document.get_pages().keys().copied().collect();
        document.extract_text(&pages).is_ok_and(|text| text.contains(name))
    }

    /// The arguments of `resume watch <real>/Resume.md --fonts bundled --debounce 50ms` and `extra`.
    fn watch_args(real: &Path, extra: &[&str]) -> Vec<String> {
        let mut args: Vec<String> = [
            "resume".to_string(),
            "watch".to_string(),
            real.join("Resume.md").display().to_string(),
            "--fonts".into(),
            "bundled".into(),
            "--debounce".into(),
            "50ms".into(),
        ]
        .to_vec();
        args.extend(extra.iter().map(|a| (*a).to_string()));
        args
    }

    fn resume_named(name: &str) -> String {
        RESUME.replace("Ada Lovelace", name)
    }

    #[test]
    fn watch_rebuilds_after_file_change() {
        let run = Running::start(&[]);
        run.stderr.wait_for("the first build", |t| t.contains("built in "));
        let first = assert_ok!(std::fs::read(run.path("out/Resume.pdf")));
        run.save_until_builds(2, &|n| resume_named(&format!("Ada Number{n}")));
        let second = assert_ok!(std::fs::read(run.path("out/Resume.pdf")));
        assert_ne!(first, second, "the second build is of the new text");
        assert!(second.starts_with(b"%PDF") && second.windows(5).any(|w| w == b"%%EOF"));
        assert_ok!(run.finish());
    }

    /// How long the first build took, read from its summary line, so a quiet period can be a multiple of it.
    fn first_build_millis(text: &str) -> u64 {
        text.lines()
            .find_map(|line| line.split("built in ").nth(1))
            .and_then(|rest| rest.trim().strip_suffix("ms").and_then(|n| n.parse().ok()))
            .unwrap_or(1000)
    }

    #[test]
    fn the_watch_does_not_rebuild_for_its_own_writes() {
        // Writing the PDF and its temp file next to the input are not changes. Reads are not reported by macOS or
        // Windows, so this proves nothing about them there; the filter's own tests cover the Linux read events.
        let run = Running::start(&[]);
        run.stderr.wait_for("the first build", |t| t.contains("built in "));
        std::thread::sleep(Duration::from_millis(
            1200.max(3 * first_build_millis(&run.stderr.text())),
        ));
        assert_eq!(run.stderr.builds(), 1, "{}", run.stderr.text());
        assert_ok!(run.finish());
    }

    #[test]
    fn an_output_directory_inside_the_styles_directory_does_not_start_builds_of_its_own() {
        // The PDF is written under a root that is watched recursively; only the output exclusion stops a loop.
        let dir = assert_ok!(tempfile::tempdir());
        let real = assert_ok!(std::fs::canonicalize(dir.path()));
        assert_ok!(std::fs::create_dir_all(real.join("styles/out")));
        let run = Running::start_in(
            dir,
            &["--styles-dir", &real.join("styles").display().to_string()],
            &real.join("styles/out"),
        );
        run.stderr.wait_for("the first build", |t| t.contains("built in "));
        std::thread::sleep(Duration::from_millis(
            1200.max(3 * first_build_millis(&run.stderr.text())),
        ));
        assert_eq!(run.stderr.builds(), 1, "{}", run.stderr.text());
        assert_ok!(run.finish());
    }

    #[test]
    fn a_save_made_after_the_first_build_read_the_file_is_built_next() {
        // The `--open` callback runs after the first build has read the resume and before the loop first waits,
        // which is the window the watches are armed before the first build to cover. A watch armed after the
        // first build would never see this save, and the PDF would stay the first one.
        let dir = assert_ok!(tempfile::tempdir());
        let real = assert_ok!(std::fs::canonicalize(dir.path()));
        assert_ok!(std::fs::write(real.join("Resume.md"), RESUME));
        let cli = assert_ok!(Cli::try_parse_validated(watch_args(
            &real,
            &["--open", "--out", &real.join("out").display().to_string()]
        )));
        let Command::Watch(args) = &cli.command else {
            panic!("watch")
        };
        let env = Env {
            cwd: real.clone(),
            resume_font_path: None,
        };
        let stderr = Shared::default();
        let mut stdout = Vec::new();
        let mut sink = NoColor::new(stderr.clone());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut sink,
        };
        let mut out = Reporter::new(cli.format, &env.cwd, &mut io);
        let resume = real.join("Resume.md");
        let mut saved = false;
        let started = Instant::now();
        let mut open = |_: &Path| {
            let sibling = resume.with_file_name(".Resume.md.swp");
            std::fs::write(&sibling, resume_named("Ada Early")).expect("write");
            std::fs::rename(&sibling, &resume).expect("rename");
            saved = true;
            Ok(())
        };
        let pdf = real.join("out/Resume.pdf");
        let mut stop = || pdf_names(&pdf, "Ada Early") || started.elapsed() > DEADLINE;
        assert_ok!(watch(&cli, args, &env, &mut out, &mut stop, &mut open));
        assert!(saved, "the first build did not write a PDF, so nothing was opened");
        let text = lopdf::Document::load(&pdf)
            .map_err(|e| format!("load: {e}"))
            .and_then(|d| {
                d.extract_text(&d.get_pages().keys().copied().collect::<Vec<_>>())
                    .map_err(|e| format!("extract: {e}"))
            });
        assert!(
            pdf_names(&pdf, "Ada Early"),
            "the save was lost; the PDF says {text:?}:\n{}",
            stderr.text()
        );
    }

    #[test]
    fn a_failing_build_is_printed_and_the_next_save_builds_again() {
        let run = Running::start(&[]);
        run.stderr.wait_for("the first build", |t| t.contains("built in "));
        run.save_until_builds(2, &|_| "## Experience\n\n- no name\n".to_string());
        run.stderr.wait_for("the error", |t| t.contains("no PDF written"));
        run.save_until_builds(run.stderr.builds() + 1, &|n| resume_named(&format!("Ada Again{n}")));
        run.stderr.wait_for("a good build after the failing one", |t| {
            let last = t.lines().rfind(|l| l.starts_with("ok ") || l.starts_with("error "));
            last.is_some_and(|l| l.starts_with("ok ")) && pdf_names(&run.path("out/Resume.pdf"), "Ada Again")
        });
        assert_ok!(run.finish());
    }

    #[test]
    fn a_resume_that_is_deleted_and_made_again_is_built_again() {
        let run = Running::start(&[]);
        run.stderr.wait_for("the first build", |t| t.contains("built in "));
        assert_ok!(std::fs::remove_file(run.path("Resume.md")));
        run.stderr.wait_for("the read error", |t| t.contains("cannot read"));
        let before = run.stderr.builds();
        run.save_until_builds(before + 1, &|n| resume_named(&format!("Ada Back{n}")));
        assert_ok!(run.finish());
    }

    #[test]
    fn open_opens_the_pdf_once_after_the_first_build_that_wrote_it() {
        let run = Running::start(&["--open"]);
        run.stderr.wait_for("the first build", |t| t.contains("built in "));
        run.save_until_builds(2, &|n| resume_named(&format!("Ada Open{n}")));
        run.save_until_builds(3, &|n| resume_named(&format!("Ada Open Again{n}")));
        let opened = run.opened.lock().expect("lock").clone();
        assert_eq!(opened, vec![run.path("out/Resume.pdf")]);
        assert_ok!(run.finish());
    }

    #[test]
    fn without_open_nothing_is_opened() {
        let run = Running::start(&[]);
        run.stderr.wait_for("the first build", |t| t.contains("built in "));
        assert!(run.opened.lock().expect("lock").is_empty());
        assert_ok!(run.finish());
    }

    #[test]
    fn open_waits_for_a_build_that_writes_a_pdf_and_one_with_a_layout_error_counts() {
        // The first save has errors and writes nothing; --open must not fire, or spend its one chance, then.
        let dir = assert_ok!(tempfile::tempdir());
        let real = assert_ok!(std::fs::canonicalize(dir.path()));
        let broken = real.join("Resume.md");
        assert_ok!(std::fs::write(&broken, "## Experience\n\n- no name\n"));
        let cli = assert_ok!(Cli::try_parse_validated([
            "resume".to_string(),
            "watch".into(),
            broken.display().to_string(),
            "--fonts".into(),
            "bundled".into(),
            "--out".into(),
            real.join("out").display().to_string(),
            "--open".into(),
            "--max-pages".into(),
            "1".into(),
        ]));
        let Command::Watch(args) = &cli.command else {
            panic!("watch")
        };
        let env = Env {
            cwd: real.clone(),
            resume_font_path: None,
        };
        let stderr = Shared::default();
        let mut stdout = Vec::new();
        let mut sink = NoColor::new(stderr.clone());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut sink,
        };
        let mut out = Reporter::new(cli.format, &env.cwd, &mut io);
        let mut opened = Vec::new();
        let long = format!(
            "{}{}",
            RESUME,
            "\n### Beta · Lead · 2010 – 2014\n\n- Grew 3 teams.\n".repeat(30)
        );
        let (file, text, seen_so_far) = (broken.clone(), long, stderr.clone());
        let mut round = 0;
        let mut stop = || {
            round += 1;
            // From the first wait after the broken build, save a resume too long for one page, the way an editor
            // does, and again every two seconds until the second build has happened: an event right after a watch
            // is armed can be late or lost on macOS.
            if (round == 1 || round % 20 == 0) && seen_so_far.builds() < 2 {
                let sibling = file.with_file_name(".Resume.md.swp");
                assert_ok!(std::fs::write(&sibling, &text));
                assert_ok!(std::fs::rename(&sibling, &file));
            }
            seen_so_far.builds() >= 2 || round >= 200
        };
        let mut open = |path: &Path| {
            opened.push(path.to_path_buf());
            Ok(())
        };
        assert_ok!(watch(&cli, args, &env, &mut out, &mut stop, &mut open));
        let seen = stderr.text();
        assert!(seen.contains("no PDF written"), "{seen}");
        assert_eq!(opened, vec![real.join("out/Resume.pdf")], "{seen}");
        assert!(seen.contains("page-limit-exceeded"), "{seen}");
        let summaries: Vec<&str> = seen
            .lines()
            .filter(|l| l.starts_with("ok ") || l.starts_with("error "))
            .collect();
        assert!(summaries.len() >= 2, "{seen}");
        assert!(
            summaries[0].starts_with("error ") && summaries[0].contains("no PDF written"),
            "{seen}"
        );
        assert!(
            summaries[1].starts_with("error ") && !summaries[1].contains("no PDF written"),
            "{seen}"
        );
    }

    #[test]
    fn a_failure_to_open_is_a_warning_and_the_watch_goes_on() {
        let dir = assert_ok!(tempfile::tempdir());
        let real = assert_ok!(std::fs::canonicalize(dir.path()));
        assert_ok!(std::fs::write(real.join("Resume.md"), RESUME));
        let cli = assert_ok!(Cli::try_parse_validated([
            "resume".to_string(),
            "watch".into(),
            real.join("Resume.md").display().to_string(),
            "--fonts".into(),
            "bundled".into(),
            "--out".into(),
            real.join("out").display().to_string(),
            "--open".into(),
        ]));
        let Command::Watch(args) = &cli.command else {
            panic!("watch")
        };
        let env = Env {
            cwd: real.clone(),
            resume_font_path: None,
        };
        let stderr = Shared::default();
        let mut stdout = Vec::new();
        let mut sink = NoColor::new(stderr.clone());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut sink,
        };
        let mut out = Reporter::new(cli.format, &env.cwd, &mut io);
        let mut ticks = 0;
        let mut stop = || {
            ticks += 1;
            ticks >= 3
        };
        let mut open = |_: &Path| Err(std::io::Error::other("no viewer"));
        assert_ok!(watch(&cli, args, &env, &mut out, &mut stop, &mut open));
        let seen = stderr.text();
        assert!(seen.contains("warning: cannot open "), "{seen}");
        assert!(seen.contains("no viewer"), "{seen}");
    }

    #[test]
    fn a_missing_input_or_style_ends_the_watch_before_any_build() {
        let dir = assert_ok!(tempfile::tempdir());
        let real = assert_ok!(std::fs::canonicalize(dir.path()));
        let attempt = |extra: &[&str], file: &str| {
            let mut args = vec![
                "resume".to_string(),
                "watch".into(),
                real.join(file).display().to_string(),
            ];
            args.extend(["--fonts".into(), "bundled".into()]);
            args.extend(extra.iter().map(|a| (*a).to_string()));
            let cli = assert_ok!(Cli::try_parse_validated(args));
            let Command::Watch(args) = &cli.command else {
                panic!("watch")
            };
            let env = Env {
                cwd: real.clone(),
                resume_font_path: None,
            };
            let mut stdout = Vec::new();
            let mut sink = NoColor::new(Shared::default());
            let mut io = Io {
                stdout: &mut stdout,
                stderr: &mut sink,
            };
            let mut out = Reporter::new(cli.format, &env.cwd, &mut io);
            watch(&cli, args, &env, &mut out, &mut || true, &mut |_| Ok(()))
        };
        assert!(matches!(attempt(&[], "nope.md"), Err(Error::ReadInput { .. })));
        assert_ok!(std::fs::write(real.join("Resume.md"), RESUME));
        assert!(matches!(
            attempt(&["--style", "nope"], "Resume.md"),
            Err(Error::UnknownStyle { .. })
        ));
        let missing = real.join("no-styles").display().to_string();
        assert!(matches!(
            attempt(&["--styles-dir", &missing], "Resume.md"),
            Err(Error::StyleRoot { .. })
        ));
    }

    /// The PDF the style in `styles_dir` named `name` builds for `RESUME`, made directly with the typesetter.
    fn pdf_of_style(styles_dir: &Path, name: &str) -> Vec<u8> {
        use crate::model::options::FontsMode;
        let text = RESUME;
        let parsed = crate::markdown::parse("Resume.md", text);
        let model = parsed.valid().expect("a valid resume");
        let fonts = Arc::new(assert_ok!(
            crate::typeset::fonts::FontSet::builder()
                .mode(FontsMode::Bundled)
                .build()
        ));
        let style = assert_ok!(crate::style::resolve(
            &assert_ok!(crate::model::StyleName::try_new(name)),
            Some(styles_dir)
        ));
        let compiled =
            assert_ok!(Typesetter::new(fonts).compile(model, &style, crate::typeset::RenderOptions::default()));
        assert_ok!(compiled.export_pdf("Resume"))
    }

    /// Waits until the PDF the watch wrote equals `wanted`.
    fn wait_for_pdf(path: &Path, wanted: &[u8], stderr: &Shared) {
        let started = Instant::now();
        while started.elapsed() < DEADLINE {
            if std::fs::read(path).is_ok_and(|now| now == wanted) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the PDF never became the one of the wanted style:\n{}", stderr.text());
    }

    fn write_style(styles: &Path, from: &str, as_name: &str) {
        let files = assert_ok!(crate::style::builtin_files(&assert_ok!(
            crate::model::StyleName::try_new(from)
        )));
        for (path, bytes) in files {
            let path = path
                .strip_prefix(from)
                .map_or(path.clone(), |rest| Path::new(as_name).join(rest));
            let target = styles.join(path);
            assert_ok!(std::fs::create_dir_all(target.parent().expect("parent")));
            assert_ok!(std::fs::write(target, bytes));
        }
    }

    #[test]
    fn a_style_made_and_then_edited_while_watching_is_the_style_the_next_builds_use() {
        let dir = assert_ok!(tempfile::tempdir());
        let real = assert_ok!(std::fs::canonicalize(dir.path()));
        assert_ok!(std::fs::create_dir(real.join("styles")));
        let styles = real.join("styles");
        let run = Running::start_in(dir, &["--styles-dir", &styles.display().to_string()], &real.join("out"));
        let pdf = real.join("out/Resume.pdf");
        run.stderr
            .wait_for("the first build, with the built-in style", |t| t.contains("built in "));
        // A style called `pragmatic` appears in the styles directory with the files of `mono`.
        write_style(&styles, "mono", "pragmatic");
        let before = pdf_of_style(&styles, "pragmatic");
        wait_for_pdf(&pdf, &before, &run.stderr);
        // Then one of its files is edited, which is what a style author does all day.
        let theme = styles.join("pragmatic/theme.typ");
        let original = assert_ok!(std::fs::read_to_string(&theme));
        let edited = original.replacen("size: (body: 11pt", "size: (body: 13pt", 1);
        assert_ne!(
            original, edited,
            "the edit must find the text it changes, or the test proves nothing"
        );
        assert_ok!(std::fs::write(&theme, edited));
        let wanted = pdf_of_style(&styles, "pragmatic");
        assert_ne!(
            before, wanted,
            "the edit must change the PDF, or the wait below passes at once"
        );
        wait_for_pdf(&pdf, &wanted, &run.stderr);
        assert_ok!(run.finish());
    }

    #[test]
    fn paths_given_in_a_non_canonical_spelling_are_still_watched() {
        // On macOS a temporary directory is /var/..., which FSEvents reports as /private/var/...; the watch must
        // compare in one form. Elsewhere this is the same as the plain case.
        let dir = assert_ok!(tempfile::tempdir());
        let typed = dir.path().to_path_buf();
        assert_ok!(std::fs::write(typed.join("Resume.md"), RESUME));
        let cli = assert_ok!(Cli::try_parse_validated([
            "resume".to_string(),
            "watch".into(),
            typed.join("Resume.md").display().to_string(),
            "--fonts".into(),
            "bundled".into(),
            "--debounce".into(),
            "50ms".into(),
            "--out".into(),
            typed.join("out").display().to_string(),
        ]));
        let Command::Watch(args) = &cli.command else {
            panic!("watch")
        };
        let env = Env {
            cwd: typed.clone(),
            resume_font_path: None,
        };
        let stderr = Shared::default();
        let mut stdout = Vec::new();
        let mut sink = NoColor::new(stderr.clone());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut sink,
        };
        let mut out = Reporter::new(cli.format, &env.cwd, &mut io);
        let (flag, started) = (Arc::new(AtomicBool::new(false)), Instant::now());
        let saver = {
            let (typed, stderr, flag) = (typed.clone(), stderr.clone(), Arc::clone(&flag));
            std::thread::spawn(move || {
                let mut round = 0;
                while !flag.load(Ordering::SeqCst) && started.elapsed() < DEADLINE {
                    if stderr.builds() >= 1 {
                        round += 1;
                        let sibling = typed.join(".Resume.md.swp");
                        std::fs::write(&sibling, resume_named(&format!("Ada Typed{round}"))).expect("write");
                        std::fs::rename(&sibling, typed.join("Resume.md")).expect("rename");
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }
            })
        };
        let pdf = typed.join("out/Resume.pdf");
        let mut stop = || pdf_names(&pdf, "Ada Typed") || started.elapsed() > DEADLINE;
        assert_ok!(watch(&cli, args, &env, &mut out, &mut stop, &mut |_| Ok(())));
        flag.store(true, Ordering::SeqCst);
        saver.join().expect("saver");
        assert!(pdf_names(&pdf, "Ada Typed"), "{}", stderr.text());
    }

    // ------------------------------------------------------------------------------------------
    // the loop over plain signals
    // ------------------------------------------------------------------------------------------

    fn serve_with(signals: Vec<Signal>, keep_open: bool) -> (Outcome, String) {
        let dir = assert_ok!(tempfile::tempdir());
        let real = assert_ok!(std::fs::canonicalize(dir.path()));
        assert_ok!(std::fs::write(real.join("Resume.md"), RESUME));
        let cli = assert_ok!(Cli::try_parse_validated(watch_args(
            &real,
            &["--out", &real.join("out").display().to_string()]
        )));
        let Command::Watch(args) = &cli.command else {
            panic!("watch")
        };
        let env = Env {
            cwd: real.clone(),
            resume_font_path: None,
        };
        let mut job = assert_ok!(Job::open(&cli, &args.out, &args.render, &env));
        let stderr = Shared::default();
        let mut stdout = Vec::new();
        let mut sink = NoColor::new(stderr.clone());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut sink,
        };
        let mut out = Reporter::new(cli.format, &env.cwd, &mut io);
        let (sender, receiver) = std::sync::mpsc::channel();
        for signal in signals {
            sender.send(signal).expect("send");
        }
        let _held = keep_open.then_some(sender);
        let mut asked = 0;
        let mut stop = || {
            asked += 1;
            asked > 3
        };
        let outcome = serve(&mut job, args, &receiver, &mut out, &mut stop, &mut |_| Ok(()));
        (outcome, stderr.text())
    }

    /// A stderr whose reader has gone: writes are taken and a flush finds the pipe closed.
    struct ClosedPipe(Shared);

    impl Write for ClosedPipe {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.write(bytes)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
    }

    #[test]
    fn a_closed_stderr_ends_the_watch_after_the_first_build_instead_of_running_unseen() {
        let dir = assert_ok!(tempfile::tempdir());
        let real = assert_ok!(std::fs::canonicalize(dir.path()));
        assert_ok!(std::fs::write(real.join("Resume.md"), RESUME));
        let cli = assert_ok!(Cli::try_parse_validated(watch_args(
            &real,
            &["--out", &real.join("out").display().to_string()]
        )));
        let Command::Watch(args) = &cli.command else {
            panic!("watch")
        };
        let env = Env {
            cwd: real.clone(),
            resume_font_path: None,
        };
        let seen = Shared::default();
        let mut stdout = Vec::new();
        let mut sink = NoColor::new(ClosedPipe(seen.clone()));
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut sink,
        };
        let mut out = Reporter::new(cli.format, &env.cwd, &mut io);
        let started = Instant::now();
        // `stop` would end it only after the deadline; the closed pipe has to end it first.
        let mut stop = || started.elapsed() > Duration::from_secs(5);
        let outcome = assert_ok!(watch(&cli, args, &env, &mut out, &mut stop, &mut |_| Ok(())));
        assert_eq!(outcome, Outcome::Success);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the watch waited for a reader that is gone"
        );
        assert_eq!(seen.builds(), 1, "{}", seen.text());
    }

    #[test]
    fn a_watcher_that_is_gone_ends_the_watch_with_failure_and_says_why() {
        let (outcome, text) = serve_with(Vec::new(), false);
        assert_eq!(outcome, Outcome::Failure);
        assert!(text.contains("error: the file watcher stopped"), "{text}");
        assert_eq!(
            text.lines().filter(|l| l.starts_with("ok ")).count(),
            1,
            "the first build still ran: {text}"
        );
    }

    #[test]
    fn being_told_to_stop_ends_the_watch_with_success() {
        let (outcome, text) = serve_with(Vec::new(), true);
        assert_eq!(outcome, Outcome::Success, "{text}");
    }

    #[test]
    fn what_the_watcher_complained_about_is_printed_as_a_warning_and_the_build_still_runs() {
        let (_, text) = serve_with(
            vec![Signal::Uncertain("events were lost".to_string()), Signal::Changed],
            true,
        );
        assert!(text.contains("warning: events were lost"), "{text}");
        assert_eq!(text.lines().filter(|l| l.starts_with("ok ")).count(), 2, "{text}");
    }

    // ------------------------------------------------------------------------------------------
    // what a build reports, and failures that end or do not end the watch
    // ------------------------------------------------------------------------------------------

    fn job_in(real: &Path, extra: &[&str]) -> (Cli, Env) {
        let mut args = watch_args(real, &["--out", &real.join("out").display().to_string()]);
        args.extend(extra.iter().map(|a| (*a).to_string()));
        (
            assert_ok!(Cli::try_parse_validated(args)),
            Env {
                cwd: real.to_path_buf(),
                resume_font_path: None,
            },
        )
    }

    fn build_once(real: &Path, extra: &[&str]) -> (BuiltFile, String) {
        let (cli, env) = job_in(real, extra);
        let Command::Watch(args) = &cli.command else {
            panic!("watch")
        };
        let job = assert_ok!(Job::open(&cli, &args.out, &args.render, &env));
        let stderr = Shared::default();
        let mut stdout = Vec::new();
        let mut sink = NoColor::new(stderr.clone());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut sink,
        };
        let mut out = Reporter::new(cli.format, &env.cwd, &mut io);
        let built = job.build_one(&args.file, &mut out);
        (built, stderr.text())
    }

    #[test]
    fn a_pdf_that_was_written_is_reported_and_one_that_was_not_is_not() {
        let dir = assert_ok!(tempfile::tempdir());
        let real = assert_ok!(std::fs::canonicalize(dir.path()));
        // Written.
        assert_ok!(std::fs::write(real.join("Resume.md"), RESUME));
        let (built, text) = build_once(&real, &[]);
        assert_eq!(built.wrote, Some(real.join("out/Resume.pdf")), "{text}");
        assert_eq!(built.outcome, Outcome::Success);
        // Written, with a layout error: the outcome is a failure and the PDF is still there to open.
        let long = format!(
            "{RESUME}{}",
            "\n### Beta · Lead · 2010 – 2014\n\n- Grew 3 teams.\n".repeat(30)
        );
        assert_ok!(std::fs::write(real.join("Resume.md"), long));
        let (built, text) = build_once(&real, &["--max-pages", "1"]);
        assert_eq!(
            (built.outcome, built.wrote),
            (Outcome::Failure, Some(real.join("out/Resume.pdf"))),
            "{text}"
        );
        // Format errors: nothing is written.
        assert_ok!(std::fs::write(real.join("Resume.md"), "## Experience\n\n- no name\n"));
        let (built, text) = build_once(&real, &[]);
        assert_eq!((built.outcome, built.wrote), (Outcome::Failure, None), "{text}");
        // An input that cannot be read.
        assert_ok!(std::fs::remove_file(real.join("Resume.md")));
        let (built, text) = build_once(&real, &[]);
        assert_eq!((built.outcome, built.wrote), (Outcome::Failure, None), "{text}");
        assert!(text.contains("cannot read"), "{text}");
    }

    #[test]
    fn a_pdf_that_could_not_be_written_is_not_reported_so_open_never_shows_a_stale_one() {
        let dir = assert_ok!(tempfile::tempdir());
        let real = assert_ok!(std::fs::canonicalize(dir.path()));
        assert_ok!(std::fs::write(real.join("Resume.md"), RESUME));
        // `out` is a file, so the PDF cannot go there on any system.
        assert_ok!(std::fs::write(real.join("out"), "in the way"));
        let (built, text) = build_once(&real, &[]);
        assert_eq!((built.outcome, built.wrote), (Outcome::Failure, None), "{text}");
        assert!(text.contains("no PDF written"), "{text}");
    }

    #[test]
    fn a_watcher_that_cannot_start_is_an_error_that_names_the_directory_and_ends_the_watch() {
        let dir = assert_ok!(tempfile::tempdir());
        let real = assert_ok!(std::fs::canonicalize(dir.path()));
        let gone = Watched::from_parts_for_test(real.join("gone/Resume.md"), None, real.join("out"));
        let Err(error) = Watching::start(&gone) else {
            panic!("a directory that is not there cannot be watched")
        };
        assert!(matches!(error, Error::Watch { .. }), "{error:?}");
        let text = error.to_string();
        assert!(text.starts_with("cannot watch ") && text.contains("gone"), "{text}");
    }

    #[test]
    fn a_style_that_goes_away_while_watching_is_reported_and_the_watch_builds_again_when_it_returns() {
        let dir = assert_ok!(tempfile::tempdir());
        let real = assert_ok!(std::fs::canonicalize(dir.path()));
        let styles = real.join("styles");
        // `mine` has no built-in twin, so removing it is a real failure and not a fall back to a built-in.
        write_style(&styles, "mono", "mine");
        let run = Running::start_in(
            dir,
            &["--styles-dir", &styles.display().to_string(), "--style", "mine"],
            &real.join("out"),
        );
        run.stderr.wait_for("the first build", |t| t.contains("built in "));
        assert_ok!(std::fs::remove_dir_all(styles.join("mine")));
        run.save_until_builds(2, &|n| resume_named(&format!("Ada Gone{n}")));
        // The style cannot be found again, so that is said; the style kept from before reads its files from the
        // directory that is gone, so the build fails too. Neither ends the watch.
        run.stderr.wait_for("both errors", |t| {
            t.contains("error: unknown style `mine`") && t.contains("file not found (searched at mine/style.typ)")
        });
        // The style comes back, and the next save builds with it.
        write_style(&styles, "mono", "mine");
        let before = run.stderr.builds();
        run.save_until_builds(before + 1, &|n| resume_named(&format!("Ada Back{n}")));
        run.stderr.wait_for("a good build after the style returned", |t| {
            t.lines()
                .rfind(|l| l.starts_with("ok ") || l.starts_with("error "))
                .is_some_and(|l| l.starts_with("ok "))
                && pdf_names(&run.path("out/Resume.pdf"), "Ada Back")
        });
        assert_ok!(run.finish());
    }

    #[cfg(windows)]
    #[test]
    fn watch_continues_after_output_locked() {
        use std::os::windows::fs::OpenOptionsExt;
        let run = Running::start(&[]);
        run.stderr.wait_for("the first build", |t| t.contains("built in "));
        // A viewer that shares nothing, as Acrobat does: the PDF cannot be replaced while it is open.
        let viewer = assert_ok!(
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(run.path("out/Resume.pdf"))
        );
        run.save_until_builds(2, &|n| resume_named(&format!("Ada Locked{n}")));
        run.stderr.wait_for("the lock to be reported", |t| {
            t.contains("is in use by another program")
        });
        assert!(
            run.stderr.text().contains("close the PDF viewer"),
            "{}",
            run.stderr.text()
        );
        drop(viewer);
        // `save_until_builds` saves before it looks, so this text is written whatever builds are still running.
        let before = run.stderr.builds();
        run.save_until_builds(before + 1, &|n| resume_named(&format!("Ada Free{n}")));
        let started = Instant::now();
        while !pdf_names(&run.path("out/Resume.pdf"), "Ada Free") && started.elapsed() < DEADLINE {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            pdf_names(&run.path("out/Resume.pdf"), "Ada Free"),
            "{}",
            run.stderr.text()
        );
        assert_ok!(run.finish());
    }
}
