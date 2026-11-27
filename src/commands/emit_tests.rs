#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::diagnostics::{SourceKind, SourceText};
    use crate::spec::content::SpecContent;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;

    #[test]
    fn content_failures_render_the_retained_unsaved_literal_in_human_and_json() {
        let dir = assert_ok!(tempfile::tempdir());
        let path = dir.path().join("position.toml");
        let literal = "\"é \\n{{unknown}}\"";
        let source = SourceText::new(path.clone(), SourceKind::Spec, format!("content = {literal}\n"));
        for wrapped in [false, true] {
            let invalid = assert_err!(SpecContent::letter(&source, 10..10 + literal.len()));
            let error = Error::Spec {
                source: Box::new(crate::spec::SpecError::AdditionContent {
                    source: Box::new(invalid),
                }),
            };
            let error = if wrapped {
                let at = crate::spec::Located {
                    path: path.as_path().into(),
                    line: 1,
                    column: 11,
                };
                Error::TracedToChoice {
                    at: vec![at.clone()],
                    source: Box::new(Error::SpecContent {
                        at,
                        source: Box::new(error),
                    }),
                }
            } else {
                error
            };
            assert_ok!(std::fs::write(&path, "different saved text"));
            for format in [OutputFormat::Human, OutputFormat::Json] {
                let mut stdout = Vec::new();
                let mut stderr = crate::diagnostics::termcolor::Buffer::no_color();
                let mut io = Io {
                    stdout: &mut stdout,
                    stderr: &mut stderr,
                };
                Reporter::new(format, dir.path(), &mut io).error(&error);
                if format == OutputFormat::Json {
                    let record: serde_json::Value = assert_ok!(serde_json::from_slice(&stdout));
                    assert_eq!(record["file"], "position.toml");
                    assert_eq!(record["line"], 1);
                    assert_eq!(record["column"], 11);
                    assert_eq!(record["fixable"], false);
                    assert_eq!(record["code"], "placeholder-unknown");
                } else {
                    let text = assert_ok!(std::str::from_utf8(stderr.as_slice()));
                    assert!(text.contains("{{unknown}}"), "{text}");
                    assert!(text.contains("in decoded Markdown at 2:1"), "{text}");
                    assert!(!text.contains("source not available"));
                    assert!(!text.contains("different saved text"));
                }
            }
        }
    }
}
