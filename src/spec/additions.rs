//! ABOUTME: Typed, leaf-only addition declarations; baseline binding checks anchors and namespaces later.
//! ABOUTME: Placement determines resume kind, and metadata cannot create alternate groups or letter frames.

use std::collections::BTreeSet;
use std::ops::Range;

use super::Located;
use super::content::{ContentError, SpecContent};
use super::layer::Sourced;
use crate::diagnostics::SourceText;
use crate::letter::LetterInline;
use crate::markdown::fragment::{ResumeFragment, ResumeFragmentKind};
use crate::model::{Annotations, ItemId, Priority, Status, Tag};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdditionMetadata {
    pub tags: Option<Sourced<BTreeSet<Tag>>>,
    pub priority: Option<Sourced<Priority>>,
    pub status: Option<Sourced<Status>>,
}

impl AdditionMetadata {
    #[must_use]
    pub fn annotations(&self, id: &ItemId) -> Annotations {
        Annotations {
            id: Some(Box::new(id.clone())),
            tags: self.tags.as_ref().map(|s| s.value.clone()).unwrap_or_default(),
            priority: self.priority.as_ref().map(|s| s.value),
            status: self.status.as_ref().map(|s| s.value),
            alternate: None,
        }
    }
}

/// References are baseline item or group names, never references to another addition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeafPlace {
    Before(Sourced<ItemId>),
    After(Sourced<ItemId>),
    Start(Sourced<ItemId>),
    End(Sourced<ItemId>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumePlace {
    Bullet(LeafPlace),
    Paragraph(LeafPlace),
    Blurb { role: Sourced<ItemId> },
}

impl ResumePlace {
    #[must_use]
    pub fn kind(&self) -> ResumeFragmentKind {
        match self {
            Self::Bullet(_) => ResumeFragmentKind::Bullet,
            Self::Paragraph(_) => ResumeFragmentKind::Paragraph,
            Self::Blurb { .. } => ResumeFragmentKind::Blurb,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LetterPlace {
    Before(Sourced<ItemId>),
    After(Sourced<ItemId>),
    Start(Located),
    End(Located),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeAddition {
    id: Sourced<ItemId>,
    place: ResumePlace,
    metadata: AdditionMetadata,
    content: SpecContent<ResumeFragment>,
}

impl ResumeAddition {
    pub fn new(
        id: Sourced<ItemId>,
        place: ResumePlace,
        metadata: AdditionMetadata,
        source: &SourceText,
        literal: Range<usize>,
    ) -> Result<Self, ContentError> {
        let content = SpecContent::resume(source, place.kind(), literal)?;
        Ok(Self {
            id,
            place,
            metadata,
            content,
        })
    }
    #[must_use]
    pub fn id(&self) -> &Sourced<ItemId> {
        &self.id
    }
    #[must_use]
    pub fn place(&self) -> &ResumePlace {
        &self.place
    }
    #[must_use]
    pub fn metadata(&self) -> &AdditionMetadata {
        &self.metadata
    }
    #[must_use]
    pub fn content(&self) -> &SpecContent<ResumeFragment> {
        &self.content
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetterAddition {
    id: Sourced<ItemId>,
    place: LetterPlace,
    metadata: AdditionMetadata,
    content: SpecContent<Vec<LetterInline>>,
}

impl LetterAddition {
    pub fn new(
        id: Sourced<ItemId>,
        place: LetterPlace,
        metadata: AdditionMetadata,
        source: &SourceText,
        literal: Range<usize>,
    ) -> Result<Self, ContentError> {
        let content = SpecContent::letter(source, literal)?;
        Ok(Self {
            id,
            place,
            metadata,
            content,
        })
    }
    #[must_use]
    pub fn id(&self) -> &Sourced<ItemId> {
        &self.id
    }
    #[must_use]
    pub fn place(&self) -> &LetterPlace {
        &self.place
    }
    #[must_use]
    pub fn metadata(&self) -> &AdditionMetadata {
        &self.metadata
    }
    #[must_use]
    pub fn content(&self) -> &SpecContent<Vec<LetterInline>> {
        &self.content
    }
}
