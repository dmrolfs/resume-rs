//! Synthetic adversarial JSON and wire-presence checks.
use super::*;
use crate::contract::JsonValue;
use claims::{assert_err, assert_ok};
use pretty_assertions::assert_eq;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const NUMBER_KEY: &str = "$serde_json::private::Number";
const RAW_KEY: &str = "$serde_json::private::RawValue";

fn keep(json: &str) -> JsonValue {
    assert_ok!(decode::<JsonValue>(json.as_bytes()), "{json}")
}

#[test]
fn duplicate_names_are_refused_at_every_depth_even_when_escaped() {
    for raw in [
        r#"{"a":1,"a":2}"#,
        r#"{"a":1,"\u0061":2}"#,
        r#"{"usage":{"detail":{"tokens":1,"tokens":2}}}"#,
        r#"[{"x":null,"x":false}]"#,
        r#"{"extension":[{"a":[],"a":{}}]}"#,
    ] {
        assert_err!(decode::<JsonValue>(raw.as_bytes()), "{raw}");
    }
    keep(r#"{"a":"{\"b\":1,\"b\":2}"}"#);
}

#[test]
fn numbers_are_kept_exactly_whatever_their_length_or_spelling() {
    let long = "9".repeat(1000);
    for number in [
        "0",
        "-0",
        "0.1",
        "1.00",
        "1e20",
        "-1.5E+3",
        "9007199254740993",
        "-9007199254740993",
        "18446744073709551615",
        "18446744073709551616",
        "-9223372036854775809",
        long.as_str(),
    ] {
        let wrapped = format!("{{\"n\":[{number}]}}");
        assert_eq!(keep(&wrapped).as_json(), wrapped, "{number}");
        assert_eq!(assert_ok!(serde_json::to_string(&keep(&wrapped))), wrapped);
    }
}

#[test]
fn a_typed_field_that_cannot_hold_an_integer_fails_instead_of_rounding() {
    #[derive(Debug, Deserialize)]
    struct Small {
        #[allow(dead_code)]
        n: u64,
    }
    assert_ok!(decode::<Small>(br#"{"n":18446744073709551615}"#));
    for raw in [r#"{"n":18446744073709551616}"#, r#"{"n":1.5}"#, r#"{"n":-1}"#] {
        assert_err!(decode::<Small>(raw.as_bytes()), "{raw}");
    }
}

#[test]
fn objects_keyed_like_serde_internals_stay_ordinary_objects() {
    for key in [NUMBER_KEY, RAW_KEY] {
        for raw in [
            format!(r#"{{"{key}":"100"}}"#),
            format!(r#"{{"{key}":"100","extra":true}}"#),
            format!(r#"{{"extra":true,"{key}":{{"nested":[1,2]}}}}"#),
            format!(r#"[{{"{key}":null}}]"#),
        ] {
            assert_eq!(keep(&raw).as_json(), raw);
        }
        assert_err!(decode::<JsonValue>(
            format!(r#"{{"{key}":"1","{key}":"2"}}"#).as_bytes()
        ));
    }
}

#[test]
fn serde_json_value_is_not_safe_for_those_objects() {
    // Why `decode::<serde_json::Value>` is not used for artifacts: with `raw_value` on, a real object is read as a
    // raw value, so it is not an object any more.
    let value: Value = assert_ok!(serde_json::from_str(&format!(r#"{{"{RAW_KEY}":"100"}}"#)));
    assert!(!value.is_object());
}

#[test]
fn malformed_nonfinite_trailing_deep_and_non_unicode_inputs_are_refused() {
    for raw in [
        "NaN",
        "Infinity",
        "-Infinity",
        "1e999",
        "-1e999",
        "[1e999]",
        r#"{"ignored":{"deep":[1e999]}}"#,
        "{} []",
        "{",
        "",
        "01",
        "1.",
        ".5",
        "+1",
        r#""\ud800""#,
        r#""\udfff""#,
        r#"{"k\ud800":1}"#,
    ] {
        assert_err!(decode::<JsonValue>(raw.as_bytes()), "{raw:?}");
    }
    assert_err!(decode::<JsonValue>(&[b'"', 0xff, b'"']));
    assert_ok!(decode::<JsonValue>(br#""\ud83d\ude00""#));
    assert_ok!(decode::<JsonValue>(b"[1e-999]"));
}

#[test]
fn nesting_is_limited_at_the_same_depth_whatever_the_container_kind() {
    let nest = |n: usize, open: &str, close: &str| format!("{}0{}", open.repeat(n), close.repeat(n));
    assert_ok!(decode::<JsonValue>(nest(127, "[", "]").as_bytes()));
    assert_err!(decode::<JsonValue>(nest(128, "[", "]").as_bytes()));
    let objects = |n: usize| format!("{}0{}", r#"{"a":"#.repeat(n), "}".repeat(n));
    assert_ok!(decode::<JsonValue>(objects(127).as_bytes()));
    assert_err!(decode::<JsonValue>(objects(128).as_bytes()));
    assert_err!(decode::<JsonValue>(nest(100_000, "[", "]").as_bytes()));
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct PresenceSample {
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    offset: Field<Option<crate::text::CharOffset>>,
}

#[test]
fn missing_null_and_value_are_distinct_and_round_trip() {
    for (raw, expected) in [
        ("{}", Field::Missing),
        (r#"{"offset":null}"#, Field::Present(None)),
        (
            r#"{"offset":12}"#,
            Field::Present(Some(crate::text::CharOffset::new(12))),
        ),
    ] {
        let value: PresenceSample = assert_ok!(decode(raw.as_bytes()));
        assert_eq!(value.offset, expected);
        assert_eq!(assert_ok!(serde_json::to_string(&value)), raw);
    }
}

#[test]
fn decoder_display_never_shows_keys_values_or_input_text() {
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    for raw in [
        br#"{"private-name":1}"#.as_slice(),
        br#"{"private-name":1,"private-name":2}"#,
    ] {
        let error = assert_err!(decode::<Empty>(raw));
        assert!(!error.to_string().contains("private-name"));
    }
}

#[test]
fn a_large_document_is_checked_within_a_generous_deadline() {
    // About 1 MiB of members. This is a smoke test for a gross slowdown, not a proof of linear time: the repeat check
    // is a hash set per object, which is what keeps it linear.
    let members: Vec<String> = (0..24_000)
        .map(|i| format!(r#""k{i}":{{"a":[1,2,3],"b":"{}"}}"#, "x".repeat(20)))
        .collect();
    let document = format!("{{{}}}", members.join(","));
    assert!(document.len() > 1_000_000);
    let started = std::time::Instant::now();
    assert_ok!(decode::<JsonValue>(document.as_bytes()));
    assert!(started.elapsed().as_secs() < 20, "took {:?}", started.elapsed());
}

#[test]
fn a_finite_decimal_passes_and_an_overflowing_one_fails_even_with_siblings() {
    // `1e999` reads as infinity; `1e-999` reads as zero and is finite. Each sits among values that pass.
    assert_ok!(decode::<JsonValue>(br#"[1.5,1e-999,-2.5e3,1E2]"#));
    for raw in [r#"[1.5,1e999]"#, r#"{"a":1.5,"b":-1e999}"#, r#"[[1e400]]"#] {
        assert_err!(decode::<JsonValue>(raw.as_bytes()), "{raw}");
    }
}

#[test]
fn a_decode_error_names_its_category_and_the_document_is_never_quoted() {
    for (raw, shown) in [
        (&b"\xff"[..], "not valid UTF-8"),
        (br#"{"a":1,"a":2}"#, "strictly decodable"),
        (b"[] []", "after its first JSON value"),
    ] {
        let message = assert_err!(decode::<JsonValue>(raw)).to_string();
        assert!(message.contains(shown), "{message}");
    }
    let shape = assert_err!(decode::<Vec<u8>>(br#"{"secret-text":1}"#)).to_string();
    assert!(
        shape.contains("expected shape") && !shape.contains("secret-text"),
        "{shape}"
    );
}
