//! Bounded folder inventory. Classification never depends on validation succeeding.
use super::problem::Problem;
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use resume::{
    markdown,
    spec::{
        self,
        layer::{Kind, Layer, SpecLayer},
    },
    style,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use strum::Display;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Display)]
pub enum FileKind {
    Resumes,
    Letters,
    Specs,
    Profiles,
    Styles,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum FileKey {
    Disk(PathBuf),
    Builtin(PathBuf),
}

pub struct FileEntry {
    pub kind: FileKind,
    pub text: Option<Arc<str>>,
    pub problems: Vec<Problem>,
}

#[derive(Default)]
pub struct FileIndex {
    pub entries: BTreeMap<FileKey, FileEntry>,
    pub problems: Vec<Problem>,
}

impl FileIndex {
    pub fn scan(root: &Path) -> Self {
        let mut index = Self::default();
        let mut remaining = 10_000;
        index.walk(root, root, 0, &mut remaining, &mut (32 * 1024 * 1024));
        index.resolve_specs();
        index.builtins();
        index
    }

    fn walk(&mut self, root: &Path, dir: &Path, depth: usize, remaining: &mut usize, bytes: &mut usize) {
        if depth > 32 || *remaining == 0 || *bytes == 0 {
            self.problems.push(Problem::file(
                dir,
                "Folder scan limit reached; some files are not listed",
            ));
            return;
        }
        let children = match std::fs::read_dir(dir) {
            Ok(children) => children,
            Err(e) => {
                self.problems.push(Problem::file(dir, e.to_string()));
                return;
            }
        };
        for child in children {
            if *remaining == 0 || *bytes == 0 {
                self.problems.push(Problem::file(
                    dir,
                    "Folder scan limit reached; some files are not listed",
                ));
                break;
            }
            *remaining -= 1;
            let child = match child {
                Ok(child) => child,
                Err(e) => {
                    self.problems.push(Problem::file(dir, e.to_string()));
                    continue;
                }
            };
            let path = child.path();
            let name = child.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || matches!(name.as_ref(), "target" | "out" | "node_modules") {
                continue;
            }
            match child.file_type() {
                Ok(ty) if ty.is_dir() && !candidate(&path) => self.walk(root, &path, depth + 1, remaining, bytes),
                Ok(ty) if ty.is_symlink() => {
                    if candidate(&path) {
                        self.entries.insert(
                            FileKey::Disk(path.clone()),
                            FileEntry {
                                kind: path_kind(path.strip_prefix(root).unwrap_or(&path)),
                                text: None,
                                problems: vec![Problem::file(
                                    &path,
                                    "Symbolic links are not followed by the folder scan",
                                )],
                            },
                        );
                    } else if std::fs::metadata(&path).is_ok_and(|metadata| metadata.is_dir()) {
                        // Non-candidate files and dangling non-candidate links are outside the inventory.
                        self.problems.push(Problem::file(
                            &path,
                            "Symbolic links are not followed by the folder scan",
                        ));
                    }
                }
                Ok(_) => {
                    if candidate(&path) {
                        let mut entry = classify_as(&path, path_kind(path.strip_prefix(root).unwrap_or(&path)));
                        let len = entry.text.as_ref().map_or(0, |s| s.len());
                        if len > *bytes {
                            entry.text = None;
                            entry.problems.push(Problem::file(&path, "Folder text budget exceeded"));
                        }
                        *bytes = bytes.saturating_sub(len);
                        self.entries.insert(FileKey::Disk(path.clone()), entry);
                    }
                }
                Err(e) => self.problems.push(Problem::file(&path, e.to_string())),
            }
        }
    }

    fn resolve_specs(&mut self) {
        let snapshots = super::profiles::Snapshots::default();
        for (key, entry) in &self.entries {
            if let (FileKey::Disk(path), Some(text)) = (key, &entry.text) {
                snapshots.text.borrow_mut().insert(path.clone(), text.clone());
            }
        }
        for (key, entry) in &mut self.entries {
            if entry.kind != FileKind::Specs {
                continue;
            }
            let (FileKey::Disk(path), Some(text)) = (key, &entry.text) else {
                continue;
            };
            let profiles = super::profiles::Profiles {
                directory: spec::chain::DirSource::new(spec::chain::DirSource::default_for(path)),
                snapshots: &snapshots,
            };
            entry.problems =
                match SpecLayer::from_text(path, text).and_then(|layer| spec::resolve_with(layer, &profiles)) {
                    Ok(spec) => spec.content_warnings().map(|d| Problem::diagnostic(path, d)).collect(),
                    Err(error) => Problem::spec(path, &error),
                };
        }
        for (path, text) in snapshots.text.into_inner() {
            let key = FileKey::Disk(path.clone());
            if self.entries.get(&key).is_some_and(|entry| entry.text.is_some()) {
                continue;
            }
            let problems = Layer::from_text(Kind::Profile, &path, &text)
                .err()
                .map_or_else(Vec::new, |e| Problem::spec(&path, &e));
            self.entries.insert(
                key,
                FileEntry {
                    kind: FileKind::Profiles,
                    text: Some(text),
                    problems,
                },
            );
        }
        for (path, problems) in snapshots.failures.into_inner() {
            self.entries.entry(FileKey::Disk(path)).or_insert(FileEntry {
                kind: FileKind::Profiles,
                text: None,
                problems,
            });
        }
    }

    fn builtins(&mut self) {
        let catalog = match style::catalog(None) {
            Ok(catalog) => catalog,
            Err(e) => {
                self.problems
                    .push(Problem::file(Path::new("<built-in>"), e.to_string()));
                return;
            }
        };
        for entry in catalog {
            let Ok(name) = resume::model::StyleName::try_new(entry.name) else {
                continue;
            };
            match style::builtin_files(&name) {
                Ok(files) => {
                    for (path, bytes) in files {
                        match std::str::from_utf8(bytes) {
                            Ok(text) => {
                                self.entries.insert(
                                    FileKey::Builtin(path),
                                    FileEntry {
                                        kind: FileKind::Styles,
                                        text: Some(Arc::from(text)),
                                        problems: vec![],
                                    },
                                );
                            }
                            Err(e) => self.problems.push(Problem::file(&path, e.to_string())),
                        }
                    }
                }
                Err(e) => self
                    .problems
                    .push(Problem::file(Path::new("<built-in>"), e.to_string())),
            }
        }
    }
}

fn candidate(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "md" | "toml" | "typ"))
}

fn path_kind(path: &Path) -> FileKind {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "typ" => FileKind::Styles,
        "toml" if path.parent().and_then(Path::file_name).is_some_and(|s| s == "profiles") => FileKind::Profiles,
        "toml" => FileKind::Specs,
        _ if path.components().any(|c| c.as_os_str() == "letters") => FileKind::Letters,
        _ => FileKind::Resumes,
    }
}

fn letter_heading(text: &str) -> bool {
    let mut heading = None;
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H2,
                ..
            }) => heading = Some(String::new()),
            Event::Text(text) | Event::Code(text) => {
                if let Some(h) = &mut heading {
                    h.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(HeadingLevel::H2))
                if heading
                    .take()
                    .is_some_and(|h| matches!(h.trim(), "Greeting" | "Body" | "Closing")) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

pub fn classify(path: &Path) -> FileEntry {
    classify_as(path, path_kind(path))
}

fn classify_as(path: &Path, kind: FileKind) -> FileEntry {
    // Only README.md beside a style entry is documentation, not arbitrary neighboring resumes.
    let style_readme = path
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("README.md"))
        && path.parent().is_some_and(|dir| dir.join("style.typ").is_file());
    let mut entry = FileEntry {
        kind: if style_readme { FileKind::Styles } else { kind },
        text: None,
        problems: vec![],
    };
    let text = match resume::input::read_text(path, path) {
        Ok(text) => text,
        Err(e) => {
            entry.problems.push(Problem::file(path, e.to_string()));
            return entry;
        }
    };
    if text.len() > markdown::MAX_INPUT_BYTES {
        entry
            .problems
            .push(Problem::file(path, "File exceeds the 1 MiB display limit"));
        return entry;
    }
    if entry.kind == FileKind::Resumes && letter_heading(&text) {
        entry.kind = FileKind::Letters;
    }
    entry.problems = match entry.kind {
        FileKind::Resumes => markdown::parse(path, &text)
            .diagnostics()
            .iter()
            .map(|d| Problem::diagnostic(path, d))
            .collect(),
        FileKind::Letters => markdown::parse_letter(path, &text)
            .diagnostics()
            .iter()
            .map(|d| Problem::diagnostic(path, d))
            .collect(),
        FileKind::Profiles => match Layer::from_text(Kind::Profile, path, &text) {
            Ok(_) => vec![],
            Err(e) => Problem::spec(path, &e),
        },
        FileKind::Specs => match SpecLayer::from_text(path, &text) {
            Ok(_) => vec![],
            Err(e) => Problem::spec(path, &e),
        },
        FileKind::Styles if style_readme => vec![],
        FileKind::Styles => {
            let source = typst_syntax::Source::detached(text.clone());
            let (errors, warnings) = source.root().errors_and_warnings();
            errors
                .into_iter()
                .chain(warnings)
                .map(|d| {
                    let mut p = Problem::file(path, d.message.to_string());
                    p.severity = if d.is_error {
                        resume::diagnostics::Severity::Error
                    } else {
                        resume::diagnostics::Severity::Warning
                    };
                    let range = match d.span.get() {
                        typst_syntax::DiagSpanKind::Number { num, sub_range, .. } => source.range(num, sub_range),
                        typst_syntax::DiagSpanKind::Range { range, .. } => Some(range),
                        typst_syntax::DiagSpanKind::Detached => None,
                    };
                    if let Some(range) = range {
                        p.location = super::problem::Location::Bytes(range);
                    }
                    p
                })
                .collect()
        }
    };
    entry.text = Some(Arc::from(text));
    entry
}

#[cfg(test)]
mod tests {
    use super::{FileIndex, FileKey, FileKind, classify};
    use claims::{assert_ok, assert_some};
    use pretty_assertions::assert_eq;
    #[test]
    fn test_style_readme_is_documentation_but_neighboring_resume_is_not() {
        let dir = assert_ok!(tempfile::tempdir());
        let style = dir.path().join("styles/mine");
        assert_ok!(std::fs::create_dir_all(&style));
        assert_ok!(std::fs::create_dir_all(dir.path().join("styles/lib")));
        for (path, text) in [
            (style.join("style.typ"), "#let x = 1"),
            (style.join("README.md"), "# My style\n## Files\nDocumentation"),
            (style.join("resume.md"), "# Ada"),
            (dir.path().join("README.md"), "# Root"),
            (dir.path().join("styles/lib/shared.typ"), "#let shared = 2"),
        ] {
            assert_ok!(std::fs::write(path, text));
        }
        let index = FileIndex::scan(dir.path());
        for path in [
            style.join("style.typ"),
            style.join("README.md"),
            dir.path().join("styles/lib/shared.typ"),
        ] {
            let entry = assert_some!(index.entries.get(&FileKey::Disk(path)));
            assert_eq!(entry.kind, FileKind::Styles);
            assert!(entry.problems.is_empty(), "{:?}", entry.problems);
        }
        for path in [style.join("resume.md"), dir.path().join("README.md")] {
            assert_eq!(
                assert_some!(index.entries.get(&FileKey::Disk(path))).kind,
                FileKind::Resumes
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_skipped_directory_symlink_is_reported() {
        let dir = assert_ok!(tempfile::tempdir());
        let target = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::write(target.path().join("style.typ"), "#let x = 1"));
        let link = dir.path().join("styles");
        assert_ok!(std::os::unix::fs::symlink(target.path(), &link));
        assert_ok!(std::fs::write(target.path().join("photo.png"), b"image"));
        assert_ok!(std::os::unix::fs::symlink(
            target.path().join("photo.png"),
            dir.path().join("photo.png")
        ));
        assert_ok!(std::os::unix::fs::symlink(
            target.path().join("missing"),
            dir.path().join("broken-link")
        ));
        let index = FileIndex::scan(dir.path());
        assert_eq!(index.problems.len(), 1);
        assert!(
            index
                .problems
                .iter()
                .any(|p| p.file == link && p.message.contains("Symbolic links"))
        );
        assert!(!index.entries.contains_key(&FileKey::Disk(link.join("style.typ"))));
    }

    #[test]
    fn test_builtin_shared_files_and_disk_style_limits() {
        let dir = assert_ok!(tempfile::tempdir());
        let path = dir.path().join("large.typ");
        assert_ok!(std::fs::write(&path, " ".repeat(resume::markdown::MAX_INPUT_BYTES + 1)));
        let index = FileIndex::scan(dir.path());
        let large = assert_some!(index.entries.get(&FileKey::Disk(path)));
        assert!(large.text.is_none());
        assert!(!large.problems.is_empty());
        for style in assert_ok!(resume::style::catalog(None)) {
            let name = assert_ok!(resume::model::StyleName::try_new(style.name));
            for (path, bytes) in assert_ok!(resume::style::builtin_files(&name)) {
                let entry = assert_some!(index.entries.get(&FileKey::Builtin(path)));
                assert_eq!(entry.kind, FileKind::Styles);
                assert_eq!(entry.text.as_deref(), Some(assert_ok!(std::str::from_utf8(bytes))));
            }
        }
        assert!(
            index
                .entries
                .contains_key(&FileKey::Builtin("lib/components.typ".into()))
        );
    }

    #[test]
    fn unreadable_external_profile_remains_visible() {
        let dir = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir(dir.path().join("positions")));
        assert_ok!(std::fs::create_dir(dir.path().join("profiles")));
        let path = dir.path().join("profiles/p.toml");
        assert_ok!(std::fs::write(&path, [255]));
        assert_ok!(std::fs::write(
            dir.path().join("positions/job.toml"),
            "extends = ['p']\n[build]\ncontent = 'r.md'\n"
        ));
        let index = FileIndex::scan(&dir.path().join("positions"));
        let entry = assert_some!(index.entries.get(&FileKey::Disk(path)));
        assert!(entry.text.is_none());
        assert!(!entry.problems.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn checked_profile_alias_gets_captured_text_not_scan_placeholder() {
        let dir = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir_all(dir.path().join("profiles/nested")));
        assert_ok!(std::fs::create_dir(dir.path().join("positions")));
        assert_ok!(std::fs::write(
            dir.path().join("profiles/nested/p.toml"),
            "[select]\nwrong = true\n"
        ));
        let path = dir.path().join("profiles/p.toml");
        assert_ok!(std::os::unix::fs::symlink("nested/p.toml", &path));
        assert_ok!(std::fs::write(
            dir.path().join("positions/job.toml"),
            "extends = ['p']\n[build]\ncontent = 'r.md'\n"
        ));
        let index = FileIndex::scan(dir.path());
        let entry = assert_some!(index.entries.get(&FileKey::Disk(path)));
        assert_eq!(entry.text.as_deref(), Some("[select]\nwrong = true\n"));
        assert!(!entry.problems.is_empty());
    }

    #[test]
    fn invalid_typst_is_visible_with_parser_location() {
        let dir = assert_ok!(tempfile::tempdir());
        let path = dir.path().join("style.typ");
        assert_ok!(std::fs::write(&path, "#let x = ("));
        let entry = classify(&path);
        assert_eq!(entry.kind, FileKind::Styles);
        assert!(!entry.problems.is_empty());
        assert!(matches!(
            entry.problems[0].location,
            super::super::problem::Location::Bytes(_)
        ));
    }

    #[test]
    fn profile_validation_uses_the_same_snapshot_as_display() {
        let dir = assert_ok!(tempfile::tempdir());
        let path = dir.path().join("profile.toml");
        assert_ok!(std::fs::write(&path, "broken"));
        let layer = assert_ok!(resume::spec::layer::Layer::from_text(
            resume::spec::layer::Kind::Profile,
            &path,
            "[select]\ntags = ['platform']\n"
        ));
        assert_eq!(layer.source().text(), "[select]\ntags = ['platform']\n");
    }

    #[test]
    fn letter_folder_hint_is_relative_and_scan_text_budget_is_visible() {
        let dir = assert_ok!(tempfile::tempdir());
        let root = dir.path().join("letters/job");
        assert_ok!(std::fs::create_dir_all(&root));
        assert_ok!(std::fs::write(root.join("resume.md"), "# Ada\n"));
        let index = FileIndex::scan(&root);
        let entry = assert_some!(index.entries.get(&FileKey::Disk(root.join("resume.md"))));
        assert_eq!(entry.kind, FileKind::Resumes);
        let mut limited = FileIndex::default();
        limited.walk(&root, &root, 0, &mut 100, &mut 1);
        let entry = assert_some!(limited.entries.get(&FileKey::Disk(root.join("resume.md"))));
        assert!(entry.text.is_none());
        assert!(entry.problems.iter().any(|p| p.message.contains("budget")));
    }

    #[test]
    fn broken_and_unreadable_candidates_remain_in_inventory() {
        let dir = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir(dir.path().join("letters")));
        for (file, text) in [
            ("broken.md", "broken"),
            ("letters/broken.md", "broken"),
            ("bad.toml", "broken"),
            ("code.md", "```\n## Greeting\n```"),
            ("letter.md", "## Body\nHi"),
        ] {
            assert_ok!(std::fs::write(dir.path().join(file), text));
        }
        assert_ok!(std::fs::write(dir.path().join("unreadable.md"), [255]));
        let index = FileIndex::scan(dir.path());
        assert_eq!(
            index.entries.keys().filter(|k| matches!(k, FileKey::Disk(_))).count(),
            6
        );
        for (file, kind) in [
            ("broken.md", FileKind::Resumes),
            ("letters/broken.md", FileKind::Letters),
            ("letter.md", FileKind::Letters),
            ("code.md", FileKind::Resumes),
            ("bad.toml", FileKind::Specs),
        ] {
            let entry = assert_some!(index.entries.get(&FileKey::Disk(dir.path().join(file))));
            assert_eq!(entry.kind, kind);
            assert!(!entry.problems.is_empty());
        }
        assert!(classify(&dir.path().join("unreadable.md")).text.is_none());
    }
}
