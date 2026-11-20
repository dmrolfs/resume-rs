//! ABOUTME: Shared recognition of reserved annotation comment bodies.
//! ABOUTME: Full annotation grammar belongs to tailoring, not this lexical predicate.

/// True when one complete comment reserves its first token for annotations.
/// Matches the concurrently developed resume annotation API.
pub(crate) fn is_annotation(comment: &str) -> bool {
    comment
        .strip_prefix("<!--")
        .and_then(|s| s.strip_suffix("-->"))
        .filter(|s| !s.contains("-->"))
        .and_then(|s| s.split_whitespace().next())
        .is_some_and(|s| s.starts_with(['@', '#', '=']))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_token_decides_reservation() {
        for body in ["@draft", " \n #1 reason", "=", "@unknown"] {
            assert!(is_annotation(&format!("<!--{body}-->")));
        }
        for body in ["", "example @draft", "plain"] {
            assert!(!is_annotation(&format!("<!--{body}-->")));
        }
    }
}
