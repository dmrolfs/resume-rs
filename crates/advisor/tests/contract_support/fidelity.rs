//! Compares JSON members without `Value`, floating-point rounding, or serde's sentinel keys.
use serde_json::value::RawValue;
use std::collections::BTreeMap;

/// Object order and string escape spelling may change. Opaque numbers compare by their exact spelling; only a known
/// typed float member may gain `.0` when written by Rust (e.g. seconds `2` becomes `2.0`).
pub fn same_members(a: &[u8], b: &[u8]) -> bool {
    fn same(a: &RawValue, b: &RawValue, field: &str) -> bool {
        let (a, b) = (a.get(), b.get());
        match (a.as_bytes().first(), b.as_bytes().first()) {
            (Some(b'{'), Some(b'{')) => {
                let Ok(a) = serde_json::from_str::<BTreeMap<String, Box<RawValue>>>(a) else {
                    return false;
                };
                let Ok(b) = serde_json::from_str::<BTreeMap<String, Box<RawValue>>>(b) else {
                    return false;
                };
                a.len() == b.len() && a.iter().all(|(k, a)| b.get(k).is_some_and(|b| same(a, b, k)))
            }
            (Some(b'['), Some(b'[')) => {
                let Ok(a) = serde_json::from_str::<Vec<Box<RawValue>>>(a) else {
                    return false;
                };
                let Ok(b) = serde_json::from_str::<Vec<Box<RawValue>>>(b) else {
                    return false;
                };
                a.len() == b.len() && a.iter().zip(&b).all(|(a, b)| same(a, b, ""))
            }
            (Some(b'"'), Some(b'"')) => match (serde_json::from_str::<String>(a), serde_json::from_str::<String>(b)) {
                (Ok(a), Ok(b)) => a == b,
                _ => false,
            },
            _ if ["seconds", "temperature", "mean_citation_jaccard"].contains(&field) => {
                a == b || a.strip_suffix(".0") == Some(b) || b.strip_suffix(".0") == Some(a)
            }
            _ => a == b,
        }
    }
    match (
        serde_json::from_slice::<Box<RawValue>>(a),
        serde_json::from_slice::<Box<RawValue>>(b),
    ) {
        (Ok(a), Ok(b)) => same(&a, &b, ""),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(a: &str, b: &str) -> bool {
        same_members(a.as_bytes(), b.as_bytes())
    }

    #[test]
    fn neighbouring_big_integers_and_float_spellings_differ() {
        assert!(!same(r#"{"n":9007199254740992}"#, r#"{"n":9007199254740993}"#));
        assert!(!same(r#"{"n":18446744073709551616}"#, r#"{"n":18446744073709551617}"#));
        assert!(!same(r#"{"n":1.50}"#, r#"{"n":1.5}"#));
        assert!(same(r#"{"n":1}"#, r#"{"n":1}"#));
    }

    #[test]
    fn a_sentinel_object_is_not_its_embedded_value_and_is_compared_whole() {
        let sentinel = r#"{"$serde_json::private::RawValue":"100"}"#;
        assert!(same(sentinel, sentinel));
        assert!(!same(sentinel, "100"));
        assert!(!same(sentinel, r#""100""#));
        let with_sibling = r#"{"$serde_json::private::RawValue":"1","extra":true}"#;
        assert!(same(with_sibling, with_sibling));
        assert!(!same(with_sibling, r#"{"$serde_json::private::RawValue":"1"}"#));
        assert!(!same(r#"{"$serde_json::private::Number":"7"}"#, "7"));
    }

    #[test]
    fn a_lost_member_a_changed_value_or_a_reordered_array_is_a_difference() {
        assert!(!same(r#"{"a":1,"b":2}"#, r#"{"a":1}"#));
        assert!(!same(r#"{"a":1}"#, r#"{"a":1,"b":2}"#));
        assert!(!same(r#"{"a":"x"}"#, r#"{"a":"y"}"#));
        assert!(!same(r#"{"a":[1,2]}"#, r#"{"a":[2,1]}"#));
        assert!(!same(r#"{"a":null}"#, r#"{"a":false}"#));
        assert!(!same(r#"{"a":1}"#, r#"[1]"#));
        assert!(!same("{", "{"));
    }

    #[test]
    fn member_order_and_string_escapes_may_change_but_only_a_known_float_gains_a_decimal_point() {
        assert!(same(r#"{"a":1,"b":"x"}"#, r#"{"b":"\u0078","a":1}"#));
        assert!(same(r#"{"seconds":2}"#, r#"{"seconds":2.0}"#));
        assert!(same(r#"{"temperature":1}"#, r#"{"temperature":1.0}"#));
        assert!(!same(r#"{"usage":2}"#, r#"{"usage":2.0}"#));
        assert!(!same(r#"{"other":{"n":2}}"#, r#"{"other":{"n":2.0}}"#));
    }
}
