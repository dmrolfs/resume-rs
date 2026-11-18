//! ABOUTME: Validated text newtypes for resume fields (name, company, title, headings, links, style names).
//! ABOUTME: Construction sanitizes and validates, so downstream code never re-checks these invariants.

use nutype::nutype;

/// The person's name, from the single `# ` heading.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 80),
    derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Serialize, TryFrom)
)]
pub struct Name(String);

/// The company, the first part of a role line.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 120),
    derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Serialize, TryFrom)
)]
pub struct Company(String);

/// The job title, the middle part of a three-part role line.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 120),
    derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Serialize, TryFrom)
)]
pub struct Title(String);

/// A section heading, from a `## ` line.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 60),
    derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Serialize, TryFrom)
)]
pub struct SectionTitle(String);

/// The name of an era inside a role, from a `**Era · Period**` paragraph.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 80),
    derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Serialize, TryFrom)
)]
pub struct EraName(String);

/// A link target: parsed with `url` (WHATWG), restricted to `http`, `https`, or `mailto`.
#[nutype(
    validate(predicate = |u: &url::Url| matches!(u.scheme(), "http" | "https" | "mailto")),
    derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Serialize, TryFrom)
)]
pub struct LinkUrl(url::Url);

/// A built-in or `--styles-dir` style directory name.
#[nutype(
    sanitize(trim, lowercase),
    validate(regex = "^[a-z][a-z0-9-]{0,31}$"),
    derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Serialize, TryFrom, FromStr)
)]
pub struct StyleName(String);

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;

    #[test]
    fn newtypes_trim_and_reject_empty() {
        assert_eq!(
            Name::try_new("  Ada Lovelace ").map(|n| n.into_inner()),
            Ok("Ada Lovelace".to_string())
        );
        assert_eq!(
            Company::try_new(" Initech\t").map(Company::into_inner),
            Ok("Initech".to_string())
        );
        assert_eq!(Title::try_new(" VP ").map(Title::into_inner), Ok("VP".to_string()));
        assert_eq!(
            SectionTitle::try_new(" Experience ").map(SectionTitle::into_inner),
            Ok("Experience".to_string())
        );
        assert_eq!(
            EraName::try_new(" Early ").map(EraName::into_inner),
            Ok("Early".to_string())
        );

        assert_err!(Name::try_new("   "));
        assert_err!(Company::try_new(""));
        assert_err!(Title::try_new(" \n"));
        assert_err!(SectionTitle::try_new(""));
        assert_err!(EraName::try_new("  "));
    }

    #[test]
    fn newtypes_reject_over_length() {
        assert_err!(Name::try_new("a".repeat(81)));
        assert_ok!(Name::try_new("a".repeat(80)));
        assert_err!(SectionTitle::try_new("a".repeat(61)));
        assert_err!(Company::try_new("a".repeat(121)));
    }

    #[test]
    fn link_url_accepts_supported_schemes() {
        for s in ["http://example.com", "https://example.com/a?b=1", "mailto:a@b.co"] {
            let url = assert_ok!(url::Url::parse(s));
            assert_ok!(LinkUrl::try_new(url));
        }
    }

    #[test]
    fn link_url_rejects_unsupported_scheme() {
        for s in ["ftp://example.com", "file:///etc/passwd", "javascript:alert(1)"] {
            let url = assert_ok!(url::Url::parse(s));
            assert_err!(LinkUrl::try_new(url));
        }
    }

    #[test]
    fn link_url_serializes_as_string() {
        let url = assert_ok!(url::Url::parse("https://example.com/x"));
        let link = assert_ok!(LinkUrl::try_new(url));
        assert_eq!(assert_ok!(serde_json::to_string(&link)), r#""https://example.com/x""#);
    }

    #[test]
    fn style_name_normalizes_and_validates() {
        assert_eq!(assert_ok!(StyleName::try_new(" Pragmatic ")).into_inner(), "pragmatic");
        assert_eq!(assert_ok!("mono-2".parse::<StyleName>()).as_ref(), "mono-2");
        assert_err!(StyleName::try_new("1abc"));
        assert_err!(StyleName::try_new("has space"));
        assert_err!(StyleName::try_new("a".repeat(33)));
        assert_err!(StyleName::try_new(""));
    }
}
