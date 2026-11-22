//! ABOUTME: The Org Leader draft's four reordered roles, rebuilt from one synthetic baseline with `promote`.
//! ABOUTME: Each case checks the leader order exactly, the architect order unchanged, and a no-promote control.

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
use resume::markdown::parse;
use resume::model::{Block, Inline, Resume, RoleBlock, Tag};
use resume::select::{Scope, SelectedTags, Selection, select};

/// One bullet of a fixture: its id, whether only the leader build keeps it, and its rank tag (1-based), if any.
struct B(&'static str, Audience, Option<u8>);

#[derive(Clone, Copy, PartialEq)]
enum Audience {
    /// In both builds (in the real baseline, a group with a default and a leader wording, or a general bullet).
    Both,
    /// Only the leader build keeps it, so its place in the baseline is free; it sits after its draft predecessor.
    Leader,
}

use Audience::{Both, Leader};

/// The role as Markdown. Shared bullets carry `#arch #lead`, so a rank tag never drops one from either build.
fn role(bullets: &[B]) -> Resume {
    let mut md = String::from("# Ada\n\nLondon\n\n## Experience\n\n### Acme · Engineer · 2015 – 2017\n\n");
    for B(id, audience, rank) in bullets {
        let mut tags = match audience {
            Both => vec!["#arch".to_string(), "#lead".to_string()],
            Leader => vec!["#lead".to_string()],
        };
        tags.extend(rank.map(|r| format!("#r{r}")));
        md.push_str(&format!("- {id} <!-- {} -->\n", tags.join(" ")));
    }
    assert_some!(parse("r.md", &md).valid(), "{md}").clone()
}

fn tag(n: &str) -> Tag {
    assert_ok!(Tag::try_new(n))
}

fn build(resume: &Resume, audience: &str, promote: &[&str]) -> Vec<String> {
    let selection = Selection {
        scope: Scope::Tags(assert_some!(SelectedTags::new([tag(audience)].into()))),
        promote: promote.iter().map(|t| tag(t)).collect(),
        ..Selection::default()
    };
    let (tailored, _) = assert_ok!(select(resume, &selection));
    let Block::Role(role) = &tailored.sections[0].blocks[0] else {
        panic!("{tailored:?}")
    };
    let [RoleBlock::Bullets { items }] = role.body.as_slice() else {
        panic!("{:?}", role.body)
    };
    items
        .iter()
        .map(|b| match b.content.as_slice() {
            [Inline::Text { text }] => text.clone(),
            other => format!("{other:?}"),
        })
        .collect()
}

/// Checks one role: the leader spec (`tags = ["lead"]` with `promote`) gives `leader`; without `promote`, the
/// baseline order; the architect spec (`tags = ["arch"]`, no `promote`) gives the shared bullets in baseline order,
/// which is what placing leader-only bullets freely and keeping every shared bullet `#arch` must not disturb.
fn check(bullets: &[B], promote: &[&str], leader: &[&str]) {
    let resume = role(bullets);
    assert_eq!(build(&resume, "lead", promote), leader, "leader order");
    let baseline: Vec<&str> = bullets.iter().map(|b| b.0).collect();
    assert_eq!(build(&resume, "lead", &[]), baseline, "no promote, no move");
    let shared: Vec<&str> = bullets.iter().filter(|b| b.1 == Both).map(|b| b.0).collect();
    assert_eq!(
        build(&resume, "arch", &[]),
        shared,
        "the architect order is the baseline's"
    );
}

// Derivation: history/rsm-141.23.3_draft-order-derivation.py (the drafts' orders, as baseline positions).

#[test]
fn the_pipeline_era_needs_four_ranks() {
    let bullets = [
        B("b1", Both, Some(1)),
        B("b2", Both, Some(1)),
        B("b3", Both, Some(1)),
        B("b4", Both, Some(4)),
        B("b5", Both, Some(3)),
        B("b6", Both, Some(2)),
        B("b7", Both, None),
        B("b8", Both, Some(4)),
        B("b9", Both, Some(1)),
    ];
    check(
        &bullets,
        &["r1", "r2", "r3", "r4"],
        &["b1", "b2", "b3", "b9", "b6", "b5", "b4", "b8", "b7"],
    );
}

#[test]
fn cdk_needs_one_rank() {
    let bullets = [
        B("b1", Both, Some(1)),
        B("b2", Leader, Some(1)),
        B("b3", Both, None),
        B("b4", Both, Some(1)),
        B("b5", Leader, Some(1)),
        B("b6", Both, None),
        B("b7", Both, None),
    ];
    check(&bullets, &["r1"], &["b1", "b2", "b4", "b5", "b3", "b6", "b7"]);
}

#[test]
fn rightside_needs_one_rank() {
    let bullets = [
        B("b1", Both, Some(1)),
        B("b2", Both, None),
        B("b3", Leader, None),
        B("b4", Both, None),
        B("b5", Leader, None),
        B("b6", Both, Some(1)),
        B("b7", Both, None),
    ];
    check(&bullets, &["r1"], &["b1", "b6", "b2", "b3", "b4", "b5", "b7"]);
}

#[test]
fn accenture_needs_one_rank() {
    let bullets = [
        B("b1", Both, None),
        B("b2", Both, Some(1)),
        B("b3", Both, None),
        B("b4", Leader, None),
        B("b5", Both, Some(1)),
        B("b6", Both, Some(1)),
    ];
    check(&bullets, &["r1"], &["b2", "b5", "b6", "b1", "b3", "b4"]);
}

#[test]
fn the_ai_era_needs_no_rank_once_its_leader_only_bullets_are_placed() {
    let bullets = [
        B("b1", Both, None),
        B("b2", Both, None),
        B("b3", Leader, None),
        B("b4", Both, None),
        B("b5", Leader, None),
        B("b6", Both, None),
        B("b7", Both, None),
        B("b8", Both, None),
        B("b9", Leader, None),
        B("b10", Both, None),
        B("b11", Leader, None),
    ];
    let order: Vec<&str> = bullets.iter().map(|b| b.0).collect();
    check(&bullets, &[], &order);
}
