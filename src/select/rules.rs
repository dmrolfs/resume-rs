//! ABOUTME: The document-agnostic selection rules: whether one item is kept, and which wording a group renders.
//! ABOUTME: Resumes and cover letters both call these two functions, so they keep and choose alike.

use std::collections::BTreeSet;

use crate::model::{Alternate, Annotations, GroupName, Priority, Status, Tag, VariantName};

use super::{Scope, Selection};

/// Why an item, or a section, role, or era, was left out of the tailored document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum DropReason {
    /// The item is `@draft` and drafts are off.
    Draft,
    /// A section, role, or era above it is `@draft` and drafts are off.
    UnderDraft,
    /// Core only, and the item is not `@core`.
    CoreOnly,
    /// Its own tags miss the selected tags.
    TagsNotSelected,
    /// It has no tags of its own, and the tags it inherits miss the selected tags.
    InheritedTagsOnly,
    /// It is `@extra` and none of its tags is selected.
    ExtraNotSelected,
    /// Every wording of its group is a draft.
    NoReadyVariant,
    /// A section, role, or era left with nothing under it.
    Empty,
}

/// What [`judge`] needs to know about one item.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    /// The item's own priority; for a resume blurb, its own or else its role's.
    pub priority: Option<Priority>,
    /// The item's own status.
    pub status: Option<Status>,
    pub own_tags: &'a BTreeSet<Tag>,
    /// The tags of every container above the item; empty for a letter paragraph.
    pub inherited: &'a BTreeSet<Tag>,
}

/// Whether an item is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Keep,
    Drop(DropReason),
}

/// Decides whether one item is kept, first rule that applies wins:
/// a draft is dropped unless drafts are on (draft wins over core); `@core` is kept; core only drops the rest;
/// with no tags selected everything else is kept; with tags, an item is kept when its tags (own and inherited)
/// meet them, an `@extra` item is dropped otherwise, and an untagged item is kept because untagged means general.
#[must_use]
pub fn judge(facts: &Facts<'_>, selection: &Selection) -> Verdict {
    use Verdict::{Drop, Keep};
    if facts.status == Some(Status::Draft) && !selection.drafts {
        return Drop(DropReason::Draft);
    }
    if facts.priority == Some(Priority::Core) {
        return Keep;
    }
    let tags = match &selection.scope {
        Scope::All => return Keep,
        Scope::CoreOnly => return Drop(DropReason::CoreOnly),
        Scope::Tags(tags) => tags,
    };
    let selected = |set: &BTreeSet<Tag>| !set.is_disjoint(tags.as_set());
    if selected(facts.own_tags) || selected(facts.inherited) {
        Keep
    } else if facts.priority == Some(Priority::Extra) {
        Drop(DropReason::ExtraNotSelected)
    } else if facts.own_tags.is_empty() && facts.inherited.is_empty() {
        Keep
    } else if facts.own_tags.is_empty() {
        Drop(DropReason::InheritedTagsOnly)
    } else {
        Drop(DropReason::TagsNotSelected)
    }
}

/// How a group's wording was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum ChosenBy {
    /// A per-group pick (from a position spec).
    Pick,
    /// The first name in the preference list that the group has, and that is ready.
    Preference,
    /// The `default` variant.
    Default,
    /// The first ready variant, because `default` is a draft and nothing else applied.
    FirstReady,
}

/// One variant of a group, in document order.
#[derive(Debug, Clone, Copy)]
pub struct Variant<'a> {
    pub name: &'a VariantName,
    pub status: Option<Status>,
}

/// Which variant a group renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution<'a> {
    Chosen {
        /// The index of the chosen variant in the slice given to [`resolve`].
        index: usize,
        by: ChosenBy,
        /// Preferred wordings passed over because they are drafts.
        skipped: Vec<&'a VariantName>,
    },
    /// Every variant is a draft and drafts are off.
    NoReady,
}

/// A pick that names a group's draft wording while drafts are off: the spec asked for text that is not ready.
pub type PickedDraft = Alternate;

/// What a group is judged by as one item: the priority its variants share, and the union of their own tags.
/// Status is left to [`resolve`], which skips draft wordings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupFacts {
    pub priority: Option<Priority>,
    pub tags: BTreeSet<Tag>,
}

impl GroupFacts {
    /// The facts of a group with these variants' annotations, in any order.
    #[must_use]
    pub fn of<'a>(variants: impl IntoIterator<Item = &'a Annotations>) -> Self {
        variants.into_iter().fold(Self::default(), |mut facts, annotations| {
            // The parser makes every variant state the same priority, or none.
            facts.priority = facts.priority.or(annotations.priority);
            facts.tags.extend(annotations.tags.iter().cloned());
            facts
        })
    }
}

/// Decides whether a group is kept, as one item: `fallback` is the priority it takes when its variants state
/// none (a resume blurb takes its role's). A kept group renders the wording [`resolve`] chose, and that wording
/// should carry `facts.tags`, so selecting the result again judges the group the same way.
#[must_use]
pub fn judge_group(
    facts: &GroupFacts,
    inherited: &BTreeSet<Tag>,
    fallback: Option<Priority>,
    selection: &Selection,
) -> Verdict {
    let item = Facts {
        priority: facts.priority.or(fallback),
        status: None,
        own_tags: &facts.tags,
        inherited,
    };
    judge(&item, selection)
}

/// Chooses the one wording `group` renders: its pick, else the first preferred name it has that is ready,
/// else `default`, else its first ready variant. A pick of a draft is an error unless drafts are on; a pick
/// that names no variant of the group is ignored here (the caller reports it).
///
/// # Errors
/// [`PickedDraft`] when the group's pick names a draft variant and drafts are off.
pub fn resolve<'a>(
    group: &GroupName,
    variants: &[Variant<'a>],
    selection: &Selection,
) -> Result<Resolution<'a>, PickedDraft> {
    let ready = |v: &Variant<'_>| selection.drafts || v.status != Some(Status::Draft);
    let find = |name: &VariantName| variants.iter().position(|v| v.name == name);
    if let Some(index) = selection.picks.get(group).and_then(find) {
        return if ready(&variants[index]) {
            Ok(chosen(index, ChosenBy::Pick, Vec::new()))
        } else {
            Err(Alternate {
                group: group.clone(),
                variant: variants[index].name.clone(),
            })
        };
    }
    let mut skipped = Vec::new();
    for index in selection.prefer.iter().filter_map(find) {
        if ready(&variants[index]) {
            return Ok(chosen(index, ChosenBy::Preference, skipped));
        }
        skipped.push(variants[index].name);
    }
    if let Some(index) = variants.iter().position(|v| v.name.is_default() && ready(v)) {
        return Ok(chosen(index, ChosenBy::Default, skipped));
    }
    Ok(match variants.iter().position(ready) {
        Some(index) => chosen(index, ChosenBy::FirstReady, skipped),
        None => Resolution::NoReady,
    })
}

fn chosen<'a>(index: usize, by: ChosenBy, skipped: Vec<&'a VariantName>) -> Resolution<'a> {
    Resolution::Chosen { index, by, skipped }
}

// The letters' contract (rsm-vqr.2) cites these tests by name, so keep them stable:
// `drafts_off_preference_skips_draft_variant_and_falls_back`, `drafts_off_pick_of_draft_variant_is_an_error_naming_it`,
// `drafts_off_draft_default_falls_back_to_first_ready_variant`, `drafts_off_group_with_no_ready_variant_has_nothing_to_render`,
// `drafts_on_every_draft_variant_can_render`, and `a_group_is_judged_as_one_item_by_its_shared_priority_and_tag_union`.
// A `@draft` container dropping its subtree is resume-only (letters have no containers): see `select::tests`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::select::SelectedTags;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    fn tags(names: &[&str]) -> BTreeSet<Tag> {
        names.iter().map(|n| assert_ok!(Tag::try_new(*n))).collect()
    }

    fn with_tags(names: &[&str]) -> Selection {
        Selection {
            scope: Scope::Tags(claims::assert_some!(SelectedTags::new(tags(names)))),
            ..Selection::default()
        }
    }

    fn name(n: &str) -> VariantName {
        assert_ok!(VariantName::try_new(n))
    }

    fn group() -> GroupName {
        assert_ok!(GroupName::try_new("g"))
    }

    // ---- judge ------------------------------------------------------------------------------------------

    /// The rule `judge` must follow, written out independently, for the exhaustive table.
    fn expected(facts: &Facts<'_>, selection: &Selection) -> Verdict {
        use DropReason::*;
        let draft = facts.status == Some(Status::Draft);
        let core = facts.priority == Some(Priority::Core);
        let extra = facts.priority == Some(Priority::Extra);
        match &selection.scope {
            _ if draft && !selection.drafts => Verdict::Drop(Draft),
            _ if core => Verdict::Keep,
            Scope::CoreOnly => Verdict::Drop(CoreOnly),
            Scope::All => Verdict::Keep,
            Scope::Tags(t) => {
                let hit = facts
                    .own_tags
                    .iter()
                    .chain(facts.inherited)
                    .any(|tag| t.as_set().contains(tag));
                match () {
                    () if hit => Verdict::Keep,
                    () if extra => Verdict::Drop(ExtraNotSelected),
                    () if facts.own_tags.is_empty() && facts.inherited.is_empty() => Verdict::Keep,
                    () if facts.own_tags.is_empty() => Verdict::Drop(InheritedTagsOnly),
                    () => Verdict::Drop(TagsNotSelected),
                }
            }
        }
    }

    #[test]
    fn judge_agrees_with_the_rules_for_every_combination() {
        let pools = [tags(&[]), tags(&["a"]), tags(&["b"]), tags(&["a", "b"])];
        let priorities = std::iter::once(None)
            .chain(Priority::iter().map(Some))
            .collect::<Vec<_>>();
        let statuses = std::iter::once(None)
            .chain(Status::iter().map(Some))
            .collect::<Vec<_>>();
        let scopes = [
            Selection::default(),
            with_tags(&["a"]),
            Selection {
                scope: Scope::CoreOnly,
                ..Selection::default()
            },
        ];
        let mut checked = 0;
        for priority in &priorities {
            for status in &statuses {
                for own in &pools {
                    for inherited in &pools {
                        for base in &scopes {
                            for drafts in [false, true] {
                                let selection = Selection { drafts, ..base.clone() };
                                let facts = Facts {
                                    priority: *priority,
                                    status: *status,
                                    own_tags: own,
                                    inherited,
                                };
                                assert_eq!(
                                    judge(&facts, &selection),
                                    expected(&facts, &selection),
                                    "{facts:?} {selection:?}"
                                );
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 3 * 3 * 4 * 4 * 3 * 2);
    }

    fn facts<'a>(
        priority: Option<Priority>,
        status: Option<Status>,
        own: &'a BTreeSet<Tag>,
        inherited: &'a BTreeSet<Tag>,
    ) -> Facts<'a> {
        Facts {
            priority,
            status,
            own_tags: own,
            inherited,
        }
    }

    #[test]
    fn judge_draft_wins_over_core() {
        let none = tags(&[]);
        let core_draft = facts(Some(Priority::Core), Some(Status::Draft), &none, &none);
        assert_eq!(
            judge(&core_draft, &Selection::default()),
            Verdict::Drop(DropReason::Draft)
        );
        let drafts_on = Selection {
            drafts: true,
            ..Selection::default()
        };
        assert_eq!(judge(&core_draft, &drafts_on), Verdict::Keep);
    }

    #[test]
    fn judge_keeps_untagged_items_under_tags_as_general() {
        let none = tags(&[]);
        assert_eq!(
            judge(&facts(None, None, &none, &none), &with_tags(&["a"])),
            Verdict::Keep
        );
    }

    #[test]
    fn judge_names_a_drop_caused_only_by_inherited_tags() {
        let (none, b) = (tags(&[]), tags(&["b"]));
        assert_eq!(
            judge(&facts(None, None, &none, &b), &with_tags(&["a"])),
            Verdict::Drop(DropReason::InheritedTagsOnly)
        );
        assert_eq!(
            judge(&facts(None, None, &b, &none), &with_tags(&["a"])),
            Verdict::Drop(DropReason::TagsNotSelected)
        );
    }

    #[test]
    fn judge_extra_needs_a_selected_tag_and_is_kept_without_tags() {
        let (none, a) = (tags(&[]), tags(&["a"]));
        let extra = |own| facts(Some(Priority::Extra), None, own, &none);
        assert_eq!(
            judge(&extra(&none), &with_tags(&["a"])),
            Verdict::Drop(DropReason::ExtraNotSelected)
        );
        assert_eq!(judge(&extra(&a), &with_tags(&["a"])), Verdict::Keep);
        assert_eq!(judge(&extra(&none), &Selection::default()), Verdict::Keep);
    }

    // ---- resolve: the selected-draft contract ------------------------------------------------------------

    fn variants<'a>(names: &'a [VariantName], drafts: &[&str]) -> Vec<Variant<'a>> {
        names
            .iter()
            .map(|n| Variant {
                name: n,
                status: drafts.contains(&n.as_ref()).then_some(Status::Draft),
            })
            .collect()
    }

    fn names(list: &[&str]) -> Vec<VariantName> {
        list.iter().map(|n| name(n)).collect()
    }

    fn picking(variant: &str) -> Selection {
        Selection {
            picks: [(group(), name(variant))].into(),
            ..Selection::default()
        }
    }

    fn preferring(list: &[&str]) -> Selection {
        Selection {
            prefer: names(list),
            ..Selection::default()
        }
    }

    fn chosen_index(resolution: Resolution<'_>) -> (usize, ChosenBy) {
        match resolution {
            Resolution::Chosen { index, by, .. } => (index, by),
            Resolution::NoReady => panic!("expected a chosen variant"),
        }
    }

    #[test]
    fn a_pick_beats_the_preferences_and_default() {
        let n = names(&["default", "short", "business"]);
        let selection = Selection {
            prefer: names(&["business"]),
            ..picking("short")
        };
        assert_eq!(
            chosen_index(assert_ok!(resolve(&group(), &variants(&n, &[]), &selection))),
            (1, ChosenBy::Pick)
        );
    }

    #[test]
    fn the_first_preferred_name_the_group_has_wins() {
        let n = names(&["default", "short"]);
        let resolved = assert_ok!(resolve(
            &group(),
            &variants(&n, &[]),
            &preferring(&["business", "short"])
        ));
        assert_eq!(chosen_index(resolved), (1, ChosenBy::Preference));
        let both = names(&["default", "short", "business"]);
        let resolved = assert_ok!(resolve(
            &group(),
            &variants(&both, &[]),
            &preferring(&["business", "short"])
        ));
        assert_eq!(chosen_index(resolved), (2, ChosenBy::Preference));
    }

    #[test]
    fn with_no_pick_and_no_preference_the_default_renders() {
        let n = names(&["short", "default"]);
        assert_eq!(
            chosen_index(assert_ok!(resolve(&group(), &variants(&n, &[]), &Selection::default()))),
            (1, ChosenBy::Default)
        );
    }

    #[test]
    fn a_pick_that_names_no_variant_falls_through_to_the_preferences() {
        let n = names(&["default", "short"]);
        let selection = Selection {
            prefer: names(&["short"]),
            ..picking("long")
        };
        assert_eq!(
            chosen_index(assert_ok!(resolve(&group(), &variants(&n, &[]), &selection))),
            (1, ChosenBy::Preference)
        );
    }

    #[test]
    fn drafts_off_preference_skips_draft_variant_and_falls_back() {
        let n = names(&["default", "short", "business"]);
        let resolved = assert_ok!(resolve(
            &group(),
            &variants(&n, &["business"]),
            &preferring(&["business", "short"])
        ));
        let Resolution::Chosen { index, by, skipped } = resolved else {
            panic!()
        };
        assert_eq!((index, by), (1, ChosenBy::Preference));
        assert_eq!(skipped, vec![&n[2]], "the skipped draft wording is reported");
    }

    #[test]
    fn drafts_off_pick_of_draft_variant_is_an_error_naming_it() {
        let n = names(&["default", "short"]);
        let error = assert_err!(resolve(&group(), &variants(&n, &["short"]), &picking("short")));
        assert_eq!(
            error,
            Alternate {
                group: group(),
                variant: name("short")
            }
        );
    }

    #[test]
    fn drafts_off_draft_default_falls_back_to_first_ready_variant() {
        let n = names(&["default", "short", "long"]);
        let resolved = assert_ok!(resolve(&group(), &variants(&n, &["default"]), &Selection::default()));
        assert_eq!(chosen_index(resolved), (1, ChosenBy::FirstReady));
    }

    #[test]
    fn drafts_off_group_with_no_ready_variant_has_nothing_to_render() {
        let n = names(&["default", "short"]);
        let resolved = assert_ok!(resolve(
            &group(),
            &variants(&n, &["default", "short"]),
            &preferring(&["short"])
        ));
        assert_eq!(resolved, Resolution::NoReady);
    }

    #[test]
    fn drafts_on_every_draft_variant_can_render() {
        let n = names(&["default", "short"]);
        let on = |s: Selection| Selection { drafts: true, ..s };
        let v = variants(&n, &["default", "short"]);
        assert_eq!(
            chosen_index(assert_ok!(resolve(&group(), &v, &on(picking("short"))))),
            (1, ChosenBy::Pick)
        );
        assert_eq!(
            chosen_index(assert_ok!(resolve(&group(), &v, &on(preferring(&["short"]))))),
            (1, ChosenBy::Preference)
        );
        assert_eq!(
            chosen_index(assert_ok!(resolve(&group(), &v, &on(Selection::default())))),
            (0, ChosenBy::Default)
        );
    }

    #[test]
    fn a_group_is_judged_as_one_item_by_its_shared_priority_and_tag_union() {
        let note = |priority, tag: &str, status| Annotations {
            priority,
            status,
            tags: tags(&[tag]),
            alternate: None,
            id: None,
        };
        let variants = [note(None, "platform", None), note(None, "ai", Some(Status::Draft))];
        let facts = GroupFacts::of(&variants);
        assert_eq!(facts.tags, tags(&["ai", "platform"]));
        let none = tags(&[]);
        assert_eq!(
            judge_group(&facts, &none, None, &with_tags(&["ai"])),
            Verdict::Keep,
            "the union meets the tags"
        );
        assert_eq!(
            judge_group(&facts, &none, None, &with_tags(&["data"])),
            Verdict::Drop(DropReason::TagsNotSelected)
        );
        assert_eq!(
            judge_group(&facts, &none, Some(Priority::Core), &with_tags(&["data"])),
            Verdict::Keep,
            "the fallback priority applies when no variant states one"
        );
        let core = [
            note(Some(Priority::Core), "x", None),
            note(Some(Priority::Core), "y", None),
        ];
        assert_eq!(GroupFacts::of(&core).priority, Some(Priority::Core));
    }
}
