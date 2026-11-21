//! ABOUTME: Reads the key paths of a style's `theme.typ`, so the guide's theme reference can be compared with them.
//! ABOUTME: A small reader of Typst dictionaries, not a Typst evaluator: themes are literal nested dictionaries.

use std::collections::BTreeSet;

use claims::assert_ok;
use regex::Regex;

/// Removes comments, string contents, and `[...]` content blocks, which hold characters that look like syntax.
///
/// A string that is a dictionary key (`"a-b": 1`) keeps its text, because the key is what is read.
/// Block comments nest, as in Typst, and a backslash escapes the next character in a string or a content block.
fn strip(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                let mut depth = 0;
                while i < chars.len() {
                    if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                        depth += 1;
                        i += 2;
                    } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
            }
            '"' => {
                let start = i + 1;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    i += if chars[i] == '\\' { 2 } else { 1 };
                }
                let content: String = chars[start..i.min(chars.len())].iter().collect();
                i += 1;
                let is_key = chars[i.min(chars.len())..]
                    .iter()
                    .find(|c| !c.is_whitespace())
                    .is_some_and(|c| *c == ':');
                out.push('"');
                if is_key {
                    out.push_str(&content);
                }
                out.push('"');
            }
            '[' => {
                let mut depth = 0;
                while i < chars.len() {
                    match chars[i] {
                        '\\' => i += 1,
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                out.push_str("[]");
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

struct Reader {
    chars: Vec<char>,
    at: usize,
    keys: Vec<String>,
}

impl Reader {
    fn skip_space(&mut self) {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
    }

    /// True when the `(` at `self.at` opens a dictionary (`(a: 1)`, `(:)`) and not an array or a parenthesized
    /// expression (`(1pt, 2pt)`, `(1pt + 2pt)`): a dictionary has a `:` at its own level before its first `,`.
    fn opens_dictionary(&self) -> bool {
        let mut depth = 0;
        for c in &self.chars[self.at..] {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return false;
                    }
                }
                ',' if depth == 1 => return false,
                ':' if depth == 1 => return true,
                _ => {}
            }
        }
        false
    }

    /// Reads one entry's key, up to its `:`, or up to the `,` or `)` that ends an entry without one (a spread).
    fn key(&mut self) -> String {
        let start = self.at;
        let mut depth = 0;
        while let Some(c) = self.chars.get(self.at) {
            match c {
                '(' => depth += 1,
                ')' if depth == 0 => break,
                ')' => depth -= 1,
                ':' | ',' if depth == 0 => break,
                _ => {}
            }
            self.at += 1;
        }
        let raw: String = self.chars[start..self.at].iter().collect();
        raw.trim().trim_matches('"').to_string()
    }

    /// Reads a dictionary that starts at `(`, recording the dotted path of every leaf.
    fn dictionary(&mut self, prefix: &str) {
        self.at += 1; // (
        loop {
            self.skip_space();
            match self.chars.get(self.at) {
                None => return,
                Some(')') => {
                    self.at += 1;
                    return;
                }
                Some(',') => self.at += 1,
                Some(_) => {
                    let key = self.key();
                    if self.chars.get(self.at) != Some(&':') {
                        continue; // a spread such as `..base`
                    }
                    self.at += 1; // :
                    self.skip_space();
                    let path = if prefix.is_empty() {
                        key
                    } else {
                        format!("{prefix}.{key}")
                    };
                    if self.chars.get(self.at) == Some(&'(') && self.opens_dictionary() {
                        self.dictionary(&path);
                    } else {
                        self.keys.push(path);
                        self.value();
                    }
                }
            }
        }
    }

    /// Skips an expression such as `35%`, `rgb("")`, `(1pt + 2pt)`, or `[]`, up to the `,` or `)` that ends it.
    fn value(&mut self) {
        let mut depth = 0;
        while let Some(c) = self.chars.get(self.at) {
            match c {
                '(' => depth += 1,
                ')' if depth == 0 => return,
                ')' => depth -= 1,
                ',' if depth == 0 => return,
                _ => {}
            }
            self.at += 1;
        }
    }
}

/// The dotted key path of every value in the `theme` dictionary of a `theme.typ`, in file order.
///
/// Only `#let theme = (...)` is read: another binding such as `#let theme-dark = (...)` is not the theme.
pub fn theme_keys(src: &str) -> Vec<String> {
    let text = strip(src);
    let binding = assert_ok!(Regex::new(r"#let theme\s*=\s*\("));
    let Some(found) = binding.find(&text) else {
        return Vec::new();
    };
    let mut reader = Reader {
        chars: text[found.end() - 1..].chars().collect(),
        at: 0,
        keys: Vec::new(),
    };
    reader.dictionary("");
    reader.keys
}

/// The font role names (`body`, `display`, `mono`), from the `fonts.<role>.<field>` keys.
pub fn font_roles(keys: &[String]) -> BTreeSet<String> {
    keys.iter()
        .filter_map(|k| k.strip_prefix("fonts."))
        .filter_map(|rest| rest.split('.').next())
        .map(str::to_string)
        .collect()
}

/// The keys as the guide writes them: `fonts.body.preferred` is `fonts.<role>.preferred`.
pub fn normalized(keys: &[String]) -> BTreeSet<String> {
    keys.iter()
        .map(
            |key| match key.strip_prefix("fonts.").and_then(|rest| rest.split_once('.')) {
                Some((_, field)) => format!("fonts.<role>.{field}"),
                None => key.clone(),
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every syntax form the real themes use.
    const SNIPPET: &str = r##"
// A comment with (parentheses): and a colon
#let theme = (
  fonts: (
    body: (preferred: "Calibri, (the template)", fallback: "Carlito", metric-compatible: true),
    mono: (preferred: "DejaVu Sans Mono", fallback: "DejaVu Sans Mono", metric-compatible: true),
  ),
  size: (body: 11pt, name: 24pt), /* block (comment): x */
  color: (text: rgb("#000000"), accent: rgb("#1155cc")),
  footer-descent: 35%,
  line: (top: 0.952em, bottom: -0.269em),
  bullet: (marker: [•], indent: 0.15in),
  header: (gap: 0pt),
)
"##;

    #[test]
    fn every_leaf_has_its_dotted_path_whatever_it_contains() {
        let keys = theme_keys(SNIPPET);
        assert_eq!(
            keys,
            [
                "fonts.body.preferred",
                "fonts.body.fallback",
                "fonts.body.metric-compatible",
                "fonts.mono.preferred",
                "fonts.mono.fallback",
                "fonts.mono.metric-compatible",
                "size.body",
                "size.name",
                "color.text",
                "color.accent",
                "footer-descent",
                "line.top",
                "line.bottom",
                "bullet.marker",
                "bullet.indent",
                "header.gap",
            ]
        );
    }

    #[test]
    fn font_roles_are_normalized_and_listed_once() {
        let keys = theme_keys(SNIPPET);
        assert_eq!(
            font_roles(&keys),
            BTreeSet::from(["body".to_string(), "mono".to_string()])
        );
        let normal = normalized(&keys);
        assert!(normal.contains("fonts.<role>.preferred") && !normal.contains("fonts.body.preferred"));
        assert!(normal.contains("size.body"));
    }

    #[test]
    fn a_new_nested_key_is_found() {
        let keys = theme_keys("#let theme = (spacing: (section: 1pt, deep: (a: 1, b: 2)), x: 3)");
        assert_eq!(keys, ["spacing.section", "spacing.deep.a", "spacing.deep.b", "x"]);
    }

    #[test]
    fn nothing_is_found_without_a_theme_dictionary() {
        assert!(theme_keys("#let other = (a: 1)").is_empty());
    }

    #[test]
    fn a_parenthesized_value_and_an_array_are_leaves_not_dictionaries() {
        let keys = theme_keys("#let theme = (a: (1pt + 2pt), b: (1pt, 2pt), c: (), d: (e: 1))");
        assert_eq!(keys, ["a", "b", "c", "d.e"]);
    }

    #[test]
    fn a_spread_is_skipped_and_the_keys_around_it_are_kept() {
        let keys = theme_keys("#let theme = (a: 1, ..base, b: 2, ..other(x, y), c: 3)");
        assert_eq!(keys, ["a", "b", "c"]);
    }

    #[test]
    fn a_quoted_key_is_read_and_a_quoted_value_is_not() {
        let keys = theme_keys(r#"#let theme = ("a-b": 1, c: "d: (e, f)", g: 2)"#);
        assert_eq!(keys, ["a-b", "c", "g"]);
    }

    #[test]
    fn block_comments_nest_and_an_escaped_bracket_does_not_end_a_content_block() {
        let keys = theme_keys("#let theme = (a: 1, /* x /* y: 2, */ z: 3, */ b: [\\[], c: 3)");
        assert_eq!(keys, ["a", "b", "c"]);
    }

    #[test]
    fn only_the_binding_named_theme_is_the_theme() {
        let keys = theme_keys("#let theme-dark = (x: 1)\n#let theme = (y: 2)\n");
        assert_eq!(keys, ["y"]);
    }
}
