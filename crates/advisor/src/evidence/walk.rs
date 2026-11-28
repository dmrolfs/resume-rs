//! ABOUTME: One pass over `Resume::nodes` that classifies every node and assigns its evidence or context ID.
//! ABOUTME: It holds the open section, role and era, the blurb group, the era cursor and the exclusion state.

use std::collections::{BTreeMap, HashMap};

use resume::markdown::{Nesting, Parsed};
use resume::model::{
    GroupName, NodeHeading, NodeKind, NodeView, Period, Resume, RoleBlock, SectionTitle, VariantName, plain_text,
};
use snafu::ensure;

use super::types::{
    Blocker, ContextKind, DuplicateIdSnafu, EmptyEvidenceSetSnafu, EraCursorMismatchSnafu, Evidence, EvidenceError,
    EvidenceKind, Excluded, ExcludedStatus, GeneratedIdIsGroupedSnafu, InvalidSourceParentSnafu,
    InvalidSourceRangeSnafu, Local, MissingStructureSnafu, NodeMetadataLengthMismatchSnafu, Parents, Provenance,
    Record, SCHEMA, SourceLineMismatchSnafu,
};

/// Projects an already parsed, valid resume.
pub(super) fn project(
    text: &str,
    source_id: String,
    parsed: &Parsed,
    resume: &Resume,
) -> Result<Evidence, EvidenceError> {
    let nodes = resume.nodes();
    let ranges = parsed.node_ranges();
    let lines = parsed.node_lines(text);
    let nesting = parsed.node_nested_in();
    ensure!(
        nodes.len() == ranges.len() && nodes.len() == lines.len() && nodes.len() == nesting.len(),
        NodeMetadataLengthMismatchSnafu {
            nodes: nodes.len(),
            ranges: ranges.len(),
            lines: lines.len(),
            nesting: nesting.len()
        }
    );
    let mut walk = Walk::new(text, source_id);
    for (index, node) in nodes.iter().enumerate() {
        let parent = walk.resolve_parent(index, node, nesting[index], &nodes)?;
        let provenance = walk.provenance(index, node, ranges[index].clone(), lines[index], parent)?;
        walk.visit(node, provenance, parent)?;
    }
    // The last role is never left by a later node, so its eras are checked here.
    walk.eras.finish(nodes.len())?;
    walk.finish()
}

/// A section, role or era that is open while the walk reads what it holds.
struct Open {
    /// The node index of the heading.
    node_index: usize,
    depth: usize,
    id: String,
    /// What a record under it shows: the section title, `Company` or `Company · Title`, the era name.
    label: String,
}

/// Hands out each era's period in order, and notices when the walk and the model disagree about the eras of a role.
///
/// `Resume::nodes` emits one era node per era, in order, so the two cannot drift today. This guards a later parser
/// change: an era taken out of order, one too many, or one never met.
struct EraCursor(std::vec::IntoIter<(String, Period)>);

impl EraCursor {
    fn new(eras: Vec<(String, Period)>) -> Self {
        Self(eras.into_iter())
    }

    /// The period of the next era, which must be the one called `name`.
    fn next(&mut self, node_index: usize, name: &str) -> Result<Period, EvidenceError> {
        match self.0.next() {
            Some((expected, period)) if expected == name => Ok(period),
            _ => EraCursorMismatchSnafu { node_index }.fail(),
        }
    }

    /// Fails when an era of the role that has closed was never met.
    fn finish(&self, node_index: usize) -> Result<(), EvidenceError> {
        ensure!(self.0.len() == 0, EraCursorMismatchSnafu { node_index });
        Ok(())
    }
}

/// A container that is not kept, so nothing it holds is.
struct Closed {
    depth: usize,
    blocker: Blocker,
}

struct Walk<'a> {
    text: &'a str,
    /// Where each line of `text` starts, counted here to check the lines the parser reports.
    line_starts: Vec<usize>,
    source_id: String,
    section: Option<Open>,
    role: Option<Open>,
    era: Option<Open>,
    /// The periods of the open role's eras, in order; the next one is taken at each era heading.
    eras: EraCursor,
    /// The group of the open role's blurb: a paragraph in it is context too.
    blurb_group: Option<GroupName>,
    closed: Option<Closed>,
    /// The node index of the innermost open section, role or era, for each bullet visited, to check a nested bullet
    /// sits in the same one as its parent.
    container_of: HashMap<usize, usize>,
    /// What excludes a node, by node index, so a bullet nested in it inherits it.
    blocked: HashMap<usize, Blocker>,
    records: Vec<Local<EvidenceKind>>,
    context: Vec<Local<ContextKind>>,
    excluded: Vec<Excluded>,
    ids: BTreeMap<String, usize>,
}

impl<'a> Walk<'a> {
    fn new(text: &'a str, source_id: String) -> Self {
        Self {
            text,
            line_starts: line_starts(text),
            source_id,
            section: None,
            role: None,
            era: None,
            eras: EraCursor::new(Vec::new()),
            blurb_group: None,
            closed: None,
            container_of: HashMap::new(),
            blocked: HashMap::new(),
            records: Vec::new(),
            context: Vec::new(),
            excluded: Vec::new(),
            ids: BTreeMap::new(),
        }
    }

    /// The bullet a node is nested in, checked: it must exist, come earlier, be a bullet, and sit in the same
    /// section, role or era. A lost or wrong parent refuses the baseline, because a child of an excluded bullet
    /// would otherwise read as ready.
    fn resolve_parent(
        &self,
        index: usize,
        node: &NodeView<'_>,
        nesting: Nesting,
        nodes: &[NodeView<'_>],
    ) -> Result<Option<usize>, EvidenceError> {
        let parent = match nesting {
            Nesting::Top => return Ok(None),
            Nesting::In(parent) => parent,
            Nesting::Unresolved => {
                return InvalidSourceParentSnafu {
                    node_index: index,
                    parent: index,
                }
                .fail();
            }
        };
        let valid = parent < index
            && node.kind == NodeKind::Bullet
            && nodes.get(parent).is_some_and(|p| p.kind == NodeKind::Bullet)
            && self.container_of.get(&parent) == self.open_container().as_ref();
        ensure!(
            valid,
            InvalidSourceParentSnafu {
                node_index: index,
                parent
            }
        );
        Ok(Some(parent))
    }

    /// The node index of the innermost open era, else role, else section.
    fn open_container(&self) -> Option<usize> {
        self.era
            .as_ref()
            .or(self.role.as_ref())
            .or(self.section.as_ref())
            .map(|open| open.node_index)
    }

    fn provenance(
        &self,
        index: usize,
        node: &NodeView<'_>,
        range: std::ops::Range<usize>,
        line: usize,
        parent: Option<usize>,
    ) -> Result<Provenance, EvidenceError> {
        let fits = range.start <= range.end
            && range.end <= self.text.len()
            && self.text.is_char_boundary(range.start)
            && self.text.is_char_boundary(range.end);
        ensure!(
            fits,
            InvalidSourceRangeSnafu {
                node_index: index,
                range,
                input_bytes: self.text.len()
            }
        );
        let counted_line = line_in(&self.line_starts, range.start);
        ensure!(
            line == counted_line,
            SourceLineMismatchSnafu {
                node_index: index,
                parser_line: line,
                counted_line
            }
        );
        ensure!(
            parent.is_none_or(|p| p < index),
            InvalidSourceParentSnafu {
                node_index: index,
                parent: parent.unwrap_or_default()
            }
        );
        Ok(Provenance {
            node_index: index,
            node_kind: node.kind,
            byte_range: range,
            line,
            source_parent_node: parent,
            item: node.annotations.id.as_ref().map(|id| id.to_string()),
        })
    }

    fn visit(
        &mut self,
        node: &NodeView<'_>,
        provenance: Provenance,
        parent: Option<usize>,
    ) -> Result<(), EvidenceError> {
        let index = provenance.node_index;
        self.leave_containers(index, node.depth)?;
        if let (NodeKind::Bullet, Some(container)) = (node.kind, self.open_container()) {
            self.container_of.insert(index, container);
        }
        let own = node.annotations.status.map(ExcludedStatus::from);
        let inherited = self
            .closed
            .as_ref()
            .map(|c| c.blocker)
            .or_else(|| parent.and_then(|p| self.blocked.get(&p).copied()));
        // The first blocker above a node is the outermost, so a grandchild names the same node as its parent.
        let blocker = inherited.or(own.map(|status| Blocker {
            node_index: index,
            status,
        }));
        if let Some(blocker) = blocker {
            self.blocked.insert(index, blocker);
        }
        match node.kind {
            NodeKind::Section => self.section(node, provenance, own, inherited),
            NodeKind::Role => self.role(node, provenance, own, inherited),
            NodeKind::Era => self.era(node, provenance, own, inherited),
            NodeKind::Blurb | NodeKind::Paragraph | NodeKind::Bullet => self.leaf(node, provenance, own, inherited),
        }
    }

    /// Closes what the node at `depth` is no longer inside, and checks the era cursor when a role closes.
    fn leave_containers(&mut self, index: usize, depth: usize) -> Result<(), EvidenceError> {
        if self.closed.as_ref().is_some_and(|c| depth <= c.depth) {
            self.closed = None;
        }
        for slot in [&mut self.era, &mut self.role, &mut self.section] {
            if slot.as_ref().is_some_and(|open| depth <= open.depth) {
                *slot = None;
            }
        }
        if self.role.is_none() {
            self.eras.finish(index)?;
            self.blurb_group = None;
        }
        Ok(())
    }

    fn claim(&mut self, id: &str, index: usize, generated: bool) -> Result<(), EvidenceError> {
        if generated {
            ensure!(
                !is_group_variant(id),
                GeneratedIdIsGroupedSnafu {
                    id: id.to_string(),
                    node_index: index
                }
            );
        }
        if let Some(&first) = self.ids.get(id) {
            return DuplicateIdSnafu {
                id: id.to_string(),
                first_node: first,
                second_node: index,
            }
            .fail();
        }
        self.ids.insert(id.to_string(), index);
        Ok(())
    }

    fn src_id(&self, index: usize) -> String {
        format!("src:{}:node:{index}", self.source_id)
    }

    fn ctx_id(&self, index: usize) -> String {
        format!("ctx:{}:node:{index}", self.source_id)
    }

    fn parents(&self) -> Parents {
        Parents {
            section: self.section.as_ref().map(|o| o.id.clone()),
            role: self.role.as_ref().map(|o| o.id.clone()),
            era: self.era.as_ref().map(|o| o.id.clone()),
        }
    }

    fn labels(&self) -> (Option<String>, Option<String>, Option<String>) {
        let label = |o: &Option<Open>| o.as_ref().map(|o| o.label.clone());
        (label(&self.section), label(&self.role), label(&self.era))
    }

    fn exclude(&mut self, id: String, provenance: Provenance, own: Option<ExcludedStatus>, inherited: Option<Blocker>) {
        self.excluded.push(Excluded {
            id,
            provenance,
            own,
            inherited,
        });
    }

    /// Opens a container. An excluded one still gets an ID, so a child's exclusion can name it.
    fn open_closed(&mut self, depth: usize, index: usize, own: Option<ExcludedStatus>, inherited: Option<Blocker>) {
        if self.closed.is_none() {
            let blocker = inherited.or(own.map(|status| Blocker {
                node_index: index,
                status,
            }));
            self.closed = blocker.map(|blocker| Closed { depth, blocker });
        }
    }

    fn section(
        &mut self,
        node: &NodeView<'_>,
        provenance: Provenance,
        own: Option<ExcludedStatus>,
        inherited: Option<Blocker>,
    ) -> Result<(), EvidenceError> {
        let index = provenance.node_index;
        let Some(NodeHeading::Section(title)) = node.heading else {
            return MissingStructureSnafu {
                node_index: index,
                missing: "section heading",
            }
            .fail();
        };
        let title: &SectionTitle = title;
        let id = self.ctx_id(index);
        self.claim(&id, index, true)?;
        let label = title.to_string();
        self.section = Some(Open {
            node_index: index,
            depth: node.depth,
            id: id.clone(),
            label: label.clone(),
        });
        self.open_closed(node.depth, index, own, inherited);
        if own.is_some() || inherited.is_some() {
            self.exclude(id, provenance, own, inherited);
            return Ok(());
        }
        let record = Record {
            id,
            kind: ContextKind::Section,
            text: label.clone(),
            section: Some(label),
            role: None,
            era: None,
            group: None,
            variant: None,
            parents: Parents::default(),
        };
        self.context.push(Local { record, provenance });
        Ok(())
    }

    fn role(
        &mut self,
        node: &NodeView<'_>,
        provenance: Provenance,
        own: Option<ExcludedStatus>,
        inherited: Option<Blocker>,
    ) -> Result<(), EvidenceError> {
        let index = provenance.node_index;
        let Some(NodeHeading::Role(role)) = node.heading else {
            return MissingStructureSnafu {
                node_index: index,
                missing: "role heading",
            }
            .fail();
        };
        let id = self.src_id(index);
        self.claim(&id, index, true)?;
        let heading = heading_of(role);
        self.eras = EraCursor::new(
            role.body
                .iter()
                .filter_map(|part| match part {
                    RoleBlock::Era { name, period, .. } => Some((name.to_string(), period.clone())),
                    _ => None,
                })
                .collect(),
        );
        self.blurb_group = role
            .blurb
            .as_ref()
            .and_then(|_| role.blurb_annotations.alternate.as_ref().map(|a| a.group.clone()));
        self.role = Some(Open {
            node_index: index,
            depth: node.depth,
            id: id.clone(),
            label: heading.clone(),
        });
        self.era = None;
        self.open_closed(node.depth, index, own, inherited);
        if own.is_some() || inherited.is_some() {
            self.exclude(id, provenance, own, inherited);
            return Ok(());
        }
        let text = format!("{heading} · {}", role.period.text);
        let (section, role_label, _) = self.labels();
        let record = Record {
            id,
            kind: EvidenceKind::EmploymentRecord,
            text,
            section,
            role: role_label,
            era: None,
            group: None,
            variant: None,
            parents: Parents {
                section: self.section.as_ref().map(|o| o.id.clone()),
                role: None,
                era: None,
            },
        };
        self.records.push(Local { record, provenance });
        Ok(())
    }

    fn era(
        &mut self,
        node: &NodeView<'_>,
        provenance: Provenance,
        own: Option<ExcludedStatus>,
        inherited: Option<Blocker>,
    ) -> Result<(), EvidenceError> {
        let index = provenance.node_index;
        let Some(NodeHeading::Era(name)) = node.heading else {
            return MissingStructureSnafu {
                node_index: index,
                missing: "era heading",
            }
            .fail();
        };
        ensure!(
            self.role.is_some(),
            MissingStructureSnafu {
                node_index: index,
                missing: "role"
            }
        );
        let period = self.eras.next(index, name.as_ref())?;
        let id = self.ctx_id(index);
        self.claim(&id, index, true)?;
        let label = name.to_string();
        self.era = Some(Open {
            node_index: index,
            depth: node.depth,
            id: id.clone(),
            label: label.clone(),
        });
        self.open_closed(node.depth, index, own, inherited);
        if own.is_some() || inherited.is_some() {
            self.exclude(id, provenance, own, inherited);
            return Ok(());
        }
        let (section, role, _) = self.labels();
        let record = Record {
            id,
            kind: ContextKind::Era,
            text: format!("{label} · {}", period.text),
            section,
            role,
            era: Some(label),
            group: None,
            variant: None,
            parents: Parents {
                section: self.section.as_ref().map(|o| o.id.clone()),
                role: self.role.as_ref().map(|o| o.id.clone()),
                era: None,
            },
        };
        self.context.push(Local { record, provenance });
        Ok(())
    }

    fn leaf(
        &mut self,
        node: &NodeView<'_>,
        provenance: Provenance,
        own: Option<ExcludedStatus>,
        inherited: Option<Blocker>,
    ) -> Result<(), EvidenceError> {
        let index = provenance.node_index;
        let alternate = node.annotations.alternate.as_ref();
        let in_blurb_group = match (&self.blurb_group, alternate) {
            (Some(group), Some(a)) => *group == a.group && self.role.is_some(),
            _ => false,
        };
        let context = node.kind == NodeKind::Blurb || (node.kind == NodeKind::Paragraph && in_blurb_group);
        // A wording keeps its own `group.variant` ID. Everything else gets a generated one, which must never read as a
        // wording ID.
        let (id, generated) = match (context, alternate) {
            (true, _) => (self.ctx_id(index), true),
            (false, Some(a)) => (format!("{}.{}", a.group, a.variant), false),
            (false, None) => (self.src_id(index), true),
        };
        self.claim(&id, index, generated)?;
        if own.is_some() || inherited.is_some() {
            self.exclude(id, provenance, own, inherited);
            return Ok(());
        }
        let text = leaf_text(node);
        let (section, role, era) = self.labels();
        let (group, variant) = alternate.map_or((None, None), |a| {
            (Some(a.group.to_string()), Some(a.variant.to_string()))
        });
        let parents = self.parents();
        if context {
            let record = Record {
                id,
                kind: ContextKind::RoleDescription,
                text,
                section,
                role,
                era,
                group,
                variant,
                parents,
            };
            self.context.push(Local { record, provenance });
        } else {
            let record = Record {
                id,
                kind: EvidenceKind::BodyStatement,
                text,
                section,
                role,
                era,
                group,
                variant,
                parents,
            };
            self.records.push(Local { record, provenance });
        }
        Ok(())
    }

    /// Keeps the context a kept record points at, and refuses an empty evidence set.
    fn finish(mut self) -> Result<Evidence, EvidenceError> {
        ensure!(
            !self.records.is_empty(),
            EmptyEvidenceSetSnafu {
                excluded_nodes: self.excluded.len(),
                context_nodes: self.context.len()
            }
        );
        let mut used = std::collections::BTreeSet::new();
        for local in &self.records {
            let p = &local.record.parents;
            used.extend([p.section.clone(), p.role.clone(), p.era.clone()].into_iter().flatten());
        }
        // A role description is always sent, so the section and era it sits in are sent with it. A description of an
        // excluded role never gets here: it inherits the exclusion and is not a record.
        for local in self
            .context
            .iter()
            .filter(|local| local.record.kind == ContextKind::RoleDescription)
        {
            let p = &local.record.parents;
            used.extend([p.section.clone(), p.era.clone()].into_iter().flatten());
        }
        self.context
            .retain(|local| local.record.kind == ContextKind::RoleDescription || used.contains(&local.record.id));
        Ok(Evidence {
            schema: SCHEMA,
            source_id: self.source_id,
            records: self.records,
            context: self.context,
            excluded: self.excluded,
        })
    }
}

fn heading_of(role: &resume::model::Role) -> String {
    match &role.title {
        Some(title) => format!("{} · {title}", role.company),
        None => role.company.to_string(),
    }
}

/// The text a model reads for a paragraph, blurb or bullet; a bullet's label comes first, then `": "`.
fn leaf_text(node: &NodeView<'_>) -> String {
    let body = node.content.map(plain_text).unwrap_or_default();
    match node.label {
        Some(label) => format!("{}: {body}", plain_text(label)),
        None => body,
    }
}

/// True when `id` has the shape of a wording ID, `group.variant`, with both parts valid.
fn is_group_variant(id: &str) -> bool {
    id.split_once('.').is_some_and(|(group, variant)| {
        GroupName::try_new(group).is_ok() && VariantName::try_new(variant).is_ok() && !variant.contains('.')
    })
}

/// The byte offset where each line starts, counting `\n`, `\r\n` and a lone `\r` as one line ending each.
///
/// Built here from the bytes, not taken from the parser, so it can check the line the parser reports.
fn line_starts(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut starts = vec![0];
    for (i, &byte) in bytes.iter().enumerate() {
        if byte == b'\n' || (byte == b'\r' && bytes.get(i + 1) != Some(&b'\n')) {
            starts.push(i + 1);
        }
    }
    starts
}

/// The 1-based line of `offset` in a table from [`line_starts`].
fn line_in(starts: &[usize], offset: usize) -> usize {
    starts.partition_point(|&start| start <= offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok, assert_some};
    use pretty_assertions::assert_eq;

    /// The names and periods of the eras `A · 2016 – 2017` and `B · 2018 – 2019`, as the parser reads them.
    fn periods() -> Vec<(String, Period)> {
        let text =
            "# A B\n\n## E\n\n### Co · 2015 – 2020\n\n**A · 2016 – 2017**\n\n- One.\n\n**B · 2018 – 2019**\n\n- Two.\n";
        let parsed = resume::markdown::parse("r.md", text);
        let resume = assert_some!(parsed.valid());
        let periods: Vec<(String, Period)> = resume
            .sections
            .iter()
            .flat_map(|s| &s.blocks)
            .filter_map(|b| match b {
                resume::model::Block::Role(role) => Some(role),
                _ => None,
            })
            .flat_map(|role| &role.body)
            .filter_map(|part| match part {
                RoleBlock::Era { name, period, .. } => Some((name.to_string(), period.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(periods.len(), 2);
        periods
    }

    #[test]
    fn the_era_cursor_hands_out_periods_in_order_and_refuses_one_too_many() {
        let mut cursor = EraCursor::new(periods());
        assert_eq!(assert_ok!(cursor.next(3, "A")).text, "2016 – 2017");
        assert_eq!(assert_ok!(cursor.next(5, "B")).text, "2018 – 2019");
        assert!(matches!(
            assert_err!(cursor.next(7, "C")),
            EvidenceError::EraCursorMismatch { node_index: 7 }
        ));
    }

    #[test]
    fn the_era_cursor_refuses_an_era_taken_out_of_order() {
        let mut cursor = EraCursor::new(periods());
        let error = assert_err!(cursor.next(3, "B"));
        assert!(
            matches!(error, EvidenceError::EraCursorMismatch { node_index: 3 }),
            "{error:?}"
        );
    }

    #[test]
    fn the_era_cursor_refuses_to_close_a_role_with_an_era_never_met() {
        let mut cursor = EraCursor::new(periods());
        assert_ok!(cursor.next(3, "A"));
        assert!(matches!(
            assert_err!(cursor.finish(9)),
            EvidenceError::EraCursorMismatch { node_index: 9 }
        ));
        assert_ok!(cursor.next(5, "B"));
        assert_ok!(cursor.finish(9));
    }

    // -----------------------------------------------------------------------------------------------
    // The guards that parser and model drift would trip. A valid baseline never reaches them, so each is fed the
    // bad state directly, at the boundary it protects.
    // -----------------------------------------------------------------------------------------------

    const TEXT: &str = "# A B\n\nL\n\n## S\n\n- Café one.\n- Two.\n";

    fn walk() -> Walk<'static> {
        Walk::new(TEXT, "0".repeat(64))
    }

    /// Runs `with` on the section node and the first bullet of `TEXT`, the bullet being node 1.
    fn on_nodes<T>(with: impl FnOnce(&Resume, &[NodeView<'_>]) -> T) -> T {
        let parsed = resume::markdown::parse("r.md", TEXT);
        let resume = assert_some!(parsed.valid());
        let nodes = resume.nodes();
        with(resume, &nodes)
    }

    #[test]
    fn line_starts_follow_lf_crlf_and_lone_cr_and_give_one_based_lines() {
        // "a", "b", "c", "d" on four lines, joined three different ways in one text: LF, CRLF, then a lone CR.
        let text = "a\nb\r\nc\rd";
        let starts = line_starts(text);
        assert_eq!(starts, [0, 2, 5, 7]);
        let lines: Vec<usize> = ["a", "b", "c", "d"]
            .iter()
            .filter_map(|c| text.find(c))
            .map(|at| line_in(&starts, at))
            .collect();
        assert_eq!(lines, [1, 2, 3, 4]);
        assert_eq!(line_in(&starts, 0), 1);
        assert_eq!(line_in(&starts, text.len()), 4, "the end is on the last line");
        assert_eq!(line_starts(""), [0]);
    }

    #[test]
    fn only_two_valid_names_around_one_dot_read_as_a_group_variant() {
        for wording in ["group.default", "a.b", "ai-platform.short2"] {
            assert!(is_group_variant(wording), "{wording}");
        }
        for not_wording in [
            "Group.default",
            "group.Default",
            "group.",
            ".default",
            "group",
            "a.b.c",
            "1group.default",
            "src:0123:node:4",
            "ctx:0123:node:4",
        ] {
            assert!(!is_group_variant(not_wording), "{not_wording}");
        }
    }

    #[test]
    fn a_second_claim_of_one_id_names_both_nodes() {
        let mut walk = walk();
        assert_ok!(walk.claim("same.default", 1, false));
        let error = assert_err!(walk.claim("same.default", 2, false));
        assert!(
            matches!(error, EvidenceError::DuplicateId { ref id, first_node: 1, second_node: 2 } if id == "same.default"),
            "{error:?}"
        );
    }

    #[test]
    fn a_generated_id_must_not_read_as_group_dot_variant() {
        let mut walk = walk();
        let error = assert_err!(walk.claim("group.default", 4, true));
        assert!(
            matches!(error, EvidenceError::GeneratedIdIsGrouped { node_index: 4, .. }),
            "{error:?}"
        );
        assert_ok!(walk.claim(&walk.src_id(4), 4, true));
        assert_ok!(walk.claim("group.default", 5, false));
    }

    #[test]
    fn a_range_that_does_not_fit_the_text_or_cuts_a_character_is_refused() {
        on_nodes(|_, nodes| {
            let walk = walk();
            let accent = assert_some!(TEXT.find('é'), "the accent is in the text");
            let cases = [
                (std::ops::Range { start: 5, end: 3 }, "reversed"),
                (0..TEXT.len() + 1, "past the end"),
                (accent + 1..accent + 3, "inside the accent"),
            ];
            for (range, why) in cases {
                let error = assert_err!(walk.provenance(1, &nodes[1], range, 7, None), "{why}");
                assert!(
                    matches!(error, EvidenceError::InvalidSourceRange { node_index: 1, .. }),
                    "{why}: {error:?}"
                );
            }
        });
    }

    #[test]
    fn a_line_the_parser_reports_wrongly_is_refused_with_both_numbers() {
        on_nodes(|_, nodes| {
            let walk = walk();
            let start = assert_some!(TEXT.find("- Café"), "the bullet is in the text");
            let error = assert_err!(walk.provenance(1, &nodes[1], start..start + 7, 3, None));
            assert!(
                matches!(
                    error,
                    EvidenceError::SourceLineMismatch {
                        node_index: 1,
                        parser_line: 3,
                        counted_line: 7
                    }
                ),
                "{error:?}"
            );
            assert_ok!(walk.provenance(1, &nodes[1], start..start + 7, 7, None));
        });
    }

    #[test]
    fn a_parent_must_be_an_earlier_bullet_in_the_same_container() {
        on_nodes(|_, nodes| {
            let mut walk = walk();
            // The section is node 5, not 0, so a container read from the wrong place cannot match by luck.
            let section = Open {
                node_index: 5,
                depth: 0,
                id: "ctx:x".into(),
                label: "S".into(),
            };
            walk.section = Some(section);
            walk.container_of.insert(1, 5);
            walk.container_of.insert(2, 5);
            // Node 2 is the second bullet; node 1 is the first, in the same section.
            assert_eq!(
                assert_ok!(walk.resolve_parent(2, &nodes[2], Nesting::In(1), nodes)),
                Some(1)
            );
            assert_eq!(assert_ok!(walk.resolve_parent(2, &nodes[2], Nesting::Top, nodes)), None);
            for (nesting, why) in [
                (Nesting::In(2), "itself"),
                (Nesting::In(3), "a later node"),
                (Nesting::In(0), "a section, not a bullet"),
                (Nesting::Unresolved, "no parent found"),
            ] {
                let error = assert_err!(walk.resolve_parent(2, &nodes[2], nesting, nodes), "{why}");
                assert!(
                    matches!(error, EvidenceError::InvalidSourceParent { node_index: 2, .. }),
                    "{why}: {error:?}"
                );
            }
            // A parent that sits in another container is refused.
            walk.container_of.insert(1, 99);
            assert_err!(walk.resolve_parent(2, &nodes[2], Nesting::In(1), nodes));
        });
    }

    #[test]
    fn a_node_without_its_heading_is_refused_as_missing_structure() {
        on_nodes(|_, nodes| {
            for kind in [NodeKind::Section, NodeKind::Role, NodeKind::Era] {
                let mut node = nodes[0];
                node.kind = kind;
                node.heading = None;
                let mut walk = walk();
                let provenance = assert_ok!(walk.provenance(0, &node, 0..5, 1, None));
                let error = assert_err!(walk.visit(&node, provenance, None), "{kind:?}");
                assert!(
                    matches!(error, EvidenceError::MissingStructure { node_index: 0, .. }),
                    "{kind:?}: {error:?}"
                );
            }
        });
    }

    #[test]
    fn an_era_outside_any_role_is_refused() {
        let text = "# A B\n\n## S\n\n### Co · 2015 – 2020\n\n**A · 2016 – 2017**\n\n- One.\n";
        let parsed = resume::markdown::parse("r.md", text);
        let resume = assert_some!(parsed.valid());
        let nodes = resume.nodes();
        let era = assert_some!(nodes.iter().position(|n| n.kind == NodeKind::Era), "an era node");
        let mut walk = Walk::new(text, "0".repeat(64));
        let provenance = assert_ok!(walk.provenance(era, &nodes[era], 0..5, 1, None));
        let error = assert_err!(walk.visit(&nodes[era], provenance, None));
        assert!(
            matches!(error, EvidenceError::MissingStructure { missing: "role", .. }),
            "{error:?}"
        );
    }

    #[test]
    fn leaving_a_role_with_an_era_never_met_is_refused_where_the_walk_leaves_it() {
        // The cursor helper is tested alone above; this proves `leave_containers` still asks it.
        let mut walk = walk();
        walk.eras = EraCursor::new(periods());
        walk.role = Some(Open {
            node_index: 0,
            depth: 1,
            id: "src:r".into(),
            label: "R".into(),
        });
        assert_ok!(walk.leave_containers(5, 2));
        let error = assert_err!(walk.leave_containers(6, 1));
        assert!(
            matches!(error, EvidenceError::EraCursorMismatch { node_index: 6 }),
            "{error:?}"
        );
    }

    #[test]
    fn metadata_of_the_wrong_length_is_refused() {
        let parsed = resume::markdown::parse("r.md", TEXT);
        let resume = assert_some!(parsed.valid());
        let mut shorter = resume.clone();
        shorter.sections.clear();
        let error = assert_err!(project(TEXT, "0".repeat(64), &parsed, &shorter));
        assert!(
            matches!(error, EvidenceError::NodeMetadataLengthMismatch { .. }),
            "{error:?}"
        );
    }
}
