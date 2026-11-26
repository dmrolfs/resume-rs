//! ABOUTME: Property tests of selection over generated annotated resumes: what survives, what renders once,
//! ABOUTME: what keeps its order (unless promoted), how `promote` orders lists, and that no draft leaks out.

use std::collections::BTreeSet;

use claims::{assert_ok, assert_some};
use proptest::prelude::*;
use resume::markdown::parse;
use resume::model::{
    Annotations, Block, Bullet, GroupName, Inline, Priority, Resume, RoleBlock, Status, Tag, VariantName,
};
use resume::select::rules::{self, Facts, Verdict};
use resume::select::{Scope, SelectError, SelectedTags, Selection, SelectionReport, select};
use std::collections::BTreeMap;

fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(256)
}

// ---- generating annotated resumes -----------------------------------------------------------------------

/// An annotation for one line: priority, status, and tags from small pools.
#[derive(Debug, Clone)]
struct Note {
    priority: Option<&'static str>,
    status: Option<&'static str>,
    tags: Vec<&'static str>,
}

fn note() -> impl Strategy<Value = Note> {
    (
        prop::option::weighted(0.3, prop::sample::select(vec!["core", "extra"])),
        prop::option::weighted(0.2, prop::sample::select(vec!["draft", "verify"])),
        prop::sample::subsequence(vec!["a", "b", "c"], 0..=2),
    )
        .prop_map(|(priority, status, tags)| Note { priority, status, tags })
}

impl Note {
    fn tokens(&self, alternate: Option<String>) -> String {
        let mut words: Vec<String> = alternate.into_iter().collect();
        words.extend(self.priority.map(|p| format!("@{p}")));
        words.extend(self.status.map(|s| format!("@{s}")));
        words.extend(self.tags.iter().map(|t| format!("#{t}")));
        if words.is_empty() {
            String::new()
        } else {
            format!(" <!-- {} -->", words.join(" "))
        }
    }
}

/// A run of leaves: a plain leaf, or a group of 2 to 3 wordings sharing one priority, one `default`.
#[derive(Debug, Clone)]
enum Run {
    Plain(Note),
    Group {
        priority: Option<&'static str>,
        variants: Vec<Note>,
    },
}

fn run() -> impl Strategy<Value = Run> {
    prop_oneof![
        3 => note().prop_map(Run::Plain),
        1 => (
            prop::option::weighted(0.3, prop::sample::select(vec!["core", "extra"])),
            prop::collection::vec(note(), 2..=3)
        )
            .prop_map(|(priority, variants)| Run::Group { priority, variants }),
    ]
}

/// A container's body: bullets in one list.
fn bullets() -> impl Strategy<Value = Vec<Run>> {
    prop::collection::vec(run(), 0..4)
}

#[derive(Debug, Clone)]
struct RoleSpec {
    note: Note,
    /// A plain blurb, or a group whose first wording is the blurb and whose others are the paragraphs after it.
    blurb: Option<Run>,
    bullets: Vec<Run>,
    /// Paragraphs after the bullets: body prose, never a blurb wording; two make a run `promote` can order.
    closing: Vec<Note>,
    eras: Vec<(Note, Vec<Run>)>,
}

fn role() -> impl Strategy<Value = RoleSpec> {
    (
        note(),
        prop::option::of(run()),
        bullets(),
        prop::collection::vec(note(), 0..=2),
        prop::collection::vec((note(), bullets()), 0..=2),
    )
        .prop_map(|(note, blurb, bullets, closing, eras)| RoleSpec {
            note,
            blurb,
            bullets,
            closing,
            eras,
        })
}

#[derive(Debug, Clone)]
struct SectionSpec {
    note: Note,
    /// Paragraphs before the bullets: plain, or a group of paragraph wordings.
    prose: Vec<Run>,
    bullets: Vec<Run>,
    roles: Vec<RoleSpec>,
    /// A page break at the end of the section.
    page_break: bool,
}

fn section() -> impl Strategy<Value = SectionSpec> {
    (
        note(),
        prop::collection::vec(run(), 0..=2),
        bullets(),
        prop::collection::vec(role(), 0..=2),
        prop::bool::weighted(0.2),
    )
        .prop_map(|(note, prose, bullets, roles, page_break)| SectionSpec {
            note,
            prose,
            bullets,
            roles,
            page_break,
        })
}

/// Writes the generated resume as Markdown; every leaf's text is a unique id (`n1`, `n2`, ...).
struct Writer {
    out: String,
    next_leaf: usize,
    next_group: usize,
}

const VARIANTS: [&str; 3] = ["default", "short", "long"];

impl Writer {
    fn leaf_id(&mut self) -> String {
        self.next_leaf += 1;
        format!("n{}", self.next_leaf)
    }

    /// Writes runs as bullets (one line each) or as paragraphs (a blank line after each).
    fn runs(&mut self, runs: &[Run], bullets: bool) {
        if runs.is_empty() {
            return;
        }
        let (marker, end) = if bullets { ("- ", "\n") } else { ("", "\n\n") };
        for run in runs {
            match run {
                Run::Plain(note) => {
                    let id = self.leaf_id();
                    self.out.push_str(&format!("{marker}{id}{}{end}", note.tokens(None)));
                }
                Run::Group { priority, variants } => {
                    self.next_group += 1;
                    let group = self.next_group;
                    for (index, note) in variants.iter().enumerate() {
                        let id = self.leaf_id();
                        // The parser requires one priority per group, so the group's priority replaces each one's.
                        let note = Note {
                            priority: *priority,
                            ..note.clone()
                        };
                        let alternate = format!("=g{group}.{}", VARIANTS[index]);
                        self.out
                            .push_str(&format!("{marker}{id}{}{end}", note.tokens(Some(alternate))));
                    }
                }
            }
        }
        if bullets {
            self.out.push('\n');
        }
    }

    fn resume(sections: &[SectionSpec]) -> String {
        let mut writer = Writer {
            out: "# Ada\n\nLondon\n\n".into(),
            next_leaf: 0,
            next_group: 0,
        };
        for (index, section) in sections.iter().enumerate() {
            writer
                .out
                .push_str(&format!("## S{index}{}\n\n", section.note.tokens(None)));
            writer.runs(&section.prose, false);
            writer.runs(&section.bullets, true);
            for (r, role) in section.roles.iter().enumerate() {
                let line = format!(
                    "### Co{index}x{r} · Engineer · 2015 – 2017{}\n\n",
                    role.note.tokens(None)
                );
                writer.out.push_str(&line);
                writer.runs(role.blurb.as_slice(), false);
                writer.runs(&role.bullets, true);
                for note in &role.closing {
                    let id = writer.leaf_id();
                    writer.out.push_str(&format!("{id}{}\n\n", note.tokens(None)));
                }
                for (e, (note, items)) in role.eras.iter().enumerate() {
                    writer
                        .out
                        .push_str(&format!("**Era{e} · 2016 – 2017**{}\n\n", note.tokens(None)));
                    writer.runs(items, true);
                }
            }
            if section.page_break {
                writer.out.push_str("<div class=\"page-break\"></div>\n\n");
            }
        }
        writer.out
    }
}

fn resume_strategy() -> impl Strategy<Value = Resume> {
    prop::collection::vec(section(), 1..=3).prop_map(|sections| {
        let markdown = Writer::resume(&sections);
        let parsed = parse("r.md", &markdown);
        assert_some!(
            parsed.valid(),
            "generated an invalid resume:\n{markdown}\n{:?}",
            parsed.diagnostics()
        )
        .clone()
    })
}

proptest! {
    #[test]
    fn explicit_membership_is_exact_in_tree_and_report(input in resume_strategy(), requested_bits in prop::collection::vec(any::<bool>(), 0..40), drafts in any::<bool>()) {
        let known: BTreeSet<_> = input.nodes().iter().filter_map(|n| n.annotations.alternate.as_ref().map(|a| a.group.clone())).collect();
        let requested: BTreeSet<_> = known.into_iter().enumerate().filter(|(i, _)| requested_bits.get(*i).copied().unwrap_or(false)).map(|(_, g)| g).collect();
        let selection = Selection { groups: Some(requested.clone()), drafts, ..Selection::default() };
        match select(&input, &selection) {
            Ok((out, report)) => {
                let actual: Vec<_> = out.nodes().iter().filter_map(|n| n.annotations.alternate.as_ref().map(|a| a.group.clone())).collect();
                let reported: Vec<_> = report.chosen.iter().map(|c| c.group.clone()).collect();
                prop_assert_eq!(actual.len(), requested.len());
                prop_assert_eq!(reported.len(), requested.len());
                prop_assert_eq!(actual.into_iter().collect::<BTreeSet<_>>(), requested.clone());
                prop_assert_eq!(reported.into_iter().collect::<BTreeSet<_>>(), requested);
            }
            Err(SelectError::UnavailableGroups { groups }) => {
                prop_assert!(!drafts);
                prop_assert!(!groups.is_empty());
                prop_assert!(groups.iter().all(|g| requested.contains(&g.group)));
            }
            Err(error) => prop_assert!(false, "unexpected error: {error}"),
        }
    }
}

fn selection_strategy() -> impl Strategy<Value = Selection> {
    let scope = prop_oneof![
        Just(Scope::All),
        Just(Scope::CoreOnly),
        prop::sample::subsequence(vec!["a", "b", "c", "d"], 1..=2).prop_map(|names| {
            let set: BTreeSet<Tag> = names.into_iter().map(|n| assert_ok!(Tag::try_new(n))).collect();
            Scope::Tags(assert_some!(SelectedTags::new(set)))
        }),
    ];
    let variant = prop::sample::select(vec!["default", "short", "long", "zzz"]);
    (
        scope,
        any::<bool>(),
        prop::sample::subsequence(vec!["short", "long", "business"], 0..=2),
        prop::collection::btree_map(1..6usize, variant, 0..=2),
    )
        .prop_map(|(scope, drafts, prefer, picks)| Selection {
            groups: None,
            scope,
            drafts,
            prefer: prefer
                .into_iter()
                .map(|v| assert_ok!(VariantName::try_new(v)))
                .collect(),
            picks: picks
                .into_iter()
                .map(|(g, v)| {
                    (
                        assert_ok!(GroupName::try_new(format!("g{g}"))),
                        assert_ok!(VariantName::try_new(v)),
                    )
                })
                .collect(),
            // Promote-free, so "keeps order" below stays a true and meaningful check; ordering has its own properties.
            promote: Vec::new(),
        })
}

// ---- reading resumes ------------------------------------------------------------------------------------

/// One leaf in document order: its id, its own annotations, and the annotations of its containers.
#[derive(Debug, Clone)]
struct Leaf {
    id: String,
    annotations: Annotations,
    containers: Vec<Annotations>,
    /// For a blurb: its role's priority, which it takes when it has none.
    fallback: Option<Priority>,
}

fn id(content: &[Inline]) -> String {
    content
        .iter()
        .map(|i| match i {
            Inline::Text { text } => text.trim().to_string(),
            other => format!("{other:?}"),
        })
        .collect()
}

fn leaves(resume: &Resume) -> Vec<Leaf> {
    let mut out = Vec::new();
    let push = |out: &mut Vec<Leaf>, content: &[Inline], a: &Annotations, containers: &[&Annotations], fallback| {
        out.push(Leaf {
            id: id(content),
            annotations: a.clone(),
            containers: containers.iter().map(|c| (*c).clone()).collect(),
            fallback,
        });
    };
    let bullets = |out: &mut Vec<Leaf>, items: &[Bullet], containers: &[&Annotations]| {
        for b in items {
            push(out, &b.content, &b.annotations, containers, None);
        }
    };
    for section in &resume.sections {
        let s = [&section.annotations];
        for block in &section.blocks {
            match block {
                Block::Paragraph { content, annotations } => push(&mut out, content, annotations, &s, None),
                Block::Bullets { items } => bullets(&mut out, items, &s),
                Block::Role(role) => {
                    let r = [&section.annotations, &role.annotations];
                    if let Some(blurb) = &role.blurb {
                        push(&mut out, blurb, &role.blurb_annotations, &r, role.annotations.priority);
                    }
                    for part in &role.body {
                        match part {
                            RoleBlock::Paragraph { content, annotations } => {
                                push(&mut out, content, annotations, &r, None)
                            }
                            RoleBlock::Bullets { items } => bullets(&mut out, items, &r),
                            RoleBlock::Era { items, annotations, .. } => {
                                bullets(&mut out, items, &[&section.annotations, &role.annotations, annotations]);
                            }
                        }
                    }
                }
                Block::PageBreak => {}
            }
        }
    }
    out
}

fn ids(resume: &Resume) -> Vec<String> {
    leaves(resume).into_iter().map(|l| l.id).collect()
}

fn is_subsequence(small: &[String], big: &[String]) -> bool {
    let mut big = big.iter();
    small.iter().all(|x| big.any(|y| y == x))
}

fn draft(a: &Annotations) -> bool {
    a.status == Some(Status::Draft)
}

/// A leaf that `select` must keep: `@core` (own, or a blurb's role's), not a draft, under no draft container,
/// and, for a variant, in a group with a ready wording. The survivor of a group is checked by its group.
fn eligible_core(leaf: &Leaf, all: &[Leaf], drafts: bool) -> Option<Option<GroupName>> {
    let core = leaf.annotations.priority.or(leaf.fallback) == Some(Priority::Core);
    let under_draft = leaf.containers.iter().any(draft);
    if !core || (!drafts && under_draft) {
        return None;
    }
    match &leaf.annotations.alternate {
        None => (!draft(&leaf.annotations) || drafts).then_some(None),
        Some(alternate) => {
            let ready = all
                .iter()
                .filter(|l| {
                    l.annotations
                        .alternate
                        .as_ref()
                        .is_some_and(|a| a.group == alternate.group)
                })
                .any(|l| drafts || !draft(&l.annotations));
            ready.then(|| Some(alternate.group.clone()))
        }
    }
}

/// The sections, roles, and eras of a resume that hold nothing, by name.
fn empty_containers(resume: &Resume) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for section in &resume.sections {
        if section.blocks.iter().all(|b| matches!(b, Block::PageBreak)) {
            out.insert(format!("section {}", section.title));
        }
        for block in &section.blocks {
            let Block::Role(role) = block else { continue };
            if role.blurb.is_none() && role.body.is_empty() {
                out.insert(format!("role {}", role.company));
            }
            for part in &role.body {
                if let RoleBlock::Era { name, items, .. } = part
                    && items.is_empty()
                {
                    out.insert(format!("era {} {name}", role.company));
                }
            }
        }
    }
    out
}

fn no_empty_lists(resume: &Resume) -> bool {
    resume.sections.iter().all(|section| {
        section.blocks.iter().all(|block| match block {
            Block::Bullets { items } => !items.is_empty(),
            Block::Role(role) => role
                .body
                .iter()
                .all(|part| !matches!(part, RoleBlock::Bullets { items } if items.is_empty())),
            _ => true,
        })
    })
}

// ---- properties -----------------------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    #[test]
    fn selection_keeps_eligible_core_renders_one_wording_keeps_order_and_is_idempotent(
        resume in resume_strategy(),
        selection in selection_strategy(),
    ) {
        let Ok((tailored, report)) = select(&resume, &selection) else { return Ok(()) };
        let input = leaves(&resume);
        let output = leaves(&tailored);
        let output_ids = ids(&tailored);

        // Output leaves are input leaves, in the same order, with the same text (the strategy never promotes).
        prop_assert!(is_subsequence(&output_ids, &ids(&resume)), "{output_ids:?}");

        // Every eligible @core leaf survives; for a group, exactly one of its wordings does.
        for leaf in &input {
            match eligible_core(leaf, &input, selection.drafts) {
                None => {}
                Some(None) => prop_assert!(output_ids.contains(&leaf.id), "core leaf {} lost", leaf.id),
                Some(Some(group)) => {
                    let kept = output.iter().filter(|l| l.annotations.alternate.as_ref().is_some_and(|a| a.group == group)).count();
                    prop_assert_eq!(kept, 1, "core group {} renders {} wordings", group, kept);
                }
            }
        }

        // Every surviving group renders exactly one wording.
        let mut groups: Vec<&GroupName> = output.iter().filter_map(|l| l.annotations.alternate.as_ref().map(|a| &a.group)).collect();
        let total = groups.len();
        groups.dedup();
        prop_assert_eq!(total, groups.len(), "a group renders twice");
        prop_assert_eq!(report.chosen.len(), groups.len());

        // No bullet list is empty, and the only empty sections, roles, and eras are the ones the author wrote empty.
        prop_assert!(no_empty_lists(&tailored));
        prop_assert!(empty_containers(&tailored).is_subset(&empty_containers(&resume)), "selection left a heading alone");

        // Without drafts, no draft and nothing under a draft reaches the output.
        if !selection.drafts {
            for leaf in &output {
                prop_assert!(!draft(&leaf.annotations) && !leaf.containers.iter().any(draft), "draft {} leaked", leaf.id);
            }
        }

        // Selecting the tailored resume again changes nothing.
        let (again, second) = prop_assert_ok(select(&tailored, &selection))?;
        prop_assert_eq!(&again, &tailored);
        prop_assert!(second.dropped.is_empty(), "{:?}", second.dropped);
    }

    #[test]
    fn a_draft_pick_is_an_error_exactly_when_it_names_a_draft_wording_and_drafts_are_off(
        resume in resume_strategy(),
        selection in selection_strategy(),
    ) {
        let input = leaves(&resume);
        let picks_a_draft = !selection.drafts && selection.picks.iter().any(|(group, variant)| {
            input.iter().any(|l| {
                draft(&l.annotations) && l.annotations.alternate.as_ref().is_some_and(|a| &a.group == group && &a.variant == variant)
            })
        });
        match select(&resume, &selection) {
            Err(SelectError::PickedDraft { picks }) => {
                prop_assert!(picks_a_draft);
                prop_assert!(!picks.is_empty());
            }
            Ok(_) => prop_assert!(!picks_a_draft),
            Err(error) => prop_assert!(false, "unexpected error: {error}"),
        }
    }

    #[test]
    fn an_empty_selection_of_an_unannotated_resume_is_the_identity(sections in prop::collection::vec(section(), 1..=3)) {
        let plain: Vec<SectionSpec> = sections.into_iter().map(strip).collect();
        let markdown = Writer::resume(&plain);
        let resume = assert_some!(parse("r.md", &markdown).valid()).clone();
        let (tailored, report) = prop_assert_ok(select(&resume, &Selection::default()))?;
        prop_assert_eq!(&tailored, &resume);
        prop_assert_eq!(report, SelectionReport::default());
    }
}

fn prop_assert_ok<T, E: std::fmt::Debug>(result: Result<T, E>) -> Result<T, TestCaseError> {
    result.map_err(|e| TestCaseError::fail(format!("{e:?}")))
}

/// The section with every annotation removed (groups become plain bullets).
fn strip(section: SectionSpec) -> SectionSpec {
    let none = || Note {
        priority: None,
        status: None,
        tags: Vec::new(),
    };
    let runs = |runs: Vec<Run>| -> Vec<Run> {
        runs.into_iter()
            .flat_map(|run| match run {
                Run::Plain(_) => vec![Run::Plain(none())],
                Run::Group { variants, .. } => variants.into_iter().map(|_| Run::Plain(none())).collect(),
            })
            .collect()
    };
    SectionSpec {
        note: none(),
        prose: runs(section.prose),
        bullets: runs(section.bullets),
        roles: section
            .roles
            .into_iter()
            .map(|role| RoleSpec {
                note: none(),
                blurb: role.blurb.map(|_| Run::Plain(none())),
                bullets: runs(role.bullets),
                closing: role.closing.iter().map(|_| none()).collect(),
                eras: role.eras.into_iter().map(|(_, items)| (none(), runs(items))).collect(),
            })
            .collect(),
        page_break: section.page_break,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    /// The upper bound: every kept leaf that is not a group wording is one the keep rule keeps, with the tags it
    /// inherits; under core only, every kept leaf is core.
    #[test]
    fn selection_keeps_only_what_the_keep_rule_keeps(resume in resume_strategy(), selection in selection_strategy()) {
        let Ok((tailored, _)) = select(&resume, &selection) else { return Ok(()) };
        for leaf in leaves(&tailored) {
            let priority = leaf.annotations.priority.or(leaf.fallback);
            if selection.scope == Scope::CoreOnly {
                prop_assert_eq!(priority, Some(Priority::Core), "{} is not core", leaf.id);
            }
            if leaf.annotations.alternate.is_some() {
                continue;
            }
            let inherited: BTreeSet<Tag> = leaf.containers.iter().flat_map(|c| c.tags.iter().cloned()).collect();
            let facts = Facts { priority, status: leaf.annotations.status, own_tags: &leaf.annotations.tags, inherited: &inherited };
            prop_assert_eq!(rules::judge(&facts, &selection), Verdict::Keep, "{} was kept", leaf.id);
        }
    }

    /// The report accounts for every leaf left out: each is listed once, or is a wording of a group that rendered
    /// or was listed once; nothing listed is in the output; and the draft counters add up.
    #[test]
    fn the_report_accounts_for_every_leaf_left_out(resume in resume_strategy(), selection in selection_strategy()) {
        let Ok((tailored, report)) = select(&resume, &selection) else { return Ok(()) };
        let input = leaves(&resume);
        let kept: BTreeSet<String> = ids(&tailored).into_iter().collect();
        let listed: Vec<&str> = report.dropped.iter().map(|d| d.node.label.as_str()).collect();
        for label in &listed {
            prop_assert!(!kept.contains(*label), "{} is listed as dropped but kept", label);
        }
        for leaf in input.iter().filter(|l| !kept.contains(&l.id)) {
            let times = listed.iter().filter(|l| **l == leaf.id).count();
            match &leaf.annotations.alternate {
                None => prop_assert_eq!(times, 1, "{} left out and listed {} times", leaf.id, times),
                Some(alternate) => {
                    let wordings: Vec<&Leaf> = input
                        .iter()
                        .filter(|l| l.annotations.alternate.as_ref().is_some_and(|a| a.group == alternate.group))
                        .collect();
                    let rendered = wordings.iter().any(|w| kept.contains(&w.id));
                    let listed_times: usize = wordings.iter().map(|w| listed.iter().filter(|l| **l == w.id).count()).sum();
                    prop_assert!(rendered != (listed_times == 1), "group {} rendered={} listed {} times", alternate.group, rendered, listed_times);
                }
            }
        }
        // Every node a report names is the input's node at that index, of the same kind.
        let nodes = resume.nodes();
        for node in report.dropped.iter().map(|d| &d.node).chain(&report.verify) {
            let found = nodes.get(node.index);
            prop_assert!(found.is_some_and(|n| n.kind == node.kind), "{:?} is not node {}", node, node.index);
            // A leaf's text is a unique id, so the index names exactly the leaf the label came from.
            if let Some(content) = found.and_then(|n| n.content) {
                let text = resume::model::plain_text(content);
                prop_assert_eq!(text.trim(), node.label.as_str());
            }
        }
        // Every leaf and container marked @draft is counted once; with drafts off, not what a draft container
        // holds (it is dropped with the container, which is counted).
        let shut = |l: &Leaf| !selection.drafts && l.containers.iter().any(draft);
        let drafts = input.iter().filter(|l| draft(&l.annotations) && !shut(l)).count()
            + containers_marked_draft(&resume, selection.drafts);
        prop_assert_eq!(report.drafts_left_out + report.drafts_included, drafts);
        if selection.drafts {
            prop_assert_eq!(report.drafts_left_out, 0);
        } else {
            prop_assert_eq!(report.drafts_included, 0);
        }
    }
}

/// Sections, roles, and eras marked `@draft`; with drafts off, not those under another draft container.
fn containers_marked_draft(resume: &Resume, drafts: bool) -> usize {
    let mut count = 0;
    for section in &resume.sections {
        if draft(&section.annotations) {
            count += 1;
            if !drafts {
                continue;
            }
        }
        for block in &section.blocks {
            let Block::Role(role) = block else { continue };
            if draft(&role.annotations) {
                count += 1;
                if !drafts {
                    continue;
                }
            }
            count += role
                .body
                .iter()
                .filter(|p| matches!(p, RoleBlock::Era { annotations, .. } if draft(annotations)))
                .count();
        }
    }
    count
}

// ---- promote ----------------------------------------------------------------------------------------------

/// A promote list: some of the generated tags, plus `d`, which nothing carries, in any order.
fn promote_strategy() -> impl Strategy<Value = Vec<Tag>> {
    prop::sample::subsequence(vec!["a", "b", "c", "d"], 0..=4)
        .prop_shuffle()
        .prop_map(|names| names.into_iter().map(|n| assert_ok!(Tag::try_new(n))).collect())
}

/// One orderable list of a tailored resume: its kind, and its leaves with their containers' annotations.
#[derive(Debug)]
struct List {
    kind: &'static str,
    leaves: Vec<Leaf>,
}

/// The resume as a skeleton (headings, blurbs, page breaks, and each list as its sorted ids) and its lists.
/// Two resumes with equal skeletons differ at most in the order inside their lists.
fn shape(resume: &Resume) -> (Vec<String>, Vec<List>) {
    let mut skeleton = Vec::new();
    let mut lists: Vec<List> = Vec::new();
    let leaf = |content: &[Inline], a: &Annotations, containers: &[&Annotations]| Leaf {
        id: id(content),
        annotations: a.clone(),
        containers: containers.iter().map(|c| (*c).clone()).collect(),
        fallback: None,
    };
    let mut close = |skeleton: &mut Vec<String>, list: List| {
        let mut ids: Vec<&str> = list.leaves.iter().map(|l| l.id.as_str()).collect();
        ids.sort_unstable();
        skeleton.push(format!("{} {}", list.kind, ids.join(" ")));
        lists.push(list);
    };
    for section in &resume.sections {
        skeleton.push(format!("section {}", section.title));
        let s = [&section.annotations];
        let mut run: Vec<Leaf> = Vec::new();
        for block in &section.blocks {
            if let Block::Paragraph { content, annotations } = block {
                run.push(leaf(content, annotations, &s));
                continue;
            }
            if !run.is_empty() {
                close(
                    &mut skeleton,
                    List {
                        kind: "paragraphs",
                        leaves: std::mem::take(&mut run),
                    },
                );
            }
            match block {
                Block::Paragraph { .. } => unreachable!(),
                Block::Bullets { items } => {
                    let leaves = items.iter().map(|b| leaf(&b.content, &b.annotations, &s)).collect();
                    close(
                        &mut skeleton,
                        List {
                            kind: "bullets",
                            leaves,
                        },
                    );
                }
                Block::PageBreak => skeleton.push("page-break".into()),
                Block::Role(role) => {
                    skeleton.push(format!("role {}", role.company));
                    if let Some(blurb) = &role.blurb {
                        skeleton.push(format!("blurb {}", id(blurb)));
                    }
                    let r = [&section.annotations, &role.annotations];
                    let mut run: Vec<Leaf> = Vec::new();
                    for part in &role.body {
                        if let RoleBlock::Paragraph { content, annotations } = part {
                            run.push(leaf(content, annotations, &r));
                            continue;
                        }
                        if !run.is_empty() {
                            close(
                                &mut skeleton,
                                List {
                                    kind: "paragraphs",
                                    leaves: std::mem::take(&mut run),
                                },
                            );
                        }
                        match part {
                            RoleBlock::Paragraph { .. } => unreachable!(),
                            RoleBlock::Bullets { items } => {
                                let leaves = items.iter().map(|b| leaf(&b.content, &b.annotations, &r)).collect();
                                close(
                                    &mut skeleton,
                                    List {
                                        kind: "bullets",
                                        leaves,
                                    },
                                );
                            }
                            RoleBlock::Era {
                                name,
                                items,
                                annotations,
                                ..
                            } => {
                                skeleton.push(format!("era {name}"));
                                let e = [&section.annotations, &role.annotations, annotations];
                                let leaves = items.iter().map(|b| leaf(&b.content, &b.annotations, &e)).collect();
                                close(
                                    &mut skeleton,
                                    List {
                                        kind: "bullets",
                                        leaves,
                                    },
                                );
                            }
                        }
                    }
                    if !run.is_empty() {
                        close(
                            &mut skeleton,
                            List {
                                kind: "paragraphs",
                                leaves: run,
                            },
                        );
                    }
                }
            }
        }
        if !run.is_empty() {
            close(
                &mut skeleton,
                List {
                    kind: "paragraphs",
                    leaves: run,
                },
            );
        }
    }
    (skeleton, lists)
}

/// The rank the rule gives, computed apart from the code under test: the smallest place in `promote` of any tag
/// the leaf or a container above it carries; `promote.len()` (after every place) for none.
fn oracle_rank(leaf: &Leaf, promote: &[Tag]) -> usize {
    let carries =
        |tag: &Tag| leaf.annotations.tags.contains(tag) || leaf.containers.iter().any(|c| c.tags.contains(tag));
    promote.iter().position(carries).unwrap_or(promote.len())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    #[test]
    fn promote_only_permutes_each_list_by_the_rule_stably_and_reports_exactly_what_rose(
        resume in resume_strategy(),
        selection in selection_strategy(),
        promote in promote_strategy(),
    ) {
        let Ok((plain, plain_report)) = select(&resume, &selection) else { return Ok(()) };
        let promoting = Selection { promote: promote.clone(), ..selection.clone() };
        let (ordered, report) = prop_assert_ok(select(&resume, &promoting))?;
        prop_assert!(plain_report.reordered.is_empty());

        // Same lists, same members, same everything else: nothing crosses a list, containers keep their order.
        let (plain_skeleton, plain_lists) = shape(&plain);
        let (ordered_skeleton, ordered_lists) = shape(&ordered);
        prop_assert_eq!(&plain_skeleton, &ordered_skeleton);
        // Ordering never changes what is kept or why.
        prop_assert_eq!(&report.dropped, &plain_report.dropped);

        let mut rose: BTreeSet<String> = BTreeSet::new();
        for (before, after) in plain_lists.iter().zip(&ordered_lists) {
            let was: BTreeMap<&str, usize> = before.leaves.iter().enumerate().map(|(i, l)| (l.id.as_str(), i)).collect();
            for pair in after.leaves.windows(2) {
                let (x, y) = (oracle_rank(&pair[0], &promote), oracle_rank(&pair[1], &promote));
                prop_assert!(x <= y, "{} ranks {x} above {} at {y}", pair[0].id, pair[1].id);
                if x == y {
                    prop_assert!(was[pair[0].id.as_str()] < was[pair[1].id.as_str()], "equal ranks swapped: {} {}", pair[0].id, pair[1].id);
                }
            }
            for (now, leaf) in after.leaves.iter().enumerate() {
                if now < was[leaf.id.as_str()] {
                    rose.insert(leaf.id.clone());
                }
            }
        }
        let reported: BTreeSet<String> = report.reordered.iter().flat_map(|r| r.moved_up.iter().map(|m| m.node.label.clone())).collect();
        prop_assert_eq!(&reported, &rose);
        for record in &report.reordered {
            prop_assert!(record.moved_up.iter().all(|m| m.to < m.from && m.from <= record.kept));
        }
        let firsts: Vec<usize> = report.reordered.iter().map(|r| r.moved_up[0].node.index).collect();
        let mut sorted = firsts.clone();
        sorted.sort_unstable();
        prop_assert_eq!(firsts, sorted, "records are in document order");

        // Reports are exact, not only as a set: one entry per item that rose, never repeated.
        let total: usize = report.reordered.iter().map(|r| r.moved_up.len()).sum();
        prop_assert_eq!(total, rose.len());

        // A promote list no item answers to changes nothing.
        if rose.is_empty() {
            prop_assert_eq!(&ordered, &plain);
        }

        // Ordering again changes nothing and reports nothing.
        let (again, second) = prop_assert_ok(select(&ordered, &promoting))?;
        prop_assert_eq!(&again, &ordered);
        prop_assert!(second.reordered.is_empty());
    }
}
