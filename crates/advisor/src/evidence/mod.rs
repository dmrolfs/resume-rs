//! ABOUTME: Projects a resume baseline into `resume-evidence/v1`: the statements a model may cite, and its context.
//! ABOUTME: Pure over the baseline text; it reads the resume with the real parser and never touches a file or a network.

mod types;
mod walk;

pub use types::*;

use resume::diagnostics::Severity;
use resume::markdown::{self, MAX_INPUT_BYTES};
use sha2::{Digest, Sha256};

/// Projects `text`, the exact content of a baseline, into evidence.
///
/// # Errors
/// See [`EvidenceError`]; every variant means the baseline is not safe to show a model.
pub fn project(text: &str) -> Result<Evidence, EvidenceError> {
    snafu::ensure!(
        text.len() <= MAX_INPUT_BYTES,
        InputOverLimitSnafu {
            actual_bytes: text.len(),
            max_bytes: MAX_INPUT_BYTES
        }
    );
    let parsed = markdown::parse("baseline.md", text);
    let Some(resume) = parsed.valid() else {
        let errors = parsed
            .diagnostics()
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        return InvalidResumeSnafu { errors }.fail();
    };
    let source_id = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    walk::project(text, source_id, &parsed, resume)
}

#[cfg(test)]
mod tests;
