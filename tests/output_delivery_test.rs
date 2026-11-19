//! ABOUTME: Command outcomes include delivery of stdout, including failures delayed by buffering.
//! ABOUTME: Closed pipes stay quiet; real write and flush failures must never look like success.

use std::io::{self, BufWriter, Write};

use pretty_assertions::assert_eq;
use resume::commands::Outcome;
use resume::diagnostics::termcolor::Buffer;

#[path = "common/workspace.rs"]
mod workspace;
use workspace::{FailingStdout, Workspace};

#[test]
fn product_write_and_buffer_flush_failures_fail_the_command() {
    for command in ["styles", "licenses"] {
        for buffered in [false, true] {
            let work = Workspace::new();
            let mut failing = FailingStdout(io::ErrorKind::PermissionDenied);
            let mut buffer = BufWriter::new(FailingStdout(io::ErrorKind::PermissionDenied));
            let stdout: &mut dyn Write = if buffered { &mut buffer } else { &mut failing };
            let mut stderr = Buffer::no_color();
            let outcome = work.invoke(&[command.to_string()], stdout, &mut stderr);
            assert_eq!(outcome, Outcome::Failure, "{command}, buffered={buffered}");
            let message = String::from_utf8_lossy(stderr.as_slice());
            assert_eq!(message.matches("cannot write to stdout").count(), 1, "{message}");
        }
    }
}

#[test]
fn buffered_json_flush_failure_fails_an_otherwise_successful_build() {
    let work = Workspace::new();
    let input = work.file("Resume.md", &workspace::fixture("full.md"));
    let args = work.build_args(&[&input], &["--format", "json"]);
    let mut stdout = BufWriter::with_capacity(1024 * 1024, FailingStdout(io::ErrorKind::PermissionDenied));
    let mut stderr = Buffer::no_color();
    let outcome = work.invoke(&args, &mut stdout, &mut stderr);
    assert_eq!(outcome, Outcome::Failure);
    assert!(work.out().join("Resume.pdf").is_file());
    let message = String::from_utf8_lossy(stderr.as_slice());
    assert!(message.contains("cannot write to stdout"), "{message}");
}

/// One failed write followed by successful writes and flushes.
struct RecoveringWriter {
    first: Option<io::Result<usize>>,
}

impl Write for RecoveringWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.first.take().unwrap_or(Ok(bytes.len()))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn write_failure_stays_failed_after_successful_writes_and_flush() {
    let work = Workspace::new();
    let first = work.file("First.md", &workspace::fixture("full.md"));
    let second = work.file("Second.md", &workspace::fixture("full.md"));
    for args in [
        vec!["styles".to_string()],
        work.build_args(&[&first, &second], &["--format", "json"]),
    ] {
        for zero_write in [false, true] {
            let mut writer = RecoveringWriter {
                first: Some(if zero_write {
                    Ok(0)
                } else {
                    Err(io::ErrorKind::PermissionDenied.into())
                }),
            };
            let mut stderr = Buffer::no_color();
            let outcome = work.invoke(&args, &mut writer, &mut stderr);
            assert_eq!(outcome, Outcome::Failure);
            let message = String::from_utf8_lossy(stderr.as_slice());
            let expected = if args[0] == "styles" {
                "cannot write to stdout"
            } else {
                "could not write diagnostics"
            };
            assert_eq!(message.matches(expected).count(), 1, "{message}");
        }
    }
    assert!(work.out().join("Second.pdf").is_file());
}

#[test]
fn interrupted_writes_are_retried_without_failing_the_command() {
    let work = Workspace::new();
    let mut writer = RecoveringWriter {
        first: Some(Err(io::ErrorKind::Interrupted.into())),
    };
    let mut stderr = Buffer::no_color();
    assert_eq!(
        work.invoke(&["styles".to_string()], &mut writer, &mut stderr),
        Outcome::Success
    );
    assert_eq!(stderr.as_slice(), b"");
}

struct FlushFailure(io::ErrorKind);

impl Write for FlushFailure {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(self.0.into())
    }
}

#[test]
fn a_flush_failure_is_reported_but_a_closed_pipe_is_quiet() {
    for (kind, expected) in [
        (io::ErrorKind::PermissionDenied, Outcome::Failure),
        (io::ErrorKind::BrokenPipe, Outcome::Success),
    ] {
        let work = Workspace::new();
        let mut stderr = Buffer::no_color();
        let outcome = work.invoke(&["styles".to_string()], &mut FlushFailure(kind), &mut stderr);
        assert_eq!(outcome, expected);
        let message = String::from_utf8_lossy(stderr.as_slice());
        if kind == io::ErrorKind::BrokenPipe {
            assert_eq!(message, "");
        } else {
            assert!(message.contains("cannot write to stdout"), "{message}");
        }
    }
}
