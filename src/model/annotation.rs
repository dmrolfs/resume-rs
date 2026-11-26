//! ABOUTME: Tailoring annotations from Markdown comments: priority, status, topic tags, and alternate wordings.
//! ABOUTME: Kept on model nodes for selection but never serialized; names are a stable vocabulary specs reuse.

use std::collections::BTreeSet;

use nutype::nutype;
use strum::{AsRefStr, Display, EnumIter, EnumString, IntoStaticStr};

/// How important an item is, whatever the job: `@core` or `@extra`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display, AsRefStr, IntoStaticStr, EnumString, EnumIter,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Priority {
    Core,
    Extra,
}

/// Whether an item is ready to send: `@draft` or `@verify`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display, AsRefStr, IntoStaticStr, EnumString, EnumIter,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Status {
    Draft,
    Verify,
}

// A name is lowercase ASCII letters and digits, starting with a letter, with single hyphens between them.
// Specs written months earlier must still match, so there is no case folding and no Unicode.

/// A topic tag, written `#ai`.
#[nutype(
    validate(len_char_max = 40, regex = "^[a-z](-?[a-z0-9])*$"),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, AsRef, TryFrom)
)]
pub struct Tag(String);

/// The name of a group of alternate wordings, the `gateway` of `=gateway.short`.
#[nutype(
    validate(len_char_max = 40, regex = "^[a-z](-?[a-z0-9])*$"),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, AsRef, TryFrom)
)]
pub struct GroupName(String);

/// The name of one wording in a group, the `short` of `=gateway.short`.
#[nutype(
    validate(len_char_max = 40, regex = "^[a-z](-?[a-z0-9])*$"),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, AsRef, TryFrom)
)]
pub struct VariantName(String);

impl VariantName {
    /// The variant every group must have; a build that prefers nothing renders it.
    pub const DEFAULT: &'static str = "default";

    /// True for the `default` variant.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.as_ref() == Self::DEFAULT
    }
}

/// The name of one item, the `gateway` of `&gateway`.
///
/// A position spec addresses an item by this name to change its tags or priority for one application. It shares the
/// document's one namespace with group names, so `&gateway` and `=gateway.short` cannot both appear.
#[nutype(
    validate(len_char_max = 40, regex = "^[a-z](-?[a-z0-9])*$"),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, AsRef, TryFrom)
)]
pub struct ItemId(String);

/// One wording of a group: `=group.variant`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Alternate {
    pub group: GroupName,
    pub variant: VariantName,
}

/// What one annotation comment says about the node it marks. Empty for an unannotated node.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Annotations {
    pub priority: Option<Priority>,
    pub status: Option<Status>,
    pub tags: BTreeSet<Tag>,
    pub alternate: Option<Alternate>,
    /// The name a spec addresses this item by; never part of what a style sees. Boxed: most nodes have none, and a
    /// name inline would make every node, and so every `Block::Role`, larger.
    pub id: Option<Box<ItemId>>,
}

impl Annotations {
    /// True when the node carries no annotation.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    #[test]
    fn names_are_lowercase_letters_digits_and_single_hyphens_starting_with_a_letter() {
        for good in ["ai", "a", "llm2", "ai-platform", "c-suite-2", "x1-y2"] {
            assert_ok!(Tag::try_new(good), "{good}");
            assert_ok!(GroupName::try_new(good), "{good}");
            assert_ok!(VariantName::try_new(good), "{good}");
        }
        for bad in [
            "", "AI", "Ai", "2ai", "-ai", "ai-", "ai--ml", "ai_ml", "ai.ml", "ai ml", "café", "#ai",
        ] {
            assert_err!(Tag::try_new(bad), "{bad:?}");
            assert_err!(GroupName::try_new(bad), "{bad:?}");
            assert_err!(VariantName::try_new(bad), "{bad:?}");
        }
    }

    #[test]
    fn names_are_at_most_forty_characters() {
        assert_ok!(Tag::try_new("a".repeat(40)));
        assert_err!(Tag::try_new("a".repeat(41)));
    }

    #[test]
    fn keywords_are_the_closed_kebab_case_set() {
        assert_eq!(Priority::from_str("core"), Ok(Priority::Core));
        assert_eq!(Priority::from_str("extra"), Ok(Priority::Extra));
        assert_eq!(Status::from_str("draft"), Ok(Status::Draft));
        assert_eq!(Status::from_str("verify"), Ok(Status::Verify));
        for bad in ["Core", "CORE", "cor", "drafts", ""] {
            assert_err!(Priority::from_str(bad), "{bad:?}");
            assert_err!(Status::from_str(bad), "{bad:?}");
        }
        assert_eq!(Priority::Core.as_ref(), "core");
        assert_eq!(Status::Verify.to_string(), "verify");
    }

    #[test]
    fn only_the_variant_named_default_is_the_default() {
        assert!(assert_ok!(VariantName::try_new("default")).is_default());
        assert!(!assert_ok!(VariantName::try_new("short")).is_default());
        assert!(!assert_ok!(VariantName::try_new("default-2")).is_default());
    }

    #[test]
    fn annotations_are_empty_until_something_is_set() {
        assert!(Annotations::default().is_empty());
        let tagged = Annotations {
            tags: BTreeSet::from([assert_ok!(Tag::try_new("ai"))]),
            ..Annotations::default()
        };
        assert!(!tagged.is_empty());
    }
}
