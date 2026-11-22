//! ABOUTME: Turns a resolved spec into the `Selection` the tailoring function takes, and checks its picks.
//! ABOUTME: The one place that knows both the spec's choices and `select`; everything else here stands alone.

use super::error::{Located, PickProblem, PickProblemKind, ScopeConflictSnafu, SpecError, UnknownPicksSnafu};
use super::merge::{ChoiceKey, SelectChoices};
use super::resolved::ResolvedSpec;
use crate::select::{Scope, SelectedTags, Selection, UnknownPick};

impl ResolvedSpec {
    /// The selection this spec asks for. `drafts` is off: whether a build previews drafts is the command line's
    /// choice, not the file's, so a sent resume can never contain text still marked draft because of a file.
    ///
    /// Tags and `core-only` are two ways to say what to keep, and a selection has one scope, so asking for both
    /// is an error rather than a guess.
    ///
    /// # Errors
    /// [`SpecError::ScopeConflict`] when `core-only` is true and tags are chosen too.
    pub fn to_selection(&self) -> Result<Selection, SpecError> {
        let select = &self.choices.select;
        let scope = match (select.core_only, SelectedTags::new(select.tags.clone())) {
            (Some(true), Some(_)) => return self.scope_conflict(),
            (Some(true), None) => Scope::CoreOnly,
            (_, Some(tags)) => Scope::Tags(tags),
            (_, None) => Scope::All,
        };
        Ok(Selection {
            scope,
            drafts: false,
            prefer: select.prefer.clone(),
            picks: select.pick.clone(),
        })
    }

    /// The conflict between `core-only` and the tags, located at the most specific file that chose tags: the one
    /// the user must change, since a less specific file's tags are only overridden by it.
    fn scope_conflict(&self) -> Result<Selection, SpecError> {
        self.scope_conflict_of(&self.choices.select, |key| self.origin_of(key))
    }

    /// [`Self::scope_conflict`] for any scope's choices, looked up in that scope's own origins.
    pub(super) fn scope_conflict_of<'a>(
        &self,
        select: &SelectChoices,
        origin_of: impl Fn(&ChoiceKey) -> Option<&'a Located>,
    ) -> Result<Selection, SpecError> {
        let position = |at: &Located| self.chain.iter().position(|link| *link.path == *at.path);
        let tags_at = select
            .tags
            .iter()
            .filter_map(|tag| origin_of(&ChoiceKey::Tag(tag.clone())))
            .max_by(|a, b| {
                position(a)
                    .cmp(&position(b))
                    .then_with(|| (b.line, b.column).cmp(&(a.line, a.column)))
            });
        match (tags_at, origin_of(&ChoiceKey::CoreOnly)) {
            (Some(tags_at), Some(core_only_at)) => ScopeConflictSnafu {
                tags_at: tags_at.clone(),
                core_only_at: core_only_at.clone(),
            }
            .fail(),
            _ => unreachable!("a merged value always has an origin"),
        }
    }

    /// Fails when a pick names a group or a wording the content does not have, naming the file and line that wrote it.
    ///
    /// `unknown` is the `unknown_picks` of the report that `select` returned. A pick that names nothing is a
    /// mistake in a profile or a spec, and a build that quietly ignored it would not be the build the file describes.
    ///
    /// # Errors
    /// [`SpecError::UnknownPicks`], listing every such pick.
    pub fn check_picks(&self, unknown: &[UnknownPick]) -> Result<(), SpecError> {
        self.check_picks_with(unknown, |key| self.origin_of(key))
    }

    /// [`Self::check_picks`] for any scope's picks, with the lines that scope's file wrote.
    pub(super) fn check_picks_with<'a>(
        &self,
        unknown: &[UnknownPick],
        origin_of: impl Fn(&ChoiceKey) -> Option<&'a Located>,
    ) -> Result<(), SpecError> {
        let problems: Vec<PickProblem> = unknown
            .iter()
            .filter_map(|pick| {
                let (group, kind) = match pick {
                    UnknownPick::Group(group) => (
                        group,
                        PickProblemKind::UnknownGroup {
                            group: group.to_string(),
                        },
                    ),
                    UnknownPick::Variant(alternate) => (
                        &alternate.group,
                        PickProblemKind::UnknownVariant {
                            group: alternate.group.to_string(),
                            variant: alternate.variant.to_string(),
                        },
                    ),
                };
                let at = origin_of(&ChoiceKey::Pick(group.clone()))?;
                Some(PickProblem { at: at.clone(), kind })
            })
            .collect();
        if problems.is_empty() {
            Ok(())
        } else {
            UnknownPicksSnafu { problems }.fail()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::parse;
    use crate::select::select;
    use crate::spec::resolve;
    use claims::{assert_err, assert_ok, assert_some};
    use pretty_assertions::assert_eq;
    use std::path::{Path, PathBuf};

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/specs")
    }

    /// A spec in a temporary directory, extending nothing.
    fn spec_text(body: &str) -> (tempfile::TempDir, ResolvedSpec) {
        let dir = assert_ok!(tempfile::tempdir());
        let path = dir.path().join("acme.toml");
        assert_ok!(std::fs::write(&path, format!("[build]\ncontent = \"a.md\"\n{body}")));
        let spec = assert_ok!(resolve(&path, None));
        (dir, spec)
    }

    #[test]
    fn a_spec_that_chooses_nothing_selects_the_default() {
        let (_dir, spec) = spec_text("");
        assert_eq!(assert_ok!(spec.to_selection()), Selection::default());
    }

    #[test]
    fn tags_prefer_and_picks_become_a_tag_scope_with_the_same_wordings() {
        let spec = assert_ok!(resolve(&fixtures().join("two-profiles/positions/acme.toml"), None));
        let selection = assert_ok!(spec.to_selection());
        let Scope::Tags(tags) = &selection.scope else {
            panic!("{:?}", selection.scope);
        };
        assert_eq!(
            tags.as_set().iter().map(AsRef::as_ref).collect::<Vec<_>>(),
            ["ai", "iceberg", "leadership", "platform"]
        );
        assert!(!selection.drafts, "a file never turns drafts on");
        assert_eq!(
            selection.prefer.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
            ["short", "business", "technical"]
        );
        assert_eq!(selection.picks.len(), 2);
    }

    #[test]
    fn core_only_is_the_core_scope_and_false_changes_nothing() {
        let (_dir, spec) = spec_text("[select]\ncore-only = true\n");
        assert_eq!(assert_ok!(spec.to_selection()).scope, Scope::CoreOnly);
        let (_dir, spec) = spec_text("[select]\ncore-only = false\ntags = [\"ai\"]\n");
        assert!(matches!(assert_ok!(spec.to_selection()).scope, Scope::Tags(_)));
        let (_dir, spec) = spec_text("[select]\ncore-only = false\n");
        assert_eq!(assert_ok!(spec.to_selection()).scope, Scope::All);
    }

    #[test]
    fn core_only_with_tags_is_a_conflict_naming_both_lines() {
        let (_dir, spec) = spec_text("[select]\ncore-only = true\ntags = [\"ai\"]\n");
        match assert_err!(spec.to_selection()) {
            SpecError::ScopeConflict { tags_at, core_only_at } => {
                assert_eq!((core_only_at.line, tags_at.line), (4, 5));
            }
            other => panic!("{other:?}"),
        }
    }

    fn selected_report(spec_body: &str) -> (ResolvedSpec, crate::select::SelectionReport) {
        let (dir, spec) = spec_text(spec_body);
        drop(dir);
        let baseline = assert_ok!(std::fs::read_to_string(fixtures().join("baseline.md")));
        let parsed = parse("baseline.md", &baseline);
        let resume = assert_some!(parsed.valid());
        let selection = assert_ok!(spec.to_selection());
        let (_, report) = assert_ok!(select(resume, &selection));
        (spec, report)
    }

    #[test]
    fn picks_that_the_content_has_are_accepted_and_applied() {
        let (spec, report) = selected_report("[select.pick]\ngateway = \"short\"\nsummary = \"ai\"\n");
        assert_ok!(spec.check_picks(&report.unknown_picks));
        let chosen: Vec<(String, String)> = report
            .chosen
            .iter()
            .map(|c| (c.group.to_string(), c.variant.to_string()))
            .collect();
        assert!(chosen.contains(&("gateway".into(), "short".into())), "{chosen:?}");
        assert!(chosen.contains(&("summary".into(), "ai".into())), "{chosen:?}");
    }

    #[test]
    fn picks_that_name_nothing_are_listed_with_the_line_that_wrote_them() {
        let (spec, report) = selected_report("[select.pick]\nnope = \"short\"\ngateway = \"huge\"\nsummary = \"ai\"\n");
        match assert_err!(spec.check_picks(&report.unknown_picks)) {
            SpecError::UnknownPicks { problems } => {
                assert_eq!(problems.len(), 2);
                let text = problems
                    .iter()
                    .map(|p| format!("{}: {}", p.at.line, p.kind))
                    .collect::<Vec<_>>();
                assert!(
                    text.iter()
                        .any(|t| t.starts_with("4:") && t.contains("no group `nope`")),
                    "{text:?}"
                );
                assert!(
                    text.iter().any(|t| t.contains("`gateway` has no wording `huge`")),
                    "{text:?}"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_list_of_unknown_picks_reads_as_lines_that_name_the_file() {
        let (spec, report) = selected_report("[select.pick]\nnope = \"short\"\n");
        let text = assert_err!(spec.check_picks(&report.unknown_picks)).to_string();
        assert!(
            text.starts_with("1 pick names nothing in the content file:\n  "),
            "{text}"
        );
        assert!(text.contains("acme.toml:4:"), "{text}");
    }

    #[test]
    fn a_scope_conflict_names_the_first_tag_of_the_most_specific_file_that_chose_tags() {
        let (_dir, spec) = spec_text("[select]\ncore-only = true\ntags = [\n  \"ai\",\n  \"llm\",\n]\n");
        match assert_err!(spec.to_selection()) {
            SpecError::ScopeConflict { tags_at, core_only_at } => {
                assert_eq!((core_only_at.line, tags_at.line), (4, 6));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_scope_conflict_points_at_the_spec_when_a_profile_also_chose_tags() {
        let dir = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir_all(dir.path().join("positions")));
        assert_ok!(std::fs::create_dir_all(dir.path().join("profiles")));
        assert_ok!(std::fs::write(
            dir.path().join("profiles/early.toml"),
            "[select]\ntags = [\"ai\"]\n"
        ));
        let spec_path = dir.path().join("positions/acme.toml");
        assert_ok!(std::fs::write(
            &spec_path,
            "extends = [\"early\"]\n[build]\ncontent = \"a.md\"\n[select]\ncore-only = true\ntags = [\"llm\"]\n"
        ));
        let spec = assert_ok!(resolve(&spec_path, None));
        match assert_err!(spec.to_selection()) {
            SpecError::ScopeConflict { tags_at, .. } => assert!(tags_at.path.ends_with("acme.toml"), "{tags_at}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn every_unknown_pick_is_located_at_its_own_line() {
        let (spec, report) = selected_report("[select.pick]\nnope = \"short\"\ngateway = \"huge\"\n");
        match assert_err!(spec.check_picks(&report.unknown_picks)) {
            SpecError::UnknownPicks { problems } => {
                let mut lines: Vec<usize> = problems.iter().map(|p| p.at.line).collect();
                lines.sort_unstable();
                assert_eq!(lines, [4, 5]);
            }
            other => panic!("{other:?}"),
        }
    }
}
