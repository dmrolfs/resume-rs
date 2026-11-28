//! ABOUTME: One pass over `Resume::nodes` that classifies every node and assigns its evidence or context ID.
//! ABOUTME: It holds the open section, role and era, the blurb group, the era cursor and the exclusion state.

use std::collections::{BTreeMap, HashMap};

use resume::markdown::Parsed;
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
        let provenance = walk.provenance(index, node, ranges[index].clone(), lines[index], nesting[index])?;
        walk.visit(node, provenance, nesting[index])?;
    }
    walk.finish()
}

/// A section, role or era that is open while the walk reads what it holds.
struct Open {
    depth: usize,
    id: String,
    /// What a record under it shows: the section title, `Company` or `Company · Title`, the era name.
    label: String,
}

/// Hands out each era's period in order, and notices when the walk and the model disagree about how many eras a role has.
///
/// `Resume::nodes` emits one era node per era, so the two cannot drift today. This guards a later parser change.
struct EraCursor(std::vec::IntoIter<Period>);

impl EraCursor {
    fn new(periods: Vec<Period>) -> Self {
        Self(periods.into_iter())
    }

    fn next(&mut self, node_index: usize) -> Result<Period, EvidenceError> {
        self.0.next().ok_or(EvidenceError::EraCursorMismatch { node_index })
    }

    /// Fails when an era of the role that has just closed was never met.
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
    source_id: String,
    section: Option<Open>,
    role: Option<Open>,
    era: Option<Open>,
    /// The periods of the open role's eras, in order; the next one is taken at each era heading.
    eras: EraCursor,
    /// The group of the open role's blurb: a paragraph in it is context too.
    blurb_group: Option<GroupName>,
    closed: Option<Closed>,
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
            source_id,
            section: None,
            role: None,
            era: None,
            eras: EraCursor::new(Vec::new()),
            blurb_group: None,
            closed: None,
            blocked: HashMap::new(),
            records: Vec::new(),
            context: Vec::new(),
            excluded: Vec::new(),
            ids: BTreeMap::new(),
        }
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
        let counted_line = line_of(self.text, range.start);
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
        let heading = match &role.title {
            Some(title) => format!("{} · {title}", role.company),
            None => role.company.to_string(),
        };
        self.eras = EraCursor::new(
            role.body
                .iter()
                .filter_map(|part| match part {
                    RoleBlock::Era { period, .. } => Some(period.clone()),
                    _ => None,
                })
                .collect(),
        );
        self.blurb_group = role
            .blurb
            .as_ref()
            .and_then(|_| role.blurb_annotations.alternate.as_ref().map(|a| a.group.clone()));
        self.role = Some(Open {
            depth: node.depth,
            id: id.clone(),
            label: heading,
        });
        self.era = None;
        self.open_closed(node.depth, index, own, inherited);
        if own.is_some() || inherited.is_some() {
            self.exclude(id, provenance, own, inherited);
            return Ok(());
        }
        let text = format!("{} · {}", heading_of(role), role.period.text);
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
        let period = self.eras.next(index)?;
        let id = self.ctx_id(index);
        self.claim(&id, index, true)?;
        let label = name.to_string();
        self.era = Some(Open {
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
        let generated = alternate.is_none() || context;
        let id = match alternate {
            Some(a) if !context => format!("{}.{}", a.group, a.variant),
            _ if context => self.ctx_id(index),
            _ => self.src_id(index),
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
        for local in &self.context {
            let p = &local.record.parents;
            if local.record.kind == ContextKind::RoleDescription
                && p.role.as_ref().is_some_and(|r| self.ids.contains_key(r))
            {
                used.extend([p.section.clone(), p.era.clone()].into_iter().flatten());
            }
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

/// The 1-based line of `offset`, counting `\n`, `\r\n` and a lone `\r` as one line ending each.
fn line_of(text: &str, offset: usize) -> usize {
    let bytes = text.as_bytes();
    let mut line = 1;
    let mut i = 0;
    while i < offset.min(bytes.len()) {
        match bytes[i] {
            b'\n' => line += 1,
            b'\r' if bytes.get(i + 1) != Some(&b'\n') => line += 1,
            _ => {}
        }
        i += 1;
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok, assert_some};
    use pretty_assertions::assert_eq;

    /// The periods of the eras of `**A · 2016 – 2017**` and `**B · 2018 – 2019**`, as the parser reads them.
    fn periods() -> Vec<Period> {
        let text =
            "# A B\n\n## E\n\n### Co · 2015 – 2020\n\n**A · 2016 – 2017**\n\n- One.\n\n**B · 2018 – 2019**\n\n- Two.\n";
        let parsed = resume::markdown::parse("r.md", text);
        let resume = assert_some!(parsed.valid());
        let periods: Vec<Period> = resume
            .sections
            .iter()
            .flat_map(|s| &s.blocks)
            .filter_map(|b| match b {
                resume::model::Block::Role(role) => Some(role),
                _ => None,
            })
            .flat_map(|role| &role.body)
            .filter_map(|part| match part {
                RoleBlock::Era { period, .. } => Some(period.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(periods.len(), 2);
        periods
    }

    #[test]
    fn the_era_cursor_hands_out_periods_in_order_and_refuses_one_too_many() {
        let mut cursor = EraCursor::new(periods());
        assert_eq!(assert_ok!(cursor.next(3)).text, "2016 – 2017");
        assert_eq!(assert_ok!(cursor.next(5)).text, "2018 – 2019");
        assert!(matches!(
            assert_err!(cursor.next(7)),
            EvidenceError::EraCursorMismatch { node_index: 7 }
        ));
    }

    #[test]
    fn the_era_cursor_refuses_to_close_a_role_with_an_era_never_met() {
        let mut cursor = EraCursor::new(periods());
        assert_ok!(cursor.next(3));
        assert!(matches!(
            assert_err!(cursor.finish(9)),
            EvidenceError::EraCursorMismatch { node_index: 9 }
        ));
        assert_ok!(cursor.next(5));
        assert_ok!(cursor.finish(9));
    }
}
