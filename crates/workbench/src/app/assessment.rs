//! Read-only assessment uses captured inventory bytes and the engine's preparation/selection APIs.
use super::{
    content::Content,
    index::{FileIndex, FileKey, FileKind},
    problem::Problem,
};
use resume::{
    diagnostics::ItemOrigin,
    markdown, select,
    spec::{self, ResolvedSpec},
};
use snafu::Snafu;
use std::{path::Path, sync::Arc};

#[derive(Debug, Snafu)]
enum AssessmentError {
    #[snafu(transparent)]
    Prepare { source: spec::items::PrepareError },
    #[snafu(transparent)]
    Spec { source: spec::SpecError },
    #[snafu(transparent)]
    Select { source: select::SelectError },
    #[snafu(transparent)]
    LetterSelect { source: resume::letter::LetterSelectError },
    #[snafu(display("Letter fields could not be resolved"))]
    Fields {
        diagnostics: Vec<resume::diagnostics::Diagnostic>,
    },
}

impl FileIndex {
    pub(super) fn assess_content(&mut self, active: Option<&Path>) {
        let resolved = active.and_then(|p| self.specs.get(p)).cloned();
        for (key, entry) in &self.entries {
            let (FileKey::Disk(path), Some(text)) = (key, &entry.text) else {
                continue;
            };
            let kind = resolved.as_deref().map_or(entry.kind, |spec| {
                if spec::error::tidy_path(&spec.content.path) == *path {
                    FileKind::Resumes
                } else if spec
                    .letter()
                    .is_some_and(|l| spec::error::tidy_path(&l.content.path) == *path)
                {
                    FileKind::Letters
                } else {
                    entry.kind
                }
            });
            let spec = resolved.as_deref().filter(|spec| match kind {
                FileKind::Resumes => spec::error::tidy_path(&spec.content.path) == *path,
                FileKind::Letters => spec
                    .letter()
                    .is_some_and(|l| spec::error::tidy_path(&l.content.path) == *path),
                _ => false,
            });
            let result = match kind {
                FileKind::Resumes => resume_content(path, text, spec),
                FileKind::Letters => letter_content(path, text, spec),
                _ => continue,
            };
            match result {
                Ok(Some(content)) => {
                    self.problems.extend(content.problems.iter().cloned());
                    self.content.insert(key.clone(), Arc::new(content));
                }
                Ok(None) => {
                    if spec.is_some() {
                        let diagnostics = match kind {
                            FileKind::Resumes => markdown::parse(path, text).diagnostics().to_vec(),
                            _ => markdown::parse_letter(path, text).diagnostics().to_vec(),
                        };
                        self.problems
                            .extend(diagnostics.iter().map(|d| Problem::diagnostic(path, d)));
                        let fallback = match entry.kind {
                            FileKind::Resumes => resume_content(path, text, None),
                            _ => letter_content(path, text, None),
                        };
                        if let Ok(Some(mut content)) = fallback {
                            content.unavailable();
                            self.content.insert(key.clone(), Arc::new(content));
                        }
                    }
                }
                Err(error) => {
                    match error {
                        AssessmentError::Fields { diagnostics } => self
                            .problems
                            .extend(diagnostics.iter().map(|d| Problem::diagnostic(path, d))),
                        AssessmentError::Spec { source } => self.problems.extend(Problem::spec(path, &source)),
                        error => self.problems.push(Problem::file(
                            active.unwrap_or(path),
                            snafu::Report::from_error(error).to_string(),
                        )),
                    }
                    let fallback = match entry.kind {
                        FileKind::Resumes => resume_content(path, text, None),
                        _ => letter_content(path, text, None),
                    };
                    if let Ok(Some(mut content)) = fallback {
                        content.unavailable();
                        self.content.insert(key.clone(), Arc::new(content));
                    }
                }
            }
        }
        // A broken active spec has no trustworthy target association or successful outcomes.
        if let Some(active) = active.filter(|_| resolved.is_none()) {
            self.problems.push(Problem::file(
                active,
                "Active spec is unavailable; no selection outcomes can be shown",
            ));
            for content in self.content.values_mut() {
                Arc::make_mut(content).unavailable();
            }
        }
    }
}

fn resume_content(path: &Path, text: &str, spec: Option<&ResolvedSpec>) -> Result<Option<Content>, AssessmentError> {
    let parsed = markdown::parse(path, text);
    let Some(baseline) = parsed.valid() else {
        return Ok(None);
    };
    let origins: Vec<_> = parsed
        .node_ranges()
        .iter()
        .map(|r| ItemOrigin::Baseline {
            file: path.to_owned(),
            range: r.clone(),
        })
        .collect();
    let prepared = spec::items::assemble_resume(
        baseline,
        &origins,
        text.len(),
        spec.map_or(&[], |s| s.additions()),
        spec.map(|s| s.item_overrides()),
    )?;
    let Some(spec) = spec else {
        return Ok(Some(Content::resume(&prepared, None)));
    };
    let result = (|| {
        let (selected, report) = select::select(prepared.resume(), &spec.to_selection()?)?;
        spec.check_picks(&report.unknown_picks)?;
        Ok(Content::resume(&prepared, Some((&selected, &report))))
    })();
    Ok(Some(retain_prepared(
        spec.sources().last().map_or(path, |s| s.path()),
        Content::resume(&prepared, None),
        result,
    )))
}

fn letter_content(path: &Path, text: &str, spec: Option<&ResolvedSpec>) -> Result<Option<Content>, AssessmentError> {
    let parsed = markdown::parse_letter(path, text);
    let Some(baseline) = parsed.valid() else {
        return Ok(None);
    };
    let Some(spec) = spec else {
        return Ok(Some(Content::letter(baseline, None)));
    };
    let Some(letter) = spec.letter() else {
        return Ok(Some(Content::letter(baseline, None)));
    };
    let prepared =
        spec::items::assemble_letter(baseline, text.len(), letter.additions(), Some(letter.item_overrides()))?;
    let result = (|| {
        let (selected, report) = resume::letter::select(&prepared, &spec.to_letter_selection()?)?;
        spec.check_letter_picks(&report.unknown_picks)?;
        let fields = letter.fields()?;
        match resume::letter::resolve(selected.clone(), &fields) {
            Ok(resolved) => Ok(Content::letter(&prepared, Some((&selected, &report, &resolved)))),
            Err(diagnostics) => FieldsSnafu { diagnostics }.fail(),
        }
    })();
    Ok(Some(retain_prepared(
        spec.sources().last().map_or(path, |s| s.path()),
        Content::letter(&prepared, None),
        result,
    )))
}

fn retain_prepared(path: &Path, mut prepared: Content, result: Result<Content, AssessmentError>) -> Content {
    match result {
        Ok(content) => content,
        Err(error) => {
            prepared.unavailable();
            prepared.problems = match error {
                AssessmentError::Fields { diagnostics } => {
                    diagnostics.iter().map(|d| Problem::diagnostic(path, d)).collect()
                }
                AssessmentError::Spec { source } => Problem::spec(path, &source),
                error => vec![Problem::file(path, snafu::Report::from_error(error).to_string())],
            };
            prepared
        }
    }
}
