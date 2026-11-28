//! ABOUTME: Checks the Rust character offsets, whitespace trim and overlapping counts against Python's own `str` results.
//! ABOUTME: `python_parity/generate.py` builds the golden table from Python's own results on synthetic text.

use advisor::text::{CharOffset, IndexedText, occurrences, trimmed_char_count};
use claims::assert_ok;
use pretty_assertions::assert_eq;

/// `(text, python len(text), python len(text.strip()), [(needle, python occurrences)], [(start, end, python text[start:end])])`
type Case = (
    &'static str,
    usize,
    usize,
    &'static [(&'static str, usize)],
    &'static [(usize, usize, &'static str)],
);

include!("python_parity/table.rs");

#[test]
fn rust_agrees_with_python_on_every_golden_case() {
    assert!(!CASES.is_empty());
    for &(text, length, trimmed, needles, slices) in CASES {
        let indexed = IndexedText::new(text);
        assert_eq!(indexed.len(), CharOffset::new(length), "len of {text:?}");
        assert_eq!(trimmed_char_count(text), trimmed, "strip of {text:?}");
        for &(needle, count) in needles {
            assert_eq!(occurrences(text, needle), count, "{needle:?} in {text:?}");
        }
        for &(start, end, expected) in slices {
            let got = assert_ok!(indexed.slice(CharOffset::new(start)..CharOffset::new(end)));
            assert_eq!(got, expected, "{start}..{end} of {text:?}");
        }
    }
}
