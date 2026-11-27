use super::{Gap, List, Owner};
use crate::diagnostics::ItemOrigin;
use crate::markdown::fragment::ResumeFragment;
use crate::model::{Annotations, Block, Bullet, Inline, Resume, Role, RoleBlock};
use crate::pipeline::PreparedResume;
use crate::spec::additions::ResumeAddition;
use std::collections::BTreeMap;

pub(super) fn assemble(
    resume: Resume,
    baseline: &[ItemOrigin],
    additions: &[ResumeAddition],
    gaps: BTreeMap<Gap, Vec<usize>>,
) -> PreparedResume {
    let mut emitter = Emitter {
        baseline: baseline.iter(),
        origins: Vec::new(),
        additions,
        gaps,
    };
    let mut resume = resume;
    for (s, section) in resume.sections.iter_mut().enumerate() {
        emitter.original();
        section.blocks = emitter.blocks(std::mem::take(&mut section.blocks), Owner::Section(s), 0);
    }
    assert!(emitter.baseline.next().is_none(), "all baseline origins consumed");
    assert!(emitter.gaps.is_empty(), "all bound insertion gaps emitted");
    PreparedResume::new(resume, emitter.origins.into())
}

struct Emitter<'a> {
    baseline: std::slice::Iter<'a, ItemOrigin>,
    origins: Vec<ItemOrigin>,
    additions: &'a [ResumeAddition],
    gaps: BTreeMap<Gap, Vec<usize>>,
}

trait BodyBlock: Sized {
    fn bullets(items: Vec<Bullet>) -> Self;
    fn paragraph(content: Vec<Inline>, annotations: Annotations) -> Self;
    fn items_mut(&mut self) -> Option<&mut Vec<Bullet>>;
    fn emit(self, owner: Owner, block: usize, emitter: &mut Emitter<'_>) -> Self;
}

impl BodyBlock for Block {
    fn bullets(items: Vec<Bullet>) -> Self {
        Self::Bullets { items }
    }
    fn paragraph(content: Vec<Inline>, annotations: Annotations) -> Self {
        Self::Paragraph { content, annotations }
    }
    fn items_mut(&mut self) -> Option<&mut Vec<Bullet>> {
        match self {
            Self::Bullets { items } => Some(items),
            _ => None,
        }
    }
    fn emit(self, owner: Owner, block: usize, emitter: &mut Emitter<'_>) -> Self {
        match self {
            Self::Paragraph { .. } => {
                emitter.original();
                self
            }
            Self::Bullets { items } => Self::Bullets {
                items: emitter.items(
                    items,
                    List {
                        owner,
                        block,
                        era: false,
                    },
                ),
            },
            Self::Role(role) => {
                let Owner::Section(section) = owner else { unreachable!() };
                Self::Role(emitter.role(role, Owner::Role(section, block)))
            }
            Self::PageBreak => self,
        }
    }
}
impl BodyBlock for RoleBlock {
    fn bullets(items: Vec<Bullet>) -> Self {
        Self::Bullets { items }
    }
    fn paragraph(content: Vec<Inline>, annotations: Annotations) -> Self {
        Self::Paragraph { content, annotations }
    }
    fn items_mut(&mut self) -> Option<&mut Vec<Bullet>> {
        match self {
            Self::Bullets { items } => Some(items),
            _ => None,
        }
    }
    fn emit(self, owner: Owner, block: usize, emitter: &mut Emitter<'_>) -> Self {
        match self {
            Self::Paragraph { .. } => {
                emitter.original();
                self
            }
            Self::Bullets { items } => Self::Bullets {
                items: emitter.items(
                    items,
                    List {
                        owner,
                        block,
                        era: false,
                    },
                ),
            },
            Self::Era {
                name,
                period,
                items,
                annotations,
            } => {
                emitter.original();
                Self::Era {
                    name,
                    period,
                    annotations,
                    items: emitter.items(
                        items,
                        List {
                            owner,
                            block,
                            era: true,
                        },
                    ),
                }
            }
        }
    }
}

impl Emitter<'_> {
    fn original(&mut self) {
        self.origins
            .push(self.baseline.next().expect("one origin per baseline node").clone());
    }
    fn added(&mut self, index: usize) -> (ResumeFragment, Annotations) {
        let addition = &self.additions[index];
        self.origins.push(addition.content().origin().clone());
        (
            addition.content().value().clone(),
            addition.metadata().annotations(&addition.id().value),
        )
    }
    fn role(&mut self, mut role: Role, owner: Owner) -> Role {
        self.original();
        let start = super::bind::blurb_prefix(&role);
        if role.blurb.is_some() {
            self.original();
        }
        if let Some(indices) = self.gaps.remove(&Gap::Blurb(owner)) {
            let (ResumeFragment::Blurb(content), annotations) = self.added(indices[0]) else {
                unreachable!()
            };
            role.blurb = Some(content);
            role.blurb_annotations = annotations;
        }
        role.body = self.blocks(role.body, owner, start);
        role
    }
    fn items(&mut self, items: Vec<Bullet>, list: List) -> Vec<Bullet> {
        let mut output = Vec::new();
        for (i, next) in items.into_iter().map(Some).chain(std::iter::once(None)).enumerate() {
            if let Some(additions) = self.gaps.remove(&Gap::Items(list, i)) {
                for index in additions {
                    let (ResumeFragment::Bullet { label, content }, annotations) = self.added(index) else {
                        unreachable!()
                    };
                    output.push(Bullet {
                        label,
                        content,
                        annotations,
                    });
                }
            }
            if let Some(item) = next {
                self.original();
                output.push(item);
            }
        }
        output
    }
    fn blocks<B: BodyBlock>(&mut self, blocks: Vec<B>, owner: Owner, start: usize) -> Vec<B> {
        let end = blocks.len();
        let mut output = Vec::new();
        for (i, next) in blocks.into_iter().map(Some).chain(std::iter::once(None)).enumerate() {
            if let Some(additions) = self.gaps.remove(&Gap::Blocks(owner, i)) {
                // Only the end edge may append directly to a pre-existing baseline list.
                let mut merge = i == end && i > start;
                for index in additions {
                    let (fragment, annotations) = self.added(index);
                    match fragment {
                        ResumeFragment::Bullet { label, content } => {
                            push_bullets(
                                &mut output,
                                vec![Bullet {
                                    label,
                                    content,
                                    annotations,
                                }],
                                merge,
                            );
                            merge = true;
                        }
                        ResumeFragment::Paragraph(content) => {
                            output.push(B::paragraph(content, annotations));
                            merge = false;
                        }
                        ResumeFragment::Blurb(_) => unreachable!(),
                    }
                }
            }
            if let Some(block) = next {
                let mut block = block.emit(owner, i, self);
                // At start, the last new bullet run belongs to the adjacent edge list.
                if i == start
                    && let Some(items) = block.items_mut()
                {
                    push_bullets(&mut output, std::mem::take(items), true);
                } else {
                    output.push(block);
                }
            }
        }
        output
    }
}

fn push_bullets<B: BodyBlock>(blocks: &mut Vec<B>, items: Vec<Bullet>, merge: bool) {
    if merge && let Some(last) = blocks.last_mut().and_then(BodyBlock::items_mut) {
        last.extend(items);
    } else {
        blocks.push(B::bullets(items));
    }
}
