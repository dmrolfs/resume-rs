//! ABOUTME: The resume walk of selection: resolves alternate groups, judges every item, drops empty containers.
//! ABOUTME: Inherited tags flow down section, role, era; a @draft container takes its subtree with it.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{
    Alternate, Annotations, Block, Bullet, EraName, GroupName, Inline, Period, Priority, Resume, Role, RoleBlock,
    Section, Status, Tag, VariantName,
};

use super::rules::{self, ChosenBy, DropReason, Facts, GroupFacts, Resolution, Variant, Verdict};
use super::{SelectError, Selection};

/// What a selection did, for the build summary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionReport {
    /// Every item, section, role, and era left out, with why, in document order.
    pub dropped: Vec<Dropped>,
    /// The wording each kept group renders.
    pub chosen: Vec<Chosen>,
    /// Kept items and containers still marked `@verify`; a container comes after what it holds, because it is
    /// known to be kept only once its contents are.
    pub verify: Vec<NodeRef>,
    /// Preferred wordings passed over because they are drafts.
    pub skipped_drafts: Vec<Alternate>,
    /// Picks that name a group or a variant the resume does not have.
    pub unknown_picks: Vec<UnknownPick>,
    /// With drafts off: each item, wording, and container marked `@draft`, except what a draft container holds
    /// (the container is counted, and its contents go with it).
    pub drafts_left_out: usize,
    /// With drafts on: each item, wording, and container marked `@draft`.
    pub drafts_included: usize,
}

/// One thing a selection left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub node: NodeRef,
    pub reason: DropReason,
}

/// The wording a kept group renders, and how it was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    pub group: GroupName,
    pub variant: VariantName,
    pub by: ChosenBy,
    /// The group's other wordings, in document order.
    pub not_chosen: Vec<VariantName>,
}

/// A pick that names nothing in the resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnknownPick {
    Group(GroupName),
    /// The group exists but has no variant of this name.
    Variant(Alternate),
}

/// What kind of node a report entry is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum NodeKind {
    Section,
    Role,
    Era,
    Blurb,
    Paragraph,
    Bullet,
}

/// A report's name for a node: its kind and a short label (a title, `Company · Title`, or a leaf's first words).
// shortcut: no source position, the model has none; join with the parser's outline if the summary needs lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRef {
    pub kind: NodeKind,
    pub label: String,
}

impl NodeRef {
    fn new(kind: NodeKind, label: impl Into<String>) -> Self {
        Self {
            kind,
            label: label.into(),
        }
    }

    /// A leaf, labelled by its first words.
    fn leaf(kind: NodeKind, content: &[Inline]) -> Self {
        const WORDS: usize = 8;
        let text = plain(content);
        let mut words = text.split_whitespace();
        let mut label: Vec<&str> = words.by_ref().take(WORDS).collect();
        if words.next().is_some() {
            label.push("…");
        }
        Self::new(kind, label.join(" "))
    }
}

fn plain(content: &[Inline]) -> String {
    content
        .iter()
        .map(|inline| match inline {
            Inline::Text { text } | Inline::Code { text } => text.clone(),
            Inline::Strong { content } | Inline::Emph { content } | Inline::Link { content, .. } => plain(content),
        })
        .collect()
}

/// Tailors `resume` to `selection`: keeps the selected items, renders one wording per kept group, and drops every
/// section, role, and era left with nothing under it. Text is never added or rewritten; the only move is a chosen
/// blurb wording written as the paragraph after the blurb taking the blurb's place.
///
/// Expects a resume that parsed with no errors, whose groups the parser checked: names unique, a `default` in
/// each, variants adjacent, of one kind, and with one priority.
///
/// # Errors
/// [`SelectError::PickedDraft`] when picks name draft wordings and drafts are off, listing every one.
pub fn select(resume: &Resume, selection: &Selection) -> Result<(Resume, SelectionReport), SelectError> {
    let mut walker = Walker::new(resume, selection)?;
    let sections = resume
        .sections
        .iter()
        .filter_map(|section| walker.section(section))
        .collect();
    let mut tailored = Resume::new(resume.name.clone(), resume.contact.clone(), sections);
    tailored.schema = resume.schema;
    Ok((tailored, walker.report))
}

type Tags = BTreeSet<Tag>;

/// One alternate group: its variants, the facts it is judged by, and the decision.
#[derive(Debug)]
struct Group {
    variants: Vec<(VariantName, Option<Status>)>,
    /// What the group is judged by; the kept variant carries `facts.tags`, so selecting again changes nothing.
    facts: GroupFacts,
    resolution: GroupResolution,
    /// The verdict, decided at its first variant (where its inherited tags are known), and so reported once.
    verdict: Option<Verdict>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GroupResolution {
    /// The variant it renders, by name (names are unique in a group), so the walk need not count variants.
    Chosen {
        variant: VariantName,
        by: ChosenBy,
        /// Preferred wordings passed over because they are drafts; reported only if the group is kept.
        skipped: Vec<VariantName>,
    },
    NoReady,
}

/// Whether a section, role, or era is kept, before its contents are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    Open,
    /// It, or a container above it, is a draft and drafts are off: it and everything under it is dropped.
    Closed,
}

struct Walker<'s> {
    selection: &'s Selection,
    groups: BTreeMap<GroupName, Group>,
    report: SelectionReport,
}

impl<'s> Walker<'s> {
    /// Collects and resolves every group, failing on draft picks before anything is built.
    fn new(resume: &Resume, selection: &'s Selection) -> Result<Self, SelectError> {
        let (mut groups, order) = collect_groups(resume);
        let mut picked_drafts = Vec::new();
        // In document order, so a list of errors reads top to bottom.
        for name in &order {
            let group = groups.get_mut(name).expect("collected");
            match resolve(name, group, selection) {
                Ok(resolution) => group.resolution = resolution,
                Err(pick) => picked_drafts.push(pick),
            }
        }
        if !picked_drafts.is_empty() {
            return Err(SelectError::PickedDraft { picks: picked_drafts });
        }
        let report = SelectionReport {
            unknown_picks: unknown_picks(&groups, selection),
            ..SelectionReport::default()
        };
        Ok(Self {
            selection,
            groups,
            report,
        })
    }

    fn section(&mut self, section: &Section) -> Option<Section> {
        let node = NodeRef::new(NodeKind::Section, section.title.as_ref());
        let gate = self.enter(&section.annotations, Gate::Open, &node);
        let tags = inherit(&Tags::new(), &section.annotations);
        let mut blocks = Vec::new();
        for block in &section.blocks {
            match block {
                Block::Paragraph { content, annotations } => {
                    let node = || NodeRef::leaf(NodeKind::Paragraph, content);
                    if let Some(annotations) = self.leaf(annotations, &tags, gate, None, node) {
                        blocks.push(Block::Paragraph {
                            content: content.clone(),
                            annotations,
                        });
                    }
                }
                Block::Bullets { items } => {
                    let items = self.bullets(items, &tags, gate);
                    if !items.is_empty() {
                        blocks.push(Block::Bullets { items });
                    }
                }
                Block::Role(role) => blocks.extend(self.role(role, &tags, gate).map(Block::Role)),
                Block::PageBreak => blocks.push(Block::PageBreak),
            }
        }
        let holds = |blocks: &[Block]| blocks.iter().any(|b| !matches!(b, Block::PageBreak));
        let emptied = holds(&section.blocks) && !holds(&blocks);
        self.leave(gate, emptied, &section.annotations, node).then(|| Section {
            title: section.title.clone(),
            blocks,
            annotations: section.annotations.clone(),
        })
    }

    fn role(&mut self, role: &Role, inherited: &Tags, above: Gate) -> Option<Role> {
        let node = NodeRef::new(NodeKind::Role, role_label(role));
        let gate = self.enter(&role.annotations, above, &node);
        let tags = inherit(inherited, &role.annotations);
        let blurb_group = role
            .blurb
            .as_ref()
            .and(role.blurb_annotations.alternate.as_ref())
            .map(|a| &a.group);
        let mut blurb = self.blurb(role, &tags, gate);
        let mut body = Vec::new();
        for part in &role.body {
            match part {
                RoleBlock::Paragraph { content, annotations } => {
                    let node = || NodeRef::leaf(NodeKind::Paragraph, content);
                    let Some(kept) = self.leaf(annotations, &tags, gate, None, node) else {
                        continue;
                    };
                    // The blurb's group renders here when its chosen wording follows the blurb: it takes the blurb's
                    // place, so a style still lays it out as the blurb.
                    let blurb_wording = blurb.is_none()
                        && body.is_empty()
                        && blurb_group.is_some_and(|g| annotations.alternate.as_ref().map(|a| &a.group) == Some(g));
                    if blurb_wording {
                        blurb = Some((content.clone(), kept));
                    } else {
                        body.push(RoleBlock::Paragraph {
                            content: content.clone(),
                            annotations: kept,
                        });
                    }
                }
                RoleBlock::Bullets { items } => {
                    let items = self.bullets(items, &tags, gate);
                    if !items.is_empty() {
                        body.push(RoleBlock::Bullets { items });
                    }
                }
                RoleBlock::Era {
                    name,
                    period,
                    items,
                    annotations,
                } => body.extend(self.era(name, period, items, annotations, &tags, gate)),
            }
        }
        let held = role.blurb.is_some() || !role.body.is_empty();
        let emptied = held && blurb.is_none() && body.is_empty();
        let (blurb, blurb_annotations) = blurb.unzip();
        self.leave(gate, emptied, &role.annotations, node).then(|| Role {
            company: role.company.clone(),
            title: role.title.clone(),
            period: role.period.clone(),
            blurb,
            body,
            annotations: role.annotations.clone(),
            blurb_annotations: blurb_annotations.unwrap_or_default(),
        })
    }

    /// The role's blurb when it is kept; a blurb without a priority of its own takes its role's, so a `@core` role
    /// keeps its blurb.
    fn blurb(&mut self, role: &Role, tags: &Tags, gate: Gate) -> Option<(Vec<Inline>, Annotations)> {
        let content = role.blurb.as_ref()?;
        let node = || NodeRef::leaf(NodeKind::Blurb, content);
        let kept = self.leaf(&role.blurb_annotations, tags, gate, role.annotations.priority, node)?;
        Some((content.clone(), kept))
    }

    fn era(
        &mut self,
        name: &EraName,
        period: &Period,
        items: &[Bullet],
        annotations: &Annotations,
        inherited: &Tags,
        above: Gate,
    ) -> Option<RoleBlock> {
        let node = NodeRef::new(NodeKind::Era, name.as_ref());
        let gate = self.enter(annotations, above, &node);
        let kept = self.bullets(items, &inherit(inherited, annotations), gate);
        let emptied = !items.is_empty() && kept.is_empty();
        self.leave(gate, emptied, annotations, node).then(|| RoleBlock::Era {
            name: name.clone(),
            period: period.clone(),
            items: kept,
            annotations: annotations.clone(),
        })
    }

    fn bullets(&mut self, items: &[Bullet], tags: &Tags, gate: Gate) -> Vec<Bullet> {
        items
            .iter()
            .filter_map(|bullet| {
                let node = || NodeRef::leaf(NodeKind::Bullet, &bullet.content);
                self.leaf(&bullet.annotations, tags, gate, None, node)
                    .map(|annotations| Bullet {
                        label: bullet.label.clone(),
                        content: bullet.content.clone(),
                        annotations,
                    })
            })
            .collect()
    }

    /// Opens a container: reports it when a draft closes it, and says whether its contents can be kept.
    fn enter(&mut self, annotations: &Annotations, above: Gate, node: &NodeRef) -> Gate {
        let draft = annotations.status == Some(Status::Draft);
        if above == Gate::Closed {
            self.drop_node(node.clone(), DropReason::UnderDraft);
            return Gate::Closed;
        }
        self.count_draft(annotations);
        if draft && !self.selection.drafts {
            self.drop_node(node.clone(), DropReason::Draft);
            return Gate::Closed;
        }
        Gate::Open
    }

    /// Counts a node marked `@draft`: as left out with drafts off, as included with drafts on.
    fn count_draft(&mut self, annotations: &Annotations) {
        if annotations.status == Some(Status::Draft) {
            if self.selection.drafts {
                self.report.drafts_included += 1;
            } else {
                self.report.drafts_left_out += 1;
            }
        }
    }

    /// Closes a container: true when it is kept. An open container that held something, all of which selection
    /// left out, is dropped as empty; one the author wrote empty stays, as it would without selection.
    fn leave(&mut self, gate: Gate, emptied: bool, annotations: &Annotations, node: NodeRef) -> bool {
        if gate == Gate::Closed {
            return false;
        }
        if emptied {
            self.drop_node(node, DropReason::Empty);
            return false;
        }
        if annotations.status == Some(Status::Verify) {
            self.report.verify.push(node);
        }
        true
    }

    /// Judges one leaf and returns the annotations it keeps, or `None` when it is left out.
    ///
    /// `fallback` is the priority a leaf without its own takes (a blurb takes its role's).
    fn leaf(
        &mut self,
        annotations: &Annotations,
        inherited: &Tags,
        gate: Gate,
        fallback: Option<Priority>,
        node: impl Fn() -> NodeRef,
    ) -> Option<Annotations> {
        if let Some(alternate) = &annotations.alternate {
            return self.variant(alternate, annotations, inherited, gate, fallback, node);
        }
        if gate == Gate::Closed {
            self.drop_node(node(), DropReason::UnderDraft);
            return None;
        }
        let facts = Facts {
            priority: annotations.priority.or(fallback),
            status: annotations.status,
            own_tags: &annotations.tags,
            inherited,
        };
        match rules::judge(&facts, self.selection) {
            Verdict::Drop(reason) => {
                self.count_draft(annotations);
                self.drop_node(node(), reason);
                None
            }
            Verdict::Keep => {
                self.kept(annotations, node);
                Some(annotations.clone())
            }
        }
    }

    /// One variant of a group: kept when the group is kept and this is its chosen wording.
    fn variant(
        &mut self,
        alternate: &Alternate,
        annotations: &Annotations,
        inherited: &Tags,
        gate: Gate,
        fallback: Option<Priority>,
        node: impl Fn() -> NodeRef,
    ) -> Option<Annotations> {
        let selection = self.selection;
        let group = self.groups.get_mut(&alternate.group).expect("the walk saw every group");
        let first = group.verdict.is_none();
        let verdict = if gate == Gate::Closed {
            Verdict::Drop(DropReason::UnderDraft)
        } else {
            rules::judge_group(&group.facts, inherited, fallback, selection)
        };
        let verdict = *group.verdict.get_or_insert(verdict);
        let reason = match (verdict, &group.resolution) {
            (Verdict::Drop(reason), _) => reason,
            (Verdict::Keep, GroupResolution::NoReady) => DropReason::NoReadyVariant,
            (Verdict::Keep, GroupResolution::Chosen { variant, by, skipped }) => {
                if *variant != alternate.variant {
                    self.count_draft(annotations);
                    return None;
                }
                let not_chosen = group.variants.iter().map(|(n, _)| n.clone()).filter(|n| n != variant);
                let chosen = Chosen {
                    group: alternate.group.clone(),
                    variant: variant.clone(),
                    by: *by,
                    not_chosen: not_chosen.collect(),
                };
                let skipped = skipped.iter().map(|v| Alternate {
                    group: alternate.group.clone(),
                    variant: v.clone(),
                });
                self.report.skipped_drafts.extend(skipped);
                let tags = group.facts.tags.clone();
                self.report.chosen.push(chosen);
                self.kept(annotations, node);
                return Some(Annotations {
                    tags,
                    ..annotations.clone()
                });
            }
        };
        // Under a draft container the container is counted, not what it holds.
        if reason != DropReason::UnderDraft {
            self.count_draft(annotations);
        }
        // A group is one item: a dropped group is reported once, at its first variant.
        if first {
            self.drop_node(node(), reason);
        }
        None
    }

    /// Records a kept leaf: counts it if it is a draft, lists it if it is to verify.
    fn kept(&mut self, annotations: &Annotations, node: impl Fn() -> NodeRef) {
        self.count_draft(annotations);
        if annotations.status == Some(Status::Verify) {
            self.report.verify.push(node());
        }
    }

    fn drop_node(&mut self, node: NodeRef, reason: DropReason) {
        self.report.dropped.push(Dropped { node, reason });
    }
}

/// The annotations of every leaf, in the walk's order: section paragraphs and bullets, then each role's blurb,
/// body paragraphs, bullets, and era bullets.
fn leaf_annotations(resume: &Resume) -> Vec<&Annotations> {
    let mut out = Vec::new();
    for section in &resume.sections {
        for block in &section.blocks {
            match block {
                Block::Paragraph { annotations, .. } => out.push(annotations),
                Block::Bullets { items } => out.extend(items.iter().map(|b| &b.annotations)),
                Block::Role(role) => {
                    if role.blurb.is_some() {
                        out.push(&role.blurb_annotations);
                    }
                    for part in &role.body {
                        match part {
                            RoleBlock::Paragraph { annotations, .. } => out.push(annotations),
                            RoleBlock::Bullets { items } | RoleBlock::Era { items, .. } => {
                                out.extend(items.iter().map(|b| &b.annotations));
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

/// Every group's variants and facts, with the group names in document order.
fn collect_groups(resume: &Resume) -> (BTreeMap<GroupName, Group>, Vec<GroupName>) {
    let mut groups: BTreeMap<GroupName, Group> = BTreeMap::new();
    let mut variants: BTreeMap<GroupName, Vec<&Annotations>> = BTreeMap::new();
    let mut order = Vec::new();
    for annotations in leaf_annotations(resume) {
        let Some(alternate) = &annotations.alternate else {
            continue;
        };
        let group = groups.entry(alternate.group.clone()).or_insert_with(|| {
            order.push(alternate.group.clone());
            Group {
                variants: Vec::new(),
                facts: GroupFacts::default(),
                resolution: GroupResolution::NoReady,
                verdict: None,
            }
        });
        group.variants.push((alternate.variant.clone(), annotations.status));
        variants.entry(alternate.group.clone()).or_default().push(annotations);
    }
    for (name, group) in &mut groups {
        group.facts = GroupFacts::of(variants[name].iter().copied());
    }
    (groups, order)
}

/// Resolves one group with the shared rule, as a decision the walk keeps by variant name.
fn resolve(name: &GroupName, group: &Group, selection: &Selection) -> Result<GroupResolution, Alternate> {
    let variants: Vec<Variant<'_>> = group
        .variants
        .iter()
        .map(|(variant, status)| Variant {
            name: variant,
            status: *status,
        })
        .collect();
    Ok(match rules::resolve(name, &variants, selection)? {
        Resolution::Chosen { index, by, skipped } => GroupResolution::Chosen {
            variant: group.variants[index].0.clone(),
            by,
            skipped: skipped.into_iter().cloned().collect(),
        },
        Resolution::NoReady => GroupResolution::NoReady,
    })
}

fn unknown_picks(groups: &BTreeMap<GroupName, Group>, selection: &Selection) -> Vec<UnknownPick> {
    selection
        .picks
        .iter()
        .filter_map(|(group, variant)| match groups.get(group) {
            None => Some(UnknownPick::Group(group.clone())),
            Some(found) if !found.variants.iter().any(|(name, _)| name == variant) => {
                Some(UnknownPick::Variant(Alternate {
                    group: group.clone(),
                    variant: variant.clone(),
                }))
            }
            Some(_) => None,
        })
        .collect()
}

/// The tags a container passes down: its parent's and its own.
fn inherit(parent: &Tags, annotations: &Annotations) -> Tags {
    parent.union(&annotations.tags).cloned().collect()
}

fn role_label(role: &Role) -> String {
    match &role.title {
        Some(title) => format!("{} · {}", role.company, title),
        None => role.company.to_string(),
    }
}
