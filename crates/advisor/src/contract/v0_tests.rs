//! Synthetic samples of every artifact family, shaped like the files the spike writes. No private text.
use super::*;
use crate::digest::Sha256Digest;
use crate::text::CharOffset;
use claims::{assert_err, assert_ok};
use pretty_assertions::assert_eq;

const DIGEST: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const OTHER: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Decodes `json` and writes it back: the written members must equal the original's, so a dropped or changed member
/// fails here, not only an unstable second write.
fn round_trips<T: serde::de::DeserializeOwned + serde::Serialize>(json: &str) -> T {
    let value: T = assert_ok!(decode(json.as_bytes()), "{json}");
    same_members(&assert_ok!(serde_json::to_string(&value)), json);
    value
}

fn same_members(written: &str, original: &str) {
    let a: serde_json::Value = assert_ok!(serde_json::from_str(written));
    let b: serde_json::Value = assert_ok!(serde_json::from_str(original));
    assert_eq!(a, b);
}

// ---- clauses -------------------------------------------------------------------------------------------------

#[test]
fn clauses_keep_a_missing_a_null_and_a_present_source_start_apart() {
    let json = r#"{"clauses":[
        {"id":"r001","quote":"Rust","parent":"Rust and Go","importance":"required"},
        {"id":"r002","quote":"Go","parent":"Rust and Go","importance":"preferred","source_start":null},
        {"id":"r003","quote":"猫","parent":"猫 and 犬","importance":"context","source_start":12}]}"#;
    let file: Clauses = round_trips(json);
    let starts: Vec<_> = file.clauses.iter().map(|c| c.source_start).collect();
    assert_eq!(
        starts,
        [
            Field::Missing,
            Field::Present(None),
            Field::Present(Some(CharOffset::new(12)))
        ]
    );
    assert_eq!(file.clauses[2].importance, Importance::Context);
    same_members(&assert_ok!(serde_json::to_string(&file)), json);
}

#[test]
fn clause_files_and_rows_accept_exactly_their_keys() {
    let row = r#"{"id":"r001","quote":"a","parent":"a","importance":"required"}"#;
    assert_ok!(decode::<Clauses>(format!(r#"{{"clauses":[{row}]}}"#).as_bytes()));
    for bad in [
        format!(r#"{{"clauses":[{row}],"extra":1}}"#),
        r#"{}"#.to_string(),
        format!(r#"{{"clauses":[{}]}}"#, row.replace('}', r#","extra":1}"#)),
        format!(r#"{{"clauses":[{}]}}"#, row.replace(r#""id":"r001","#, "")),
        format!(r#"{{"clauses":[{}]}}"#, row.replace("required", "mandatory")),
        format!(
            r#"{{"clauses":[{}]}}"#,
            row.replace(
                r#""importance":"required""#,
                r#""importance":"required","source_start":-1"#
            )
        ),
        format!(
            r#"{{"clauses":[{}]}}"#,
            row.replace(
                r#""importance":"required""#,
                r#""importance":"required","source_start":1.0"#
            )
        ),
        format!(
            r#"{{"clauses":[{}]}}"#,
            row.replace(
                r#""importance":"required""#,
                r#""importance":"required","source_start":"3""#
            )
        ),
        format!(
            r#"{{"clauses":[{}]}}"#,
            row.replace(
                r#""importance":"required""#,
                r#""importance":"required","source_start":true"#
            )
        ),
    ] {
        assert_err!(decode::<Clauses>(bad.as_bytes()), "{bad}");
    }
}

#[test]
fn a_decoded_clause_is_not_a_validated_clause() {
    // The id pattern, the source text, the unique quote and the importance rules belong to the validators (rsm-eam.5.6).
    let loose =
        r#"{"clauses":[{"id":"not-an-id","quote":"","parent":"x","importance":"required","source_start":99999}]}"#;
    assert_ok!(decode::<Clauses>(loose.as_bytes()));
}

// ---- extraction proposals ------------------------------------------------------------------------------------

#[test]
fn an_extraction_proposal_has_segment_quote_and_importance_and_nothing_else() {
    let json = r#"{"clauses":[{"segment":3,"quote":"Rust","importance":"required"}]}"#;
    let file: ExtractionProposal = round_trips(json);
    assert_eq!(file.clauses[0].segment, 3);
    for bad in [
        r#"{"clauses":[{"segment":3,"quote":"a","importance":"required","id":"r001"}]}"#,
        r#"{"clauses":[{"segment":"3","quote":"a","importance":"required"}]}"#,
        r#"{"clauses":[{"segment":-3,"quote":"a","importance":"required"}]}"#,
        r#"{"clauses":[{"segment":3.5,"quote":"a","importance":"required"}]}"#,
        r#"{"clauses":[],"extra":null}"#,
    ] {
        assert_err!(decode::<ExtractionProposal>(bad.as_bytes()), "{bad}");
    }
}

// ---- assessments ---------------------------------------------------------------------------------------------

#[test]
fn an_assessment_reads_every_support_label_and_citation() {
    for support in ["direct", "adjacent", "not-established"] {
        let json = format!(
            r#"{{"id":"r001","support":"{support}","citations":[{{"id":"a.default","quote":"q"}}],"boundary":"b"}}"#
        );
        let value: Assessment = round_trips(&json);
        assert_eq!(value.support.to_string(), support);
        assert_eq!(
            (value.citations[0].id.as_str(), value.citations[0].quote.as_str()),
            ("a.default", "q")
        );
        assert_eq!(value.boundary, "b");
    }
    assert_eq!(
        assert_ok!(serde_json::to_string(&Support::NotEstablished)),
        r#""not-established""#
    );
}

#[test]
fn an_assessment_refuses_extra_missing_and_misspelled_members_at_each_level() {
    let ok = r#"{"id":"r001","support":"direct","citations":[{"id":"a.default","quote":"q"}],"boundary":"b"}"#;
    assert_ok!(decode::<Assessment>(ok.as_bytes()));
    for bad in [
        ok.replace(r#""boundary":"b""#, r#""boundary":"b","note":"n""#),
        ok.replace(
            r#"{"id":"a.default","quote":"q"}"#,
            r#"{"id":"a.default","quote":"q","extra":1}"#,
        ),
        ok.replace(r#","quote":"q""#, ""),
        ok.replace("direct", "Direct"),
        ok.replace("direct", "strong"),
        ok.replace(r#","boundary":"b""#, ""),
        r#"{"id":"r001","support":"direct","evidence":["a.default"],"boundary":"b"}"#.to_string(),
    ] {
        assert_err!(decode::<Assessment>(bad.as_bytes()), "{bad}");
    }
}

#[test]
fn an_assessment_decodes_without_being_judged() {
    // An unknown citation id, an empty boundary and four citations are the validators' business (rsm-eam.5.6).
    let loose = r#"{"id":"zz","support":"direct","citations":[],"boundary":""}"#;
    assert_ok!(decode::<Assessment>(loose.as_bytes()));
}

// ---- approval ------------------------------------------------------------------------------------------------

#[test]
fn an_approval_has_exactly_three_members_and_typed_digests() {
    let json = format!(r#"{{"reviewer":"Ada","jd_sha256":"{DIGEST}","clauses_sha256":"{OTHER}"}}"#);
    let approval: Approval = round_trips(&json);
    assert_eq!(approval.reviewer, "Ada");
    assert_eq!(approval.jd_sha256.to_string(), DIGEST);
    assert_eq!(approval.clauses_sha256.to_string(), OTHER);
    for bad in [
        json.replace("Ada", "Ada\",\"extra\":\"1"),
        json.replace(r#""reviewer":"Ada","#, ""),
        json.replace(DIGEST, &DIGEST.to_uppercase()),
        json.replace(OTHER, "abc"),
        json.replace(r#""reviewer":"Ada""#, r#""reviewer":null"#),
    ] {
        assert_err!(decode::<Approval>(bad.as_bytes()), "{bad}");
    }
}

#[test]
fn decoding_an_approval_is_not_approving_and_writes_nothing_new() {
    let written = format!(r#"{{"reviewer":"Ada","jd_sha256":"{DIGEST}","clauses_sha256":"{DIGEST}"}}"#);
    let approval: Approval = assert_ok!(decode(written.as_bytes()));
    // Whitespace, member order and escapes differ in the owner's file; the approval binds to those bytes, so the typed
    // value must never stand in for them.
    let owner_file = format!(
        "{{ \"jd_sha256\": \"{DIGEST}\",\n  \"reviewer\": \"A\\u0064a\", \"clauses_sha256\": \"{DIGEST}\" }}\n"
    );
    let from_owner_file: Approval = assert_ok!(decode(owner_file.as_bytes()));
    assert_eq!(from_owner_file, approval);
    assert_ne!(
        Sha256Digest::of(owner_file.as_bytes()),
        Sha256Digest::of(written.as_bytes())
    );
}
