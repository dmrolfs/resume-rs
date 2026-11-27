use super::{Gap, List, Owner};
use crate::model::{Annotations, Block, Bullet, ItemId, Resume, Role, RoleBlock};
use crate::spec::additions::{LeafPlace, ResumeAddition, ResumePlace};
use crate::spec::items::{BindingProblem, BindingReason};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Before,
    After,
    Start,
    End,
    Blurb,
}

#[derive(Clone)]
enum Target {
    Container {
        owner: Owner,
        start: usize,
        end: usize,
        occupied: bool,
    },
    Era {
        list: List,
        end: usize,
    },
    Paragraph {
        before: Gap,
        after: Gap,
    },
    Bullet {
        before: Gap,
        after: Gap,
    },
    Blurb {
        after: Gap,
    },
}

pub(super) struct Index {
    targets: BTreeMap<ItemId, Target>,
}
impl Index {
    pub(super) fn new(resume: &Resume) -> Self {
        let mut index = Self {
            targets: BTreeMap::new(),
        };
        for (s, section) in resume.sections.iter().enumerate() {
            let owner = Owner::Section(s);
            let end = section.blocks.len();
            index.add(
                &section.annotations,
                Target::Container {
                    owner,
                    start: 0,
                    end,
                    occupied: false,
                },
            );
            for (b, block) in section.blocks.iter().enumerate() {
                match block {
                    Block::Paragraph { annotations, .. } => index.paragraph(owner, b, annotations),
                    Block::Bullets { items } => index.bullets(
                        List {
                            owner,
                            block: b,
                            era: false,
                        },
                        items,
                        0,
                        end,
                    ),
                    Block::Role(role) => index.role(Owner::Role(s, b), role),
                    Block::PageBreak => {}
                }
            }
        }
        index
    }

    pub(super) fn contains(&self, id: &ItemId) -> bool {
        self.targets.contains_key(id)
    }

    fn add(&mut self, annotations: &Annotations, target: Target) {
        let names = annotations.id.as_deref().cloned().into_iter().chain(
            annotations
                .alternate
                .as_ref()
                .map(|a| ItemId::try_new(a.group.as_ref()).expect("group and item names share grammar")),
        );
        for name in names {
            self.targets
                .entry(name)
                .and_modify(|old| match (&mut *old, &target) {
                    (Target::Paragraph { after, .. }, Target::Paragraph { after: next, .. })
                    | (Target::Bullet { after, .. }, Target::Bullet { after: next, .. }) => *after = *next,
                    _ => {}
                })
                .or_insert_with(|| target.clone());
        }
    }

    fn paragraph(&mut self, owner: Owner, block: usize, annotations: &Annotations) {
        self.add(
            annotations,
            Target::Paragraph {
                before: Gap::Blocks(owner, block),
                after: Gap::Blocks(owner, block + 1),
            },
        );
    }

    fn bullets(&mut self, list: List, items: &[Bullet], start: usize, end: usize) {
        let gap = |i| {
            if !list.era && i == 0 && list.block == start {
                Gap::Blocks(list.owner, start)
            } else if !list.era && i == items.len() && list.block + 1 == end {
                Gap::Blocks(list.owner, end)
            } else {
                Gap::Items(list, i)
            }
        };
        for (i, item) in items.iter().enumerate() {
            self.add(
                &item.annotations,
                Target::Bullet {
                    before: gap(i),
                    after: gap(i + 1),
                },
            );
        }
    }

    fn role(&mut self, owner: Owner, role: &Role) {
        let start = blurb_prefix(role);
        let end = role.body.len();
        self.add(
            &role.annotations,
            Target::Container {
                owner,
                start,
                end,
                occupied: role.blurb.is_some(),
            },
        );
        if role.blurb.is_some() {
            self.add(
                &role.blurb_annotations,
                Target::Blurb {
                    after: Gap::Blocks(owner, start),
                },
            );
        }
        for (b, block) in role.body.iter().enumerate() {
            match block {
                RoleBlock::Paragraph { annotations, .. } if b >= start => self.paragraph(owner, b, annotations),
                RoleBlock::Paragraph { .. } => {}
                RoleBlock::Bullets { items } => self.bullets(
                    List {
                        owner,
                        block: b,
                        era: false,
                    },
                    items,
                    start,
                    end,
                ),
                RoleBlock::Era { annotations, items, .. } => {
                    let list = List {
                        owner,
                        block: b,
                        era: true,
                    };
                    self.add(annotations, Target::Era { list, end: items.len() });
                    self.bullets(list, items, 0, 0);
                }
            }
        }
    }

    pub(super) fn bind(
        &self,
        addition: &ResumeAddition,
        ids: &BTreeSet<ItemId>,
        problems: &mut Vec<BindingProblem>,
    ) -> Option<Gap> {
        let (anchor, operation, bullet) = match addition.place() {
            ResumePlace::Blurb { role } => (role, Operation::Blurb, false),
            ResumePlace::Bullet(place) | ResumePlace::Paragraph(place) => {
                let (anchor, op) = match place {
                    LeafPlace::Before(a) => (a, Operation::Before),
                    LeafPlace::After(a) => (a, Operation::After),
                    LeafPlace::Start(a) => (a, Operation::Start),
                    LeafPlace::End(a) => (a, Operation::End),
                };
                (anchor, op, matches!(addition.place(), ResumePlace::Bullet(_)))
            }
        };
        let target = self.targets.get(&anchor.value);
        let reason = if ids.contains(&anchor.value) {
            Some(BindingReason::AdditionTarget)
        } else if target.is_none() {
            Some(BindingReason::MissingReference)
        } else {
            None
        };
        let result = match (reason, target) {
            (Some(reason), _) => Err(reason),
            (_, Some(target)) => place(target, operation, bullet),
            _ => unreachable!(),
        };
        match result {
            Ok(gap) => Some(gap),
            Err(reason) => {
                problems.push(BindingProblem {
                    id: anchor.value.clone(),
                    at: anchor.at.clone(),
                    reason,
                });
                None
            }
        }
    }
}

fn place(target: &Target, operation: Operation, bullet: bool) -> Result<Gap, BindingReason> {
    use Operation::*;
    match (target, operation) {
        (Target::Bullet { before, after }, Before | After) if bullet => {
            Ok(if operation == Before { *before } else { *after })
        }
        (Target::Paragraph { before, after }, Before | After) if !bullet => {
            Ok(if operation == Before { *before } else { *after })
        }
        (Target::Blurb { after }, After) if !bullet => Ok(*after),
        (Target::Container { owner, start, end, .. }, Start | End) => {
            Ok(Gap::Blocks(*owner, if operation == Start { *start } else { *end }))
        }
        (Target::Era { list, end }, Start | End) if bullet => {
            Ok(Gap::Items(*list, if operation == Start { 0 } else { *end }))
        }
        (
            Target::Container {
                owner: owner @ Owner::Role(..),
                occupied,
                ..
            },
            Blurb,
        ) => {
            if *occupied {
                Err(BindingReason::OccupiedBlurb)
            } else {
                Ok(Gap::Blurb(*owner))
            }
        }
        _ => Err(BindingReason::WrongTargetKind),
    }
}

pub(super) fn blurb_prefix(role: &Role) -> usize {
    let Some(group) = role
        .blurb_annotations
        .alternate
        .as_ref()
        .filter(|_| role.blurb.is_some())
        .map(|a| &a.group)
    else {
        return 0;
    };
    role.body.iter().take_while(|block| matches!(block, RoleBlock::Paragraph { annotations, .. } if annotations.alternate.as_ref().is_some_and(|a| &a.group == group))).count()
}
