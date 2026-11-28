//! Typst's innermost syntax tags as non-overlapping UTF-8 byte ranges.
use std::ops::Range;
use typst_syntax::Tag;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub range: Range<usize>,
    pub tag: Tag,
}

pub(crate) fn tokens(text: &str) -> Vec<Token> {
    let root = typst_syntax::parse(text);
    let mut pending = vec![(typst_syntax::LinkedNode::new(&root), None)];
    let mut found: Vec<Token> = Vec::new();
    while let Some((node, inherited)) = pending.pop() {
        let tag = typst_syntax::highlight(&node).or(inherited);
        if node.children().len() != 0 {
            pending.extend(node.children().rev().map(|child| (child, tag)));
        } else if let Some(tag) = tag {
            let range = node.range();
            if range.is_empty() {
                continue;
            }
            if let Some(last) = found.last_mut()
                && last.tag == tag
                && last.range.end == range.start
            {
                last.range.end = range.end;
            } else {
                found.push(Token { range, tag });
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{Token, tokens};
    use claims::{assert_ok, assert_some};
    use pretty_assertions::assert_eq;
    use typst_syntax::Tag;

    fn check_ranges(text: &str, tokens: &[Token]) {
        let mut end = 0;
        for token in tokens {
            assert!(token.range.start >= end);
            assert!(token.range.start < token.range.end);
            assert!(text.get(token.range.clone()).is_some());
            end = token.range.end;
        }
    }

    #[test]
    fn test_known_tokens_have_exact_source_ranges() {
        let text = "// note\r\n#import \"/lib/components.typ\": *\n#let f(x) = x + 42\n";
        let found = tokens(text);
        check_ranges(text, &found);
        for (needle, tag) in [
            ("// note", Tag::Comment),
            ("import", Tag::Keyword),
            ("\"/lib/components.typ\"", Tag::String),
            ("f", Tag::Function),
            ("42", Tag::Number),
        ] {
            let start = assert_some!(text.find(needle));
            assert!(
                found.iter().any(|t| t.tag == tag && t.range.contains(&start)),
                "{needle}: {found:?}"
            );
        }
    }

    #[test]
    fn test_inner_tags_override_markup_and_adjacent_leaves_merge() {
        let text = "*bold #f(x)*\n= Title #v\n";
        let found = tokens(text);
        let f = assert_some!(text.find("f(x)"));
        let v = assert_some!(text.find("#v")) + 1;
        assert_eq!(
            assert_some!(found.iter().find(|t| t.range.contains(&f))).tag,
            Tag::Function
        );
        assert_eq!(
            assert_some!(found.iter().find(|t| t.range.contains(&v))).tag,
            Tag::Interpolated
        );
        assert!(
            found
                .windows(2)
                .all(|p| p[0].tag != p[1].tag || p[0].range.end != p[1].range.start)
        );
        check_ranges(text, &found);
    }

    #[test]
    fn test_all_builtin_typst_files_have_valid_sorted_disjoint_ranges() {
        for style in assert_ok!(resume::style::catalog(None)) {
            let name = assert_ok!(resume::model::StyleName::try_new(style.name));
            for (path, bytes) in assert_ok!(resume::style::builtin_files(&name)) {
                if path.extension().is_some_and(|ext| ext == "typ") {
                    let text = assert_ok!(std::str::from_utf8(bytes));
                    let found = tokens(text);
                    assert!(!found.is_empty(), "{}", path.display());
                    check_ranges(text, &found);
                }
            }
        }
    }

    #[test]
    fn test_empty_unicode_and_malformed_text_remain_safe() {
        assert!(tokens("").is_empty());
        for text in ["= Café 🦀\r\n#let λ = \"é\"", "#let x = (", "*é #f(\"🦀\")*", "#"] {
            check_ranges(text, &tokens(text));
        }
    }
}
