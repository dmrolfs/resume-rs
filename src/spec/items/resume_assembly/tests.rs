use super::*;
use crate::diagnostics::{SourceKind, SourceText};
use crate::model::{Block, ItemId, RoleBlock};
use crate::spec::{
    Located,
    additions::{AdditionMetadata, LeafPlace, ResumePlace},
    layer::Sourced,
};
use claims::{assert_err, assert_ok, assert_some};
use pretty_assertions::assert_eq;

fn named(id: &str) -> Sourced<ItemId> {
    Sourced {
        value: assert_ok!(ItemId::try_new(id)),
        at: Located {
            path: std::path::Path::new("position.toml").into(),
            line: 2,
            column: 1,
        },
    }
}
fn parsed(body: &str) -> (Resume, Vec<ItemOrigin>, usize) {
    let text = format!("# Ada\n\nHere\n\n{body}\n");
    let parsed = crate::markdown::parse("baseline.md", &text);
    let model = assert_some!(parsed.valid(), "{:?}", parsed.diagnostics()).clone();
    let origins = parsed
        .node_ranges()
        .iter()
        .map(|range| ItemOrigin::Baseline {
            file: "baseline.md".into(),
            range: range.clone(),
        })
        .collect();
    (model, origins, text.len())
}
fn addition(id: &str, place: ResumePlace) -> ResumeAddition {
    let literal = format!("'{id}'");
    let source = SourceText::new(format!("{id}.toml").into(), SourceKind::Spec, literal.as_str());
    assert_ok!(ResumeAddition::new(
        named(id),
        place,
        AdditionMetadata::default(),
        &source,
        0..literal.len()
    ))
}
fn bullet(place: LeafPlace) -> ResumePlace {
    ResumePlace::Bullet(place)
}
fn paragraph(place: LeafPlace) -> ResumePlace {
    ResumePlace::Paragraph(place)
}
fn run(base: &(Resume, Vec<ItemOrigin>, usize), additions: &[ResumeAddition]) -> PreparedResume {
    let built = assert_ok!(assemble_resume(&base.0, &base.1, base.2, additions, None));
    assert_origin_order(base, &built, additions);
    built
}
fn ids(resume: &Resume) -> Vec<String> {
    resume
        .nodes()
        .iter()
        .map(|n| {
            n.annotations
                .id
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| match n.heading {
                    Some(crate::model::NodeHeading::Section(title)) => title.to_string(),
                    _ => n.content.map(crate::model::plain_text).unwrap_or_default(),
                })
        })
        .collect()
}
fn assert_origin_order(base: &(Resume, Vec<ItemOrigin>, usize), built: &PreparedResume, additions: &[ResumeAddition]) {
    let mut original = base.0.nodes().into_iter().zip(&base.1);
    for (node, origin) in built.resume().nodes().into_iter().zip(built.origins().iter()) {
        let added = node
            .annotations
            .id
            .as_deref()
            .and_then(|id| additions.iter().find(|a| &a.id().value == id));
        if let Some(added) = added {
            assert_eq!(origin, added.content().origin());
        } else {
            let (expected, expected_origin) = assert_some!(original.next());
            assert_eq!(node.kind, expected.kind);
            assert_eq!(node.content, expected.content);
            assert_eq!(node.label, expected.label);
            assert_eq!(origin, expected_origin);
        }
    }
    assert!(original.next().is_none());
}

fn block_shapes(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .map(|b| match b {
            Block::Bullets { items } => format!(
                "list:{}",
                items
                    .iter()
                    .map(|b| crate::model::plain_text(&b.content))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Block::Paragraph { content, .. } => format!("p:{}", crate::model::plain_text(content)),
            Block::PageBreak => "break".into(),
            Block::Role(_) => "role".into(),
        })
        .collect()
}

#[test]
fn container_edges_and_bullet_anchors_share_mixed_run_buckets() {
    let base = parsed("## S <!-- &s -->\n\n- a <!-- &a -->\n- b <!-- &b -->");
    for start in [true, false] {
        let parent = if start {
            LeafPlace::Start(named("s"))
        } else {
            LeafPlace::End(named("s"))
        };
        let anchor = if start {
            LeafPlace::Before(named("a"))
        } else {
            LeafPlace::After(named("b"))
        };
        let additions = [
            addition("x", bullet(anchor)),
            addition("p", paragraph(parent.clone())),
            addition("y", bullet(parent.clone())),
        ];
        let built = run(&base, &additions);
        assert_eq!(
            block_shapes(&built.resume().sections[0].blocks),
            if start {
                vec!["list:x", "p:p", "list:y,a,b"]
            } else {
                vec!["list:a,b,x", "p:p", "list:y"]
            }
        );
        let alias = [
            addition("x", bullet(parent.clone())),
            additions[1].clone(),
            additions[2].clone(),
        ];
        assert_eq!(run(&base, &alias), built);
        assert_eq!(built.resume().nodes().len(), built.origins().len());
    }
}

#[test]
fn interior_gaps_groups_era_and_page_break_keep_existing_structure() {
    let base = parsed(
        "## S <!-- &s -->\n\nP <!-- &p -->\n\n- a <!-- =g.default -->\n- alternate <!-- =g.short -->\n- b <!-- &b -->\n\n<div class=\"page-break\"></div>\n\n### Acme · Engineer · 2020 – Present <!-- &r -->\n\nBlurb <!-- &blurb -->\n\n**Era · 2020 – Present** <!-- &era -->\n\n- e <!-- &e -->",
    );
    let additions = [
        addition("x", bullet(LeafPlace::After(named("g")))),
        addition("y", bullet(LeafPlace::Before(named("b")))),
        addition("z", bullet(LeafPlace::End(named("era")))),
    ];
    let built = run(&base, &additions);
    let Block::Bullets { items } = &built.resume().sections[0].blocks[1] else {
        panic!("list")
    };
    assert_eq!(
        items
            .iter()
            .map(|b| crate::model::plain_text(&b.content))
            .collect::<Vec<_>>(),
        ["a", "alternate", "x", "y", "b"]
    );
    assert!(matches!(built.resume().sections[0].blocks[2], Block::PageBreak));
    assert_eq!(ids(built.resume()).last().map(String::as_str), Some("z"));
    for origin in &base.1 {
        assert_eq!(built.origins().iter().filter(|o| *o == origin).count(), 1);
    }
}

#[test]
fn role_start_and_after_blurb_group_do_not_split_wordings() {
    let base = parsed(
        "## S\n\n### Acme · Engineer · 2020 – Present <!-- &r -->\n\nDefault <!-- =intro.default -->\n\nShort <!-- =intro.short -->\n\n- a <!-- &a -->",
    );
    let a = addition("x", paragraph(LeafPlace::Start(named("r"))));
    let b = addition("x", paragraph(LeafPlace::After(named("intro"))));
    let built = run(&base, &[a]);
    assert_eq!(built, run(&base, &[b]));
    let Block::Role(role) = &built.resume().sections[0].blocks[0] else {
        panic!("role")
    };
    let RoleBlock::Paragraph { annotations, .. } = &role.body[0] else {
        panic!("variant")
    };
    assert_eq!(
        annotations.alternate.as_ref().map(|a| a.variant.as_ref()),
        Some("short")
    );
    let selection = crate::select::Selection {
        prefer: vec![assert_ok!(crate::model::VariantName::try_new("short"))],
        ..Default::default()
    };
    let (selected, _) = assert_ok!(crate::select::select(built.resume(), &selection));
    let Block::Role(role) = &selected.sections[0].blocks[0] else {
        panic!("role")
    };
    assert_eq!(crate::model::plain_text(assert_some!(role.blurb.as_ref())), "Short");
}

#[test]
fn absent_blurb_insertion_has_its_own_origin_and_occupied_slots_fail() {
    let base = parsed("## S\n\n### Acme · Engineer · 2020 – Present <!-- &r -->\n\n- a <!-- &a -->");
    let added = addition("blurb", ResumePlace::Blurb { role: named("r") });
    let built = run(&base, std::slice::from_ref(&added));
    assert_eq!(ids(built.resume()), ["S", "r", "blurb", "a"]);
    assert_eq!(built.origins()[2], *added.content().origin());
    let duplicate = addition("other", ResumePlace::Blurb { role: named("r") });
    assert_err!(assemble_resume(
        &base.0,
        &base.1,
        base.2,
        &[added.clone(), duplicate.clone()],
        None
    ));
    assert_err!(assemble_resume(
        built.resume(),
        built.origins(),
        base.2,
        &[duplicate],
        None
    ));
}

#[test]
fn role_edge_aliases_and_empty_blurb_group_body_preserve_origins() {
    for list in ["- a <!-- =g.default -->\n- alt <!-- =g.short -->", ""] {
        let base = parsed(&format!(
            "## S\n\n### Acme · Engineer · 2020 – Present <!-- &r -->\n\nIntro <!-- =intro.default -->\n\nBrief <!-- =intro.short -->\n\n{list}"
        ));
        for start in [true, false] {
            let edge = if start {
                LeafPlace::Start(named("r"))
            } else {
                LeafPlace::End(named("r"))
            };
            let additions = [
                addition("x", bullet(edge.clone())),
                addition("p", paragraph(edge.clone())),
                addition("y", bullet(edge.clone())),
            ];
            let built = run(&base, &additions);
            let Block::Role(role) = &built.resume().sections[0].blocks[0] else {
                panic!("role")
            };
            assert!(
                matches!(&role.body[0], RoleBlock::Paragraph { annotations, .. } if annotations.alternate.is_some())
            );
            if !list.is_empty() {
                let anchor = if start {
                    LeafPlace::Before(named("g"))
                } else {
                    LeafPlace::After(named("g"))
                };
                let alias = [
                    addition("x", bullet(anchor)),
                    additions[1].clone(),
                    additions[2].clone(),
                ];
                assert_eq!(run(&base, &alias), built);
            } else {
                let opposite = if start {
                    LeafPlace::End(named("r"))
                } else {
                    LeafPlace::Start(named("r"))
                };
                assert_eq!(
                    built,
                    run(
                        &base,
                        &[
                            addition("x", bullet(opposite.clone())),
                            addition("p", paragraph(opposite.clone())),
                            addition("y", bullet(opposite))
                        ]
                    )
                );
            }
        }
    }
}

#[test]
fn paragraph_and_era_gaps_are_equivalent_but_distinct_lists_stay_distinct() {
    let base = parsed(
        "## S <!-- &s -->\n\nA <!-- &a -->\n\nB <!-- &b -->\n\n### Acme · Engineer · 2020 – Present <!-- &r -->\n\n**Era · 2020 – Present** <!-- &era -->\n\n- e <!-- &e -->",
    );
    assert_eq!(
        run(&base, &[addition("p", paragraph(LeafPlace::After(named("a"))))]),
        run(&base, &[addition("p", paragraph(LeafPlace::Before(named("b"))))])
    );
    for (edge, anchor) in [
        (LeafPlace::Start(named("era")), LeafPlace::Before(named("e"))),
        (LeafPlace::End(named("era")), LeafPlace::After(named("e"))),
    ] {
        assert_eq!(
            run(&base, &[addition("x", bullet(edge))]),
            run(&base, &[addition("x", bullet(anchor))])
        );
    }
    let mut lists = parsed("## S <!-- &s -->\n\n- a <!-- &a -->\n- b <!-- &b -->");
    let Block::Bullets { items } = &mut lists.0.sections[0].blocks[0] else {
        panic!("list")
    };
    let second = items.split_off(1);
    lists.0.sections[0].blocks.push(Block::Bullets { items: second });
    let built = run(&lists, &[addition("x", bullet(LeafPlace::End(named("s"))))]);
    assert_eq!(block_shapes(&built.resume().sections[0].blocks), ["list:a", "list:b,x"]);
    assert_eq!(run(&lists, &[]).resume(), &lists.0);
}

#[test]
fn overrides_apply_only_to_baseline_and_all_binding_errors_are_typed() {
    use crate::spec::merge::{ChoiceKey, ItemOverride};
    let base = parsed("## S <!-- &s -->\n\n- a <!-- &a @extra -->");
    let changes = BTreeMap::from([(
        named("a").value,
        ItemOverride {
            tags: None,
            priority: Some(crate::model::PriorityEdit::Core),
        },
    )]);
    let origins = BTreeMap::from([(ChoiceKey::OverridePriority(named("a").value), named("a").at)]);
    let overrides = ItemOverrides::new(&changes, &origins);
    let empty = assert_ok!(assemble_resume(&base.0, &base.1, base.2, &[], Some(overrides)));
    assert_eq!(
        empty.resume(),
        &assert_ok!(crate::spec::items::prepare_resume(&base.0, overrides))
    );
    let added = addition("x", bullet(LeafPlace::Before(named("a"))));
    let built = assert_ok!(assemble_resume(
        &base.0,
        &base.1,
        base.2,
        std::slice::from_ref(&added),
        Some(overrides)
    ));
    let nodes = built.resume().nodes();
    assert_eq!(nodes[1].annotations.priority, None);
    assert_eq!(nodes[2].annotations.priority, Some(crate::model::Priority::Core));
    assert_eq!(built.origins()[2], base.1[1]);
    let bad = [
        addition("a", bullet(LeafPlace::After(named("a")))),
        addition("x", bullet(LeafPlace::After(named("missing")))),
        addition("x", paragraph(LeafPlace::Before(named("a")))),
        addition("y", bullet(LeafPlace::After(named("x")))),
    ];
    let PrepareError::AdditionBinding { problems } = assert_err!(assemble_resume(&base.0, &base.1, base.2, &bad, None))
    else {
        panic!("binding")
    };
    assert_eq!(
        problems.iter().map(|p| &p.reason).collect::<Vec<_>>(),
        [
            &BindingReason::BaselineCollision,
            &BindingReason::DuplicateAddition,
            &BindingReason::MissingReference,
            &BindingReason::WrongTargetKind,
            &BindingReason::AdditionTarget
        ]
    );
    assert!(problems.iter().all(|p| p.at.line == 2 && p.at.column == 1));
}

#[test]
fn no_op_repeat_budget_and_binding_failures_leave_baseline_unchanged() {
    let base = parsed("## S <!-- &s -->\n\n- a <!-- &a -->");
    assert_eq!(run(&base, &[]).resume(), &base.0);
    let a = addition("x", bullet(LeafPlace::After(named("a"))));
    assert_eq!(
        run(&base, std::slice::from_ref(&a)),
        run(&base, std::slice::from_ref(&a))
    );
    assert_ok!(assemble_resume(
        &base.0,
        &base.1,
        crate::markdown::MAX_INPUT_BYTES - 1,
        std::slice::from_ref(&a),
        None
    ));
    assert!(matches!(
        assert_err!(assemble_resume(&base.0, &base.1, usize::MAX, &[a], None)),
        PrepareError::PreparedContentLimit { .. }
    ));
    for place in [
        paragraph(LeafPlace::After(named("a"))),
        bullet(LeafPlace::Before(named("s"))),
        paragraph(LeafPlace::After(named("missing"))),
    ] {
        assert_err!(assemble_resume(&base.0, &base.1, base.2, &[addition("x", place)], None));
    }
    assert_eq!(ids(&base.0), ["s", "a"]);
}
