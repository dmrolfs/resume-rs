//! ABOUTME: `doctor --fix`: plans the parser's mechanical edits, proves they keep the resume's meaning, and diffs them.
//! ABOUTME: Pure: it never touches a file. The command writes the result through `output::Writer`.

use std::ops::Range;
use std::path::Path;

use similar::TextDiff;
use snafu::Snafu;

use crate::diagnostics::{Category, Diagnostic, Edit};
use crate::markdown::{Parsed, parse};
use crate::model::Resume;

/// Why a file cannot be fixed. Nothing is changed in that case.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum FixError {
    /// Two edits claim the same bytes: a parser bug, so no winner is picked.
    #[snafu(display(
        "two fixes overlap at bytes {} and {}; nothing was changed",
        describe(first),
        describe(second)
    ))]
    Overlap { first: Range<usize>, second: Range<usize> },

    /// An edit's range is outside the text or inside a character: a parser bug.
    #[snafu(display("a fix at bytes {} is not inside the text; nothing was changed", describe(range)))]
    BadRange { range: Range<usize> },

    /// An edit would add or remove a line ending, which `--fix` promises never to do.
    #[snafu(display("a fix at bytes {} would change a line ending; nothing was changed", describe(range)))]
    LineEnding { range: Range<usize> },

    /// There is no model, so the fixes cannot be checked against one.
    #[snafu(display("the file has no name heading, so its fixes cannot be checked"))]
    NoModel,

    /// The fixed text no longer parses to the same resume or to the same other findings.
    #[snafu(display("the fixes would change what the resume says ({what}); nothing was changed"))]
    ChangesMeaning { what: &'static str },
}

fn describe(range: &Range<usize>) -> String {
    format!("{}..{}", range.start, range.end)
}

/// The fixed text of one file, and what was fixed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixed {
    /// The whole fixed text.
    pub text: String,
    /// How many findings had their edit applied. Counts findings, not edits: two findings can share one edit.
    pub fixed: usize,
}

/// The edits `parsed` offers, as `(edit, finding)` pairs in the order they were reported.
///
/// Only a Format finding may carry an edit: the parser proved those, and content advice is never auto-fixed.
fn edits_of(parsed: &Parsed) -> Vec<&Edit> {
    parsed
        .diagnostics()
        .iter()
        .filter(|d| d.code.category() == Category::Format)
        .filter_map(|d| d.fix.as_ref())
        .collect()
}

/// Plans the fix of `text`, checking it before it is offered.
///
/// Returns `Ok(None)` when there is nothing to fix. Otherwise the edits are sorted, checked not to overlap or to
/// touch a line ending, applied to the original text (so LF, CRLF, a lone CR, mixed endings, and a BOM are all
/// kept as they are), and the result is parsed again: it must give the same model and the same findings, less
/// the ones the edits fixed. The hard-break edits were proven only as a whole set, and `parse` hands every one
/// of them over or none, so they are never applied in part.
///
/// # Errors
/// [`FixError`] - the edits conflict, would change a line ending, cannot be checked, or change the meaning.
pub fn plan(file: &Path, text: &str, parsed: &Parsed) -> Result<Option<Fixed>, FixError> {
    let fixable = parsed
        .diagnostics()
        .iter()
        .filter(|d| d.code.category() == Category::Format && d.fix.is_some())
        .count();
    let mut edits = edits_of(parsed);
    if edits.is_empty() {
        return Ok(None);
    }
    edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
    edits.dedup_by(|a, b| a.range == b.range && a.replacement == b.replacement);
    check_edits(text, &edits)?;
    let fixed = Edit::apply_all(text, &edits);
    check_meaning(file, parsed, &fixed)?;
    Ok(Some(Fixed {
        text: fixed,
        fixed: fixable,
    }))
}

/// The edits are sorted by start. Overlap is a parser bug; touching is fine.
fn check_edits(text: &str, edits: &[&Edit]) -> Result<(), FixError> {
    for pair in edits.windows(2) {
        let [first, second] = pair else { continue };
        let tied_insertions =
            first.range.start == second.range.start && (first.range.is_empty() || second.range.is_empty());
        if second.range.start < first.range.end || tied_insertions {
            return OverlapSnafu {
                first: first.range.clone(),
                second: second.range.clone(),
            }
            .fail();
        }
    }
    for edit in edits {
        let Some(covered) = text.get(edit.range.clone()) else {
            return BadRangeSnafu {
                range: edit.range.clone(),
            }
            .fail();
        };
        if covered.contains(['\r', '\n']) || edit.replacement.contains(['\r', '\n']) {
            return LineEndingSnafu {
                range: edit.range.clone(),
            }
            .fail();
        }
    }
    Ok(())
}

/// The fixed text must read as the same resume, with the same findings except the fixed ones.
///
/// The models are compared whole, through [`label_blind`]: moving the colon of `**Label**: text` into the bold
/// makes `Label` a recognized label, which changes the model on purpose, so that one difference is projected
/// away and every other is not. One comparison for every fix set, whether or not it holds a relabel.
fn check_meaning(file: &Path, before: &Parsed, fixed: &str) -> Result<(), FixError> {
    let after = parse(file, fixed);
    let (Some(old), Some(new)) = (before.best_effort(), after.best_effort()) else {
        return NoModelSnafu.fail();
    };
    if label_blind(old) != label_blind(new) {
        return ChangesMeaningSnafu {
            what: "the model differs",
        }
        .fail();
    }
    // The findings must be the same ones, less those the edits fixed (all of them: a plan applies every edit).
    let mut expected: Vec<Key> = before
        .diagnostics()
        .iter()
        .filter(|d| !(d.code.category() == Category::Format && d.fix.is_some()))
        .map(key)
        .collect();
    let mut now = shape(after.diagnostics());
    expected.sort();
    now.sort();
    if expected == now {
        Ok(())
    } else {
        ChangesMeaningSnafu {
            what: "the other findings differ",
        }
        .fail()
    }
}

/// The model as JSON with each bullet reduced to what a reader sees: its label, a colon, its content, as one
/// line of text, and its links in order. A bullet with a label and one whose label is the same words in bold
/// before a colon come out the same; any other change to a bullet, and every change outside bullets, shows.
fn label_blind(resume: &Resume) -> serde_json::Value {
    let mut value = serde_json::to_value(resume).unwrap_or(serde_json::Value::Null);
    project(&mut value);
    value
}

fn project(value: &mut serde_json::Value) {
    use serde_json::Value;
    match value {
        Value::Object(map) if map.contains_key("label") && map.contains_key("content") => {
            let mut text = String::new();
            let mut links = Vec::new();
            if let Some(label) = map.get("label").filter(|l| !l.is_null()) {
                visible(label, &mut text, &mut links);
                text.push_str(": ");
            }
            if let Some(content) = map.get("content") {
                visible(content, &mut text, &mut links);
            }
            let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
            *value = serde_json::json!({ "bullet": collapsed, "links": links });
        }
        Value::Object(map) => map.values_mut().for_each(project),
        Value::Array(items) => items.iter_mut().for_each(project),
        _ => {}
    }
}

/// The visible text of a run of inlines, with the address of each link in order.
fn visible(inlines: &serde_json::Value, text: &mut String, links: &mut Vec<String>) {
    use serde_json::Value;
    match inlines {
        Value::Array(items) => items.iter().for_each(|item| visible(item, text, links)),
        Value::Object(map) => {
            match map.get("kind").and_then(Value::as_str) {
                Some("text" | "code") => text.push_str(map.get("text").and_then(Value::as_str).unwrap_or("")),
                Some("link") => links.push(map.get("url").map(Value::to_string).unwrap_or_default()),
                _ => {}
            }
            if let Some(content) = map.get("content") {
                visible(content, text, links);
            }
        }
        _ => {}
    }
}

/// A finding without its position, which moves when text is deleted.
type Key = (crate::diagnostics::Severity, &'static str, String);

fn key(diagnostic: &Diagnostic) -> Key {
    (
        diagnostic.severity,
        <&'static str>::from(diagnostic.code),
        diagnostic.message.clone(),
    )
}

fn shape(diagnostics: &[Diagnostic]) -> Vec<Key> {
    diagnostics.iter().map(key).collect()
}

/// A unified diff of `before` to `after`, three lines of context, headed by `path`.
#[must_use]
pub fn unified_diff(path: &str, before: &str, after: &str) -> String {
    TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_none, assert_ok, assert_some};
    use pretty_assertions::{assert_eq, assert_ne};

    const FILE: &str = "r.md";

    fn plan_of(text: &str) -> Result<Option<Fixed>, FixError> {
        let parsed = parse(FILE, text);
        plan(Path::new(FILE), text, &parsed)
    }

    fn fixed(text: &str) -> Fixed {
        assert_some!(assert_ok!(plan_of(text)))
    }

    const DASHES: &str = "# Ada\n\nLondon\n\n## Experience\n\n### Acme · Engineer · 2015 - 2017\n\n- Led 3 teams.\n";

    #[test]
    fn a_period_hyphen_becomes_an_en_dash_and_nothing_else_changes() {
        let fixed = fixed(DASHES);
        assert_eq!(fixed.text, DASHES.replace("2015 - 2017", "2015 \u{2013} 2017"));
        assert_eq!(fixed.fixed, 1);
    }

    #[test]
    fn a_file_with_nothing_to_fix_has_no_plan() {
        assert_none!(assert_ok!(plan_of(
            "# Ada\n\nLondon\n\n## Experience\n\n### Acme · 2015 – 2017\n\n- Led 3 teams.\n"
        )));
    }

    #[test]
    fn several_fixes_are_applied_in_one_pass() {
        let text = "# Ada\n\nLondon\n\n## Experience\n\n### Acme | Engineer | 2015 - Current\n\n- **Stack**: Rust 3.\n";
        let fixed = fixed(text);
        assert!(fixed.fixed >= 2, "{}", fixed.fixed);
        assert!(
            fixed.text.contains("Acme · Engineer · 2015 \u{2013} Present"),
            "{}",
            fixed.text
        );
        assert!(fixed.text.contains("**Stack:**"), "{}", fixed.text);
    }

    #[test]
    fn crlf_endings_are_kept_byte_for_byte() {
        let crlf = DASHES.replace('\n', "\r\n");
        let fixed = fixed(&crlf);
        assert_eq!(fixed.text.matches("\r\n").count(), crlf.matches("\r\n").count());
        assert_eq!(
            fixed.text.matches('\n').count(),
            fixed.text.matches("\r\n").count(),
            "a bare LF appeared"
        );
        assert_eq!(fixed.text, crlf.replace("2015 - 2017", "2015 \u{2013} 2017"));
    }

    #[test]
    fn mixed_endings_and_a_bom_are_kept() {
        let text = "\u{feff}# Ada\r\n\r\nLondon\n\n## Experience\r\n\n### Acme · 2015 - 2017\r\n\n- Led 3 teams.\n";
        let fixed = fixed(text);
        assert!(fixed.text.starts_with('\u{feff}'));
        assert_eq!(fixed.text, text.replace("2015 - 2017", "2015 \u{2013} 2017"));
    }

    #[test]
    fn a_fix_is_idempotent() {
        let once = fixed(DASHES);
        assert_none!(assert_ok!(plan_of(&once.text)), "a second run finds nothing to fix");
    }

    #[test]
    fn a_hard_break_is_removed_without_touching_the_line_ending() {
        let text = "# Ada\n\nLondon\n\n## Experience\n\n### Acme · 2015 – 2017\n\n- Led 3  \n  teams.\n";
        let fixed = fixed(text);
        assert_eq!(
            fixed.text,
            "# Ada\n\nLondon\n\n## Experience\n\n### Acme · 2015 – 2017\n\n- Led 3\n  teams.\n"
        );
    }

    #[test]
    fn overlapping_edits_fail_the_file_and_touching_ones_do_not() {
        let edit = |range: Range<usize>| Edit {
            range,
            replacement: "x".into(),
        };
        let (a, b, c) = (edit(0..4), edit(2..6), edit(4..6));
        assert!(matches!(
            check_edits("abcdefgh", &[&a, &b]),
            Err(FixError::Overlap { .. })
        ));
        assert_ok!(check_edits("abcdefgh", &[&a, &c]));
        let insert = edit(3..3);
        let tie = edit(3..5);
        assert!(matches!(
            check_edits("abcdefgh", &[&insert, &tie]),
            Err(FixError::Overlap { .. })
        ));
    }

    #[test]
    fn an_edit_that_covers_or_adds_a_line_ending_is_refused() {
        let covers = Edit {
            range: 0..3,
            replacement: "x".into(),
        };
        assert!(matches!(
            check_edits("a\r\nb", &[&covers]),
            Err(FixError::LineEnding { .. })
        ));
        let adds = Edit {
            range: 0..1,
            replacement: "x\n".into(),
        };
        assert!(matches!(check_edits("abc", &[&adds]), Err(FixError::LineEnding { .. })));
    }

    #[test]
    fn a_file_with_no_name_is_not_fixed() {
        let text = "## Experience\n\n### Acme · 2015 - 2017\n\n- Led 3 teams.\n";
        let parsed = parse(FILE, text);
        assert!(
            parsed.diagnostics().iter().any(|d| d.fix.is_some()),
            "the fixture must offer an edit"
        );
        assert_eq!(assert_err!(plan(Path::new(FILE), text, &parsed)), FixError::NoModel);
    }

    #[test]
    fn a_file_with_an_error_that_has_no_edit_is_still_fixed_where_it_can_be() {
        let text = "# Ada\n\nLondon\n\n## Experience\n\n### Acme · 2015 - 2017\n\n### Broken\n\n- Led 3 teams.\n";
        let fixed = fixed(text);
        assert!(fixed.text.contains("2015 \u{2013} 2017"));
        assert!(fixed.text.contains("### Broken"));
    }

    /// The text of `DASHES` with its dash fix applied, which `check_meaning` must accept.
    fn dash_fixed() -> String {
        DASHES.replace("2015 - 2017", "2015 \u{2013} 2017")
    }

    #[test]
    fn the_exact_fix_passes_the_meaning_check() {
        let parsed = parse(FILE, DASHES);
        assert_ok!(check_meaning(Path::new(FILE), &parsed, &dash_fixed()));
    }

    #[test]
    fn a_fix_applied_together_with_a_change_to_the_text_is_refused_for_the_model() {
        // The dash fix is applied, and so is "Led 3" to "Led 4": the findings still match, the model does not.
        let parsed = parse(FILE, DASHES);
        let tampered = dash_fixed().replace("Led 3 teams", "Led 4 teams");
        let err = assert_err!(check_meaning(Path::new(FILE), &parsed, &tampered));
        assert_eq!(
            err,
            FixError::ChangesMeaning {
                what: "the model differs"
            }
        );
    }

    #[test]
    fn a_fix_that_leaves_the_model_but_adds_a_finding_is_refused_for_the_findings() {
        // The same resume, but with a hard break added: the model is unchanged, a Format finding is new.
        let parsed = parse(FILE, DASHES);
        let tampered = dash_fixed().replace("- Led 3 teams.", "- Led 3 teams.  \n  More.");
        let err = assert_err!(check_meaning(Path::new(FILE), &parsed, &tampered));
        assert!(matches!(err, FixError::ChangesMeaning { .. }), "{err}");
    }

    #[test]
    fn a_relabel_does_not_hide_a_change_made_by_another_edit_in_the_same_set() {
        // The relabel is applied, and so is a change of "Rust" to "Go": the label projection must not mask it.
        let text = "# Ada\n\nLondon\n\n## Skills\n\n- **Languages**: Rust\n";
        let parsed = parse(FILE, text);
        let tampered = text.replace("**Languages**:", "**Languages:**").replace("Rust", "Go");
        let err = assert_err!(check_meaning(Path::new(FILE), &parsed, &tampered));
        assert_eq!(
            err,
            FixError::ChangesMeaning {
                what: "the model differs"
            }
        );
        let honest = text.replace("**Languages**:", "**Languages:**");
        assert_ok!(check_meaning(Path::new(FILE), &parsed, &honest));
    }

    #[test]
    fn a_relabel_with_a_changed_period_in_the_same_set_is_refused() {
        let text = "# Ada\n\nLondon\n\n## Experience\n\n### Acme · 2015 - 2017\n\n- **Stack**: Rust 3.\n";
        let parsed = parse(FILE, text);
        let tampered = "# Ada\n\nLondon\n\n## Experience\n\n### Acme · 2015 \u{2013} 2018\n\n- **Stack:** Rust 3.\n";
        assert!(matches!(
            assert_err!(check_meaning(Path::new(FILE), &parsed, tampered)),
            FixError::ChangesMeaning { .. }
        ));
    }

    #[test]
    fn a_label_and_its_text_swapped_between_bullets_is_not_mistaken_for_a_relabel() {
        let text = "# Ada\n\nLondon\n\n## Skills\n\n- **A**: one\n- **B**: two\n";
        let parsed = parse(FILE, text);
        let swapped = "# Ada\n\nLondon\n\n## Skills\n\n- **A:** two\n- **B:** one\n";
        assert!(matches!(
            assert_err!(check_meaning(Path::new(FILE), &parsed, swapped)),
            FixError::ChangesMeaning { .. }
        ));
    }

    #[test]
    fn a_range_outside_the_text_or_inside_a_character_is_refused_not_applied() {
        let edit = |range: Range<usize>| Edit {
            range,
            replacement: "x".into(),
        };
        let past = edit(2..9);
        assert!(matches!(check_edits("abc", &[&past]), Err(FixError::BadRange { .. })));
        let inside = edit(1..2);
        assert!(matches!(
            check_edits("\u{e9}a", &[&inside]),
            Err(FixError::BadRange { .. })
        ));
        assert_ok!(check_edits("\u{e9}a", &[&edit(0..2)]));
    }

    #[test]
    fn a_colon_moved_into_the_bold_relabels_the_bullet_and_keeps_the_structure() {
        let text = "# Ada\n\nLondon\n\n## Skills\n\n- **Languages**: Rust\n";
        let fixed = fixed(text);
        assert_eq!(fixed.text, text.replace("**Languages**:", "**Languages:**"));
        let after = parse(FILE, &fixed.text);
        let model = assert_some!(after.best_effort());
        assert_ne!(
            Some(model),
            parse(FILE, text).best_effort(),
            "the label is recognized now"
        );
    }

    #[test]
    fn the_diff_has_the_old_and_new_lines_and_a_header() {
        let diff = unified_diff("r.md", "a\nb\nc\n", "a\nB\nc\n");
        assert!(diff.starts_with("--- a/r.md\n+++ b/r.md\n"), "{diff}");
        assert!(diff.contains("\n-b\n") && diff.contains("\n+B\n"), "{diff}");
    }

    #[test]
    fn the_diff_of_a_crlf_file_keeps_its_line_endings() {
        let diff = unified_diff("r.md", "a\r\nb\r\n", "a\r\nB\r\n");
        assert!(diff.contains("-b\r\n") && diff.contains("+B\r\n"), "{diff:?}");
    }

    #[test]
    fn no_parser_edit_contains_or_covers_a_line_ending() {
        // The invariant `plan` enforces, checked over every fixture the parser has edits for.
        for text in [
            DASHES,
            "# Ada\n\nLondon\n\n## E\n\n### Acme | Eng | 2015 - Now\n\n- **Stack**: Rust 3  \n  x\n",
        ] {
            for text in [text.to_string(), text.replace('\n', "\r\n")] {
                let parsed = parse(FILE, &text);
                for edit in edits_of(&parsed) {
                    assert!(!edit.replacement.contains(['\r', '\n']), "{edit:?}");
                    assert!(!text[edit.range.clone()].contains(['\r', '\n']), "{edit:?}");
                }
            }
        }
    }
}
