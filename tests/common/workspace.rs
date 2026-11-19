//! ABOUTME: A temporary directory standing in for the user's current directory, and a way to run `resume` in it.
//! ABOUTME: Commands run in process over injected streams, so tests see exactly what a user would see on each.

#![allow(dead_code)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use claims::assert_ok;
use resume::cli::Cli;
use resume::commands::{self, Env, Io, Outcome};
use resume::diagnostics::termcolor::Buffer;

/// A temporary directory standing in for the user's current directory.
pub struct Workspace {
    dir: tempfile::TempDir,
}

/// What a run printed and how it ended.
pub struct Run {
    pub outcome: Outcome,
    pub stdout: String,
    pub stderr: String,
}

pub fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    assert_ok!(fs::read_to_string(&path), "{}", path.display())
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            dir: assert_ok!(tempfile::tempdir()),
        }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Writes `text` to `name` under the workspace and returns its absolute path.
    pub fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.path().join(name);
        if let Some(parent) = path.parent() {
            assert_ok!(fs::create_dir_all(parent));
        }
        assert_ok!(fs::write(&path, text));
        path
    }

    pub fn out(&self) -> PathBuf {
        self.path().join("out")
    }

    /// Runs `resume <args>` with the workspace as the current directory, over the streams given.
    pub fn invoke(&self, args: &[String], stdout: &mut dyn io::Write, stderr: &mut Buffer) -> Outcome {
        let cli = assert_ok!(Cli::try_parse_validated(
            std::iter::once("resume".to_string()).chain(args.iter().cloned())
        ));
        let env = Env {
            cwd: self.path().to_path_buf(),
            resume_font_path: None,
        };
        commands::run(&cli, &env, &mut Io { stdout, stderr })
    }

    /// Runs `resume <args>` and collects what it printed; stderr has no color.
    pub fn run(&self, args: &[String]) -> Run {
        let mut stdout = Vec::new();
        let mut stderr = Buffer::no_color();
        let outcome = self.invoke(args, &mut stdout, &mut stderr);
        Run {
            outcome,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(stderr.as_slice()).into_owned(),
        }
    }

    /// The arguments of `resume build <files> --fonts bundled --out <workspace>/out <extra>`: bundled fonts so
    /// the result does not depend on the machine.
    pub fn build_args(&self, files: &[&Path], extra: &[&str]) -> Vec<String> {
        let mut args = vec!["build".to_string()];
        args.extend(files.iter().map(|f| f.display().to_string()));
        args.extend(["--fonts", "bundled", "--out"].map(String::from));
        args.push(self.out().display().to_string());
        args.extend(extra.iter().map(|a| (*a).to_string()));
        args
    }

    pub fn build(&self, files: &[&Path], extra: &[&str]) -> Run {
        self.run(&self.build_args(files, extra))
    }
}

/// A stdout that fails every write with `kind`.
pub struct FailingStdout(pub io::ErrorKind);

impl io::Write for FailingStdout {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(self.0.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(self.0.into())
    }
}
