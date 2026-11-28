use super::{Content, ContentItem, Outcome, render};
use ::resume::{model::Tag, pipeline::PreparedResume, select::SelectionReport};
use std::collections::{BTreeMap, BTreeSet};

impl Content {
    pub fn resume(prepared: &PreparedResume, assessment: Option<(&::resume::model::Resume, &SelectionReport)>) -> Self {
        let nodes = prepared.resume().nodes();
        let mut group_annotations = BTreeMap::<_, Vec<_>>::new();
        for node in &nodes {
            if let Some(a) = &node.annotations.alternate {
                group_annotations
                    .entry(a.group.clone())
                    .or_default()
                    .push(node.annotations);
            }
        }
        let group_tags: BTreeMap<_, _> = group_annotations
            .into_iter()
            .map(|(g, annotations)| (g, ::resume::select::rules::GroupFacts::of(annotations).tags))
            .collect();
        let mut ancestors = Vec::<(usize, BTreeSet<Tag>)>::new();
        let mut items: Vec<_> = nodes
            .iter()
            .zip(prepared.origins().iter())
            .map(|(node, origin)| {
                while ancestors.last().is_some_and(|(depth, _)| *depth >= node.depth) {
                    ancestors.pop();
                }
                let inherited_tags = ancestors.iter().flat_map(|(_, tags)| tags.iter().cloned()).collect();
                ancestors.push((node.depth, node.annotations.tags.clone()));
                ContentItem {
                    origin: origin.clone(),
                    annotations: node.annotations.clone(),
                    group_tags: node
                        .annotations
                        .alternate
                        .as_ref()
                        .and_then(|a| group_tags.get(&a.group))
                        .cloned()
                        .unwrap_or_default(),
                    inherited_tags,
                    depth: node.depth,
                    text: render::node(node),
                    outcome: Outcome::Unselected,
                    promoted: None,
                    mandatory: false,
                }
            })
            .collect();
        if let Some((_, report)) = assessment {
            let dropped: BTreeMap<_, _> = report.dropped.iter().map(|d| (d.node.index, d.reason)).collect();
            let groups: BTreeMap<_, _> = report
                .dropped
                .iter()
                .filter_map(|d| {
                    nodes[d.node.index]
                        .annotations
                        .alternate
                        .as_ref()
                        .map(|a| (&a.group, d.reason))
                })
                .collect();
            let chosen: BTreeMap<_, _> = report.chosen.iter().map(|c| (&c.group, c)).collect();
            for (i, item) in items.iter_mut().enumerate() {
                item.outcome = if let Some(a) = &item.annotations.alternate {
                    if let Some(reason) = groups.get(&a.group) {
                        Outcome::Dropped(*reason)
                    } else if let Some(c) = chosen.get(&a.group) {
                        if a.variant == c.variant {
                            Outcome::Chosen(c.by)
                        } else {
                            Outcome::NotChosen
                        }
                    } else {
                        Outcome::Unavailable
                    }
                } else {
                    dropped.get(&i).map_or(Outcome::Kept, |r| Outcome::Dropped(*r))
                };
            }
            for moved in report.reordered.iter().flat_map(|r| &r.moved_up) {
                items[moved.node.index].promoted = Some((moved.from, moved.to));
            }
        }
        Self {
            problems: vec![],
            rendered: Some(render::resume(
                assessment.map_or(prepared.resume(), |(selected, _)| selected),
            )),
            items,
            assessed: assessment.is_some(),
        }
    }
}
