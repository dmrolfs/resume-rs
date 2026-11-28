//! Synthetic samples of the manifest, status, report, fixture, probe and comparison files.
use super::*;
use claims::{assert_err, assert_none, assert_ok, assert_some};
use pretty_assertions::assert_eq;

const D: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn same_members(written: &str, original: &str) {
    let a: serde_json::Value = assert_ok!(serde_json::from_str(written));
    let b: serde_json::Value = assert_ok!(serde_json::from_str(original));
    assert_eq!(a, b);
}

/// Decodes, writes back, and compares the members with the original JSON.
fn round_trips<T: serde::de::DeserializeOwned + serde::Serialize>(json: &str) -> T {
    let value: T = assert_ok!(decode(json.as_bytes()), "{json}");
    same_members(&assert_ok!(serde_json::to_string(&value)), json);
    value
}

fn configuration(total: bool) -> String {
    let total = if total { r#""total_timeout":300,"# } else { "" };
    format!(
        r#"{{"base_url":"http://127.0.0.1:11434/v1","requested_model":"m","remote_consent":false,"credential_used":false,"custom_ca":false,"engagement":null,"idle_timeout":120,{total}"max_tokens":4096,"temperature":null,"response_format":"json_object"}}"#
    )
}

// ---- manifests -----------------------------------------------------------------------------------------------

fn manifest(parts: &[&str]) -> String {
    format!(
        r#"{{"configuration":{},"baseline_sha256":"{D}","jd_sha256":"{D}","code_sha256":{{"advise.py":"{D}","evaluate.py":"{D}"}},"clauses_source":"owner-reviewed-file","mode":"m"{}}}"#,
        configuration(true),
        parts.iter().map(|p| format!(",{p}")).collect::<String>()
    )
}

#[test]
fn a_manifest_keeps_historical_omissions_and_nulls_distinct() {
    let approval = format!(r#""approval":{{"reviewer":"Ada","jd_sha256":"{D}","clauses_sha256":"{D}"}}"#);
    let newest = manifest(&[
        &format!(r#""configuration_file_sha256":"{D}""#),
        r#""runtime":{"python":"3","platform":"p"}"#,
        &approval,
    ]);
    let m: RunManifest = round_trips(&newest);
    assert_some!(m.approval.as_present().and_then(Option::as_ref));
    let null_approval = manifest(&[r#""approval":null"#, r#""configuration_file_sha256":null"#]);
    let m: RunManifest = round_trips(&null_approval);
    assert_eq!(m.approval, Field::Present(None));
    assert_eq!(m.configuration_file_sha256, Field::Present(None));
    let oldest = manifest(&[]);
    let m: RunManifest = round_trips(&oldest);
    assert!(m.approval.is_missing() && m.runtime.is_missing() && m.configuration_file_sha256.is_missing());
}

#[test]
fn a_manifest_with_an_older_configuration_still_reads() {
    let json = manifest(&[]).replace(&configuration(true), &configuration(false));
    let m: RunManifest = round_trips(&json);
    assert!(m.configuration.total_timeout.is_missing());
}

#[test]
fn a_manifest_keeps_unknown_members_and_refuses_a_missing_required_one() {
    let json = manifest(&[r#""invented":{"deep":[1,2]}"#]);
    let m: RunManifest = round_trips(&json);
    assert_eq!(m.extra.len(), 1);
    assert_some!(m.extra.get("invented"));
    assert_err!(decode::<RunManifest>(
        manifest(&[]).replace(r#","mode":"m""#, "").as_bytes()
    ));
    assert_err!(decode::<RunManifest>(
        manifest(&[])
            .replace("owner-reviewed-file", "owner-approved")
            .as_bytes()
    ));
}

#[test]
fn every_clause_source_seen_in_saved_runs_reads_and_a_development_case_is_not_an_approval() {
    for (wire, expected) in [
        ("owner-reviewed-file", ClausesSource::OwnerReviewedFile),
        ("model-extracted-unreviewed", ClausesSource::ModelExtractedUnreviewed),
        ("supplied-development-case", ClausesSource::SuppliedDevelopmentCase),
    ] {
        let m: RunManifest = round_trips(&manifest(&[]).replace("owner-reviewed-file", wire));
        assert_eq!(m.clauses_source, expected);
        assert_eq!(m.clauses_source.to_string(), wire);
        assert!(m.approval.is_missing(), "reading `{wire}` never creates an approval");
    }
}

#[test]
fn a_gateway_era_configuration_is_inert_data() {
    let json = configuration(true)
        .replace("http://127.0.0.1:11434/v1", "https://gateway.example.test/v1")
        .replace(
            r#""remote_consent":false,"credential_used":false,"custom_ca":false,"engagement":null"#,
            r#""remote_consent":true,"credential_used":true,"custom_ca":true,"engagement":"eng-1""#,
        );
    let c: SavedConfiguration = round_trips(&json);
    assert_eq!(c.base_url, "https://gateway.example.test/v1");
    assert_eq!(c.engagement, Some("eng-1".to_string()));
}

#[test]
fn a_compare_manifest_has_optional_prompt_provenance() {
    let with = format!(
        r#"{{"fixture":"f","configuration":{},"system_prompt_sha256":"{D}","system_prompt_source":"built-in"}}"#,
        configuration(true)
    );
    let without = format!(r#"{{"fixture":"f","configuration":{}}}"#, configuration(true));
    assert!(!round_trips::<CompareManifest>(&with).system_prompt_sha256.is_missing());
    assert!(
        round_trips::<CompareManifest>(&without)
            .system_prompt_sha256
            .is_missing()
    );
}

// ---- status --------------------------------------------------------------------------------------------------

#[test]
fn each_status_reads_its_own_members_and_refuses_another_variant_s() {
    for (json, variant) in [
        (r#"{"status":"running","usable_as_complete_proposal":false}"#, "running"),
        (
            r#"{"status":"awaiting-clause-review","inputs_unchanged":true,"clauses_extracted":12}"#,
            "awaiting",
        ),
        (
            r#"{"status":"complete-unreviewed","inputs_unchanged":true,"clauses_assessed":9}"#,
            "complete",
        ),
        (
            r#"{"status":"failed","usable_as_complete_proposal":false,"stage":"extraction:1","category":"timeout","http_status":null}"#,
            "failed",
        ),
        (
            r#"{"status":"failed","usable_as_complete_proposal":false,"stage":"s","category":"http-status","http_status":502,"inputs_changed_or_unreadable":true}"#,
            "failed",
        ),
        (r#"{"status":"failed","usable_as_complete_proposal":false}"#, "failed"),
    ] {
        let status: RunStatus = round_trips(json);
        let seen = match status {
            RunStatus::Running(_) => "running",
            RunStatus::AwaitingClauseReview(_) => "awaiting",
            RunStatus::CompleteUnreviewed(_) => "complete",
            RunStatus::Failed(_) => "failed",
        };
        assert_eq!(seen, variant);
    }
    for bad in [
        r#"{"status":"running"}"#,
        r#"{"status":"done","usable_as_complete_proposal":false}"#,
        r#"{"status":"complete-unreviewed","inputs_unchanged":true}"#,
        r#"{"usable_as_complete_proposal":false}"#,
        r#"{"status":7}"#,
        r#"{"status":"failed","usable_as_complete_proposal":false,"http_status":70000}"#,
    ] {
        assert_err!(decode::<RunStatus>(bad.as_bytes()), "{bad}");
    }
}

#[test]
fn an_unknown_status_is_refused_without_repeating_it() {
    let error = assert_err!(decode::<RunStatus>(br#"{"status":"private-label"}"#));
    assert!(!error.to_string().contains("private-label"));
}

// ---- reports, results, references ----------------------------------------------------------------------------

#[test]
fn a_report_may_lack_skipped_context_which_is_not_an_empty_list() {
    let base = r#""status":"proposal-only","coverage_review":"owner-attested","semantic_review":"pending","clauses":[],"assessments":[],"cited_wordings":[],"warning":"w""#;
    let without: Report = round_trips(&format!("{{{base}}}"));
    let empty: Report = round_trips(&format!(r#"{{{base},"skipped_context":[]}}"#));
    assert!(without.skipped_context.is_missing());
    assert_eq!(empty.skipped_context, Field::Present(vec![]));
}

#[test]
fn a_call_result_needs_usage_which_may_be_null_and_has_the_right_types() {
    assert_err!(decode::<CallResult>(
        br#"{"served_model":"m","seconds":1.0,"status":"ok"}"#
    ));
    assert_err!(decode::<CallResult>(
        br#"{"served_model":"m","usage":null,"seconds":"1","status":"ok"}"#
    ));
    for status in ["structure-valid", "ok"] {
        let json = format!(r#"{{"served_model":"m","usage":{{"t":1}},"seconds":2.5,"status":"{status}"}}"#);
        assert_eq!(
            assert_ok!(decode::<CallResult>(json.as_bytes())).status.to_string(),
            status
        );
    }
    let problems = r#"{"served_model":"m","usage":null,"seconds":2.0,"status":"ok","problems":[]}"#;
    assert_some!(
        assert_ok!(decode::<CallResult>(problems.as_bytes()))
            .extra
            .get("problems")
    );
}

#[test]
fn a_reference_may_have_no_served_model() {
    let json = format!(
        r#"{{"served_model":null,"configuration":{},"assessments":[]}}"#,
        configuration(true)
    );
    assert_eq!(round_trips::<Reference>(&json).served_model, None);
    assert_err!(decode::<Reference>(
        json.replace(r#""served_model":null,"#, "").as_bytes()
    ));
}

// ---- fixtures, probes ----------------------------------------------------------------------------------------

#[test]
fn a_fixture_without_kind_is_a_reference_and_stays_without_one_when_written() {
    let reference = format!(r#"{{"files":{{"jd.txt":"{D}"}},"source_run":"r","created":"2026-11-01"}}"#);
    let Fixture::Reference(fixture) = round_trips::<Fixture>(&reference) else {
        panic!("a reference")
    };
    assert!(fixture.kind.is_missing());
    let probe = format!(r#"{{"kind":"probe","files":{{"jd.txt":"{D}"}},"labels":"l","created":"c"}}"#);
    assert!(matches!(round_trips::<Fixture>(&probe), Fixture::Probe(_)));
    let explicit = format!(r#"{{"kind":"reference","files":{{"jd.txt":"{D}"}},"source_run":"r","created":"c"}}"#);
    let Fixture::Reference(explicit) = round_trips::<Fixture>(&explicit) else {
        panic!("a reference")
    };
    assert!(!explicit.kind.is_missing());
    assert_err!(decode::<Fixture>(probe.replace("probe", "other").as_bytes()));
    assert_err!(decode::<Fixture>(reference.replace(D, "xyz").as_bytes()));
}

#[test]
fn a_probe_spec_keeps_its_annotations() {
    let json = r#"{"probes":[
        {"id":"r001","category":"near-miss","text":"t","absent_terms":["x"],"note":"n","label":"n01","related_baseline":"b"},
        {"id":"r002","category":"control","text":"t","citations":["a.default"],"importance":"preferred"},
        {"id":"r003","category":"negative","text":"t","absent_terms":["y"],"private_annotation":{"k":[1]}}]}"#;
    let spec: ProbeSpec = round_trips(json);
    assert_eq!(spec.probes.len(), 3);
    assert!(!spec.probes[0].related_baseline.is_missing());
    assert!(spec.probes[1].related_baseline.is_missing());
    assert_some!(spec.probes[2].extra.get("private_annotation"));
    assert_err!(decode::<ProbeSpec>(
        br#"{"probes":[{"id":"r","text":"t","category":"unknown"}]}"#
    ));
    assert_err!(decode::<ProbeSpec>(br#"{"probes":[{"id":"r","text":"t"}]}"#));
}

#[test]
fn expected_probes_are_keyed_by_clause_id() {
    let json = r#"{"r001":{"label":"n01","category":"trap","acceptable":["adjacent","not-established"],"citations":[],"note":"n"}}"#;
    let expected: ExpectedProbes = round_trips(json);
    assert_eq!(expected["r001"].category, ProbeCategory::Trap);
    assert_err!(decode::<ExpectedProbes>(json.replace("trap", "baited").as_bytes()));
}

// ---- comparison ----------------------------------------------------------------------------------------------

#[test]
fn candidate_results_read_both_outcomes() {
    let json = r#"{"r001":{"status":"ok","support":"direct","citations":["a.default"]},"r002":{"status":"failed","category":"validation"}}"#;
    let results: CandidateResults = round_trips(json);
    assert!(matches!(results["r001"], ClauseResult::Ok(_)));
    assert!(matches!(results["r002"], ClauseResult::Failed(_)));
    for bad in [
        r#"{"r":{"status":"ok","support":"direct"}}"#,
        r#"{"r":{"status":"failed"}}"#,
        r#"{"r":{"status":"pending"}}"#,
        r#"{"r":{"support":"direct","citations":[]}}"#,
    ] {
        assert_err!(decode::<CandidateResults>(bad.as_bytes()), "{bad}");
    }
}

fn probe_summary(direct: &str) -> String {
    format!(
        r#"{{"kind":"probe","by_category":{{"control":{{"n":6,"valid":6,"as_expected":6}},"negative":{{"n":4,"valid":3,"as_expected":3}}}},"failed":{{"r014":"validation"}},{direct}"false_support_on_negatives":{{"r001":"adjacent"}},"controls_not_direct":{{}},"controls_direct_without_expected_citation":[],"note":"n"}}"#
    )
}

#[test]
fn both_probe_summary_writers_read_and_their_metrics_stay_separate() {
    let older = probe_summary(r#""direct_on_traps":{"r016":["trap","direct"]},"#);
    let newer = probe_summary(r#""direct_on_near_misses_or_traps":{"r015":["near-miss","direct"]},"#);
    let (older, newer): (ProbeSummary, ProbeSummary) = (round_trips(&older), round_trips(&newer));
    assert!(!older.direct_on_traps.is_missing() && older.direct_on_near_misses_or_traps.is_missing());
    assert!(newer.direct_on_traps.is_missing() && !newer.direct_on_near_misses_or_traps.is_missing());
    assert!(older.rejected_claiming_direct.is_missing());
    let with_rejected = probe_summary(
        r#""direct_on_near_misses_or_traps":{},"rejected_claiming_direct":["r016"],"rejected_overclaims_on_negatives_near_misses_traps":{"r016":["trap","direct"]},"#,
    );
    assert!(
        !round_trips::<ProbeSummary>(&with_rejected)
            .rejected_claiming_direct
            .is_missing()
    );
    assert_err!(decode::<ProbeSummary>(
        probe_summary("")
            .replace(r#""kind":"probe""#, r#""kind":"reference""#)
            .as_bytes()
    ));
}

#[test]
fn a_reference_summary_may_have_no_jaccard_mean() {
    let json = r#"{"clauses":2,"valid_answers":1,"failed":{"r002":"validation"},"rejected_claiming_direct":[],"exact_agreement":1,"candidate_stronger_than_reference":{"r001":["adjacent","direct"]},"candidate_weaker_than_reference":{},"mean_citation_jaccard":null}"#;
    assert_eq!(round_trips::<ReferenceSummary>(json).mean_citation_jaccard, None);
    let with = json.replace("null", "0.667");
    assert_eq!(
        round_trips::<ReferenceSummary>(&with).mean_citation_jaccard,
        Some(0.667)
    );
}

// ---- legacy evidence and segments ----------------------------------------------------------------------------

#[test]
fn legacy_evidence_and_segments_read_and_keep_extras() {
    let rows = "[{\"id\":\"a.default\",\"text\":\"t\",\"section\":\"## S\",\"role\":\"### R\",\"era\":\"**E · 2020**\",\"note\":\"x\"}]";
    let evidence: Vec<LegacyEvidence> = round_trips(rows);
    assert_eq!(evidence[0].section, "## S");
    assert_some!(evidence[0].extra.get("note"));
    let segments: Vec<Segment> = round_trips(r#"[{"id":1,"start":0,"end":5,"text":"héllo"}]"#);
    assert_eq!(segments[0].end.get(), 5);
    assert_err!(decode::<Vec<Segment>>(br#"[{"id":1,"start":-1,"end":5,"text":"t"}]"#));
    let blocks: Vec<String> = round_trips(r#"["one","two"]"#);
    assert_eq!(blocks.len(), 2);
}

#[test]
fn an_extra_that_repeats_a_known_member_cannot_be_written() {
    let mut segment: Segment = assert_ok!(decode(br#"{"id":1,"start":0,"end":5,"text":"t"}"#));
    segment
        .extra
        .insert("text", assert_ok!(JsonValue::parse(r#""shadow""#)));
    assert_err!(serde_json::to_string(&segment));
}

// ---- review findings: tag collisions, escaped discriminators, preservation -------------------------------------

#[test]
fn an_extra_named_like_the_tag_cannot_be_written_beside_it() {
    let mut status: RunStatus = assert_ok!(decode(br#"{"status":"running","usable_as_complete_proposal":false}"#));
    let RunStatus::Running(running) = &mut status else {
        panic!("running")
    };
    running
        .extra
        .insert("status", assert_ok!(JsonValue::parse(r#""failed""#)));
    assert_err!(serde_json::to_string(&status));
    let mut results: CandidateResults = assert_ok!(decode(br#"{"r":{"status":"failed","category":"validation"}}"#));
    let Some(ClauseResult::Failed(failed)) = results.get_mut("r") else {
        panic!("failed")
    };
    failed.extra.insert("status", assert_ok!(JsonValue::parse(r#""ok""#)));
    assert_err!(serde_json::to_string(&results));
    // A same-valued extra is still a second member, so it is refused too.
    let mut same: RunStatus = assert_ok!(decode(br#"{"status":"running","usable_as_complete_proposal":false}"#));
    let RunStatus::Running(running) = &mut same else {
        panic!("running")
    };
    running
        .extra
        .insert("status", assert_ok!(JsonValue::parse(r#""running""#)));
    assert_err!(serde_json::to_string(&same));
}

#[test]
fn a_discriminator_is_read_by_its_value_not_its_spelling() {
    let files = format!(r#""files":{{"jd.txt":"{D}"}}"#);
    for kind in [r#""pr\u006fbe""#, r#""\u0070robe""#, r#""probe""#] {
        let json = format!(r#"{{"kind":{kind},{files},"labels":"l","created":"c"}}"#);
        assert!(
            matches!(assert_ok!(decode::<Fixture>(json.as_bytes())), Fixture::Probe(_)),
            "{kind}"
        );
    }
    for kind in [r#""refer\u0065nce""#, r#""reference""#] {
        let json = format!(r#"{{"kind":{kind},{files},"source_run":"r","created":"c"}}"#);
        assert!(
            matches!(assert_ok!(decode::<Fixture>(json.as_bytes())), Fixture::Reference(_)),
            "{kind}"
        );
    }
    let status = br#"{"st\u0061tus":"run\u006eing","usable_as_complete_proposal":false}"#;
    assert!(matches!(assert_ok!(decode::<RunStatus>(status)), RunStatus::Running(_)));
    for kind in [
        "null",
        "1",
        "true",
        r#""""#,
        r#""Probe""#,
        r#""probes""#,
        "[]",
        r#"{"a":1}"#,
    ] {
        let probe = format!(r#"{{"kind":{kind},{files},"labels":"l","created":"c"}}"#);
        let reference = format!(r#"{{"kind":{kind},{files},"source_run":"r","created":"c"}}"#);
        assert_err!(decode::<Fixture>(probe.as_bytes()), "{kind}");
        assert_err!(decode::<Fixture>(reference.as_bytes()), "{kind}");
    }
}

#[test]
fn provider_usage_survives_a_call_result_exactly() {
    let usage = r#"{"prompt_tokens":18446744073709551616,"details":{"cached":1},"$serde_json::private::Number":"1","a":[1.50,-0]}"#;
    let json = format!(r#"{{"served_model":"m","usage":{usage},"seconds":1.25,"status":"structure-valid"}}"#);
    let result: CallResult = assert_ok!(decode(json.as_bytes()));
    let written = assert_ok!(serde_json::to_string(&result));
    assert!(written.contains(&format!(r#""usage":{usage}"#)), "{written}");
    let null: CallResult = assert_ok!(decode(
        br#"{"served_model":"m","usage":null,"seconds":2,"status":"ok"}"#
    ));
    assert!(assert_ok!(serde_json::to_string(&null)).contains(r#""usage":null"#));
    assert!(assert_some!(result.usage).as_json().contains("18446744073709551616"));
    assert_none!(null.usage);
}

#[test]
fn extras_keep_numbers_and_sentinel_objects_inside_every_open_object() {
    let big = "123456789012345678901234567890";
    let sentinel = r#"{"$serde_json::private::RawValue":"1","after":[1.0]}"#;
    let json = format!(
        r#"{{"id":1,"start":0,"end":3,"text":"t","big":{big},"s":{sentinel},"$serde_json::private::Number":"7"}}"#
    );
    let segment: Segment = assert_ok!(decode(json.as_bytes()));
    assert_eq!(assert_some!(segment.extra.get("big")).as_json(), big);
    assert_eq!(assert_some!(segment.extra.get("s")).as_json(), sentinel);
    assert_eq!(
        assert_some!(segment.extra.get("$serde_json::private::Number")).as_json(),
        r#""7""#
    );
    let written = assert_ok!(serde_json::to_string(&segment));
    for part in [big, sentinel, r#""$serde_json::private::Number":"7""#] {
        assert!(written.contains(part), "{part} in {written}");
    }
    // The same inside a tagged payload, with the sentinel first among the extras.
    let status = format!(
        r#"{{"status":"running","usable_as_complete_proposal":true,"$serde_json::private::RawValue":{{"n":{big}}}}}"#
    );
    let RunStatus::Running(running) = assert_ok!(decode::<RunStatus>(status.as_bytes())) else {
        panic!("running")
    };
    assert_eq!(
        assert_some!(running.extra.get("$serde_json::private::RawValue")).as_json(),
        format!(r#"{{"n":{big}}}"#)
    );
}

#[test]
fn duplicate_names_inside_an_extra_or_in_usage_are_refused_by_the_boundary_callers_use() {
    for json in [
        r#"{"id":1,"start":0,"end":3,"text":"t","x":{"a":1,"a":2}}"#,
        r#"{"id":1,"start":0,"end":3,"text":"t","x":[{"a":1,"\u0061":2}]}"#,
    ] {
        assert_err!(decode::<Segment>(json.as_bytes()), "{json}");
    }
    assert_err!(decode::<CallResult>(
        br#"{"served_model":"m","usage":{"a":1,"a":2},"seconds":1,"status":"ok"}"#
    ));
    assert_err!(decode::<RunStatus>(
        br#"{"status":"running","usable_as_complete_proposal":true,"x":{"d":0,"d":1}}"#
    ));
}

#[test]
fn nesting_inside_a_retained_member_counts_against_the_whole_document() {
    let nest = |n: usize| format!("{}0{}", "[".repeat(n), "]".repeat(n));
    // The segment object is the first container, so a member holding 126 more is the deepest allowed.
    assert_ok!(decode::<Segment>(
        format!(r#"{{"id":1,"start":0,"end":3,"text":"t","x":{}}}"#, nest(126)).as_bytes()
    ));
    assert_err!(decode::<Segment>(
        format!(r#"{{"id":1,"start":0,"end":3,"text":"t","x":{}}}"#, nest(127)).as_bytes()
    ));
    assert_err!(decode::<CallResult>(
        format!(
            r#"{{"served_model":"m","usage":{},"seconds":1,"status":"ok"}}"#,
            nest(127)
        )
        .as_bytes()
    ));
}

#[test]
fn a_missing_field_cannot_be_serialised_and_a_call_status_must_be_known() {
    assert_err!(serde_json::to_string(&Field::<u8>::Missing));
    assert_err!(decode::<CallResult>(
        br#"{"served_model":"m","usage":null,"seconds":1,"status":"done"}"#
    ));
}

#[test]
fn extras_report_their_members_in_name_order() {
    let json = r#"{"id":1,"start":0,"end":3,"text":"t","zeta":1,"alpha":{"k":[true]},"mid":null}"#;
    let mut segment: Segment = assert_ok!(decode(json.as_bytes()));
    assert!(!segment.extra.is_empty());
    assert_eq!(segment.extra.len(), 3);
    let seen: Vec<(&str, &str)> = segment
        .extra
        .iter()
        .map(|(name, value)| (name, value.as_json()))
        .collect();
    assert_eq!(seen, [("alpha", r#"{"k":[true]}"#), ("mid", "null"), ("zeta", "1")]);
    let replaced = segment.extra.insert("mid", assert_ok!(JsonValue::parse("2")));
    assert_eq!(assert_some!(replaced).as_json(), "null");
    assert_eq!(segment.extra.len(), 3);
    let none: Segment = assert_ok!(decode(br#"{"id":1,"start":0,"end":3,"text":"t"}"#));
    assert!(none.extra.is_empty());
    assert_eq!(none.extra.len(), 0);
    assert_none!(none.extra.get("anything"));
}

// ---- every declared member: what happens when it is missing, and when it is null -------------------------------

/// What a member does when it is removed and when it is `null`: `(name, missing is accepted, null is accepted)`.
type Rule = (&'static str, bool, bool);

/// Removes each member of the object `sample` in turn, then sets it to `null`, and checks `decode::<T>` against `rules`.
/// The sample is synthetic and must decode; every member in it must have a rule, so a new member fails here.
fn member_rules<T: serde::de::DeserializeOwned>(sample: &str, rules: &[Rule]) {
    assert_ok!(decode::<T>(sample.as_bytes()), "the sample itself must decode");
    let object: serde_json::Map<String, serde_json::Value> = assert_ok!(serde_json::from_str(sample));
    let mut names: Vec<&str> = object.keys().map(String::as_str).collect();
    let mut declared: Vec<&str> = rules.iter().map(|r| r.0).collect();
    names.sort_unstable();
    declared.sort_unstable();
    assert_eq!(
        names, declared,
        "every member of the sample needs a rule, and every rule a member"
    );
    for &(name, missing_ok, null_ok) in rules {
        let mut without = object.clone();
        without.remove(name);
        let result = decode::<T>(assert_ok!(serde_json::to_vec(&without)).as_slice());
        assert_eq!(result.is_ok(), missing_ok, "`{name}` removed");
        let mut nulled = object.clone();
        nulled.insert(name.to_string(), serde_json::Value::Null);
        let result = decode::<T>(assert_ok!(serde_json::to_vec(&nulled)).as_slice());
        assert_eq!(result.is_ok(), null_ok, "`{name}` set to null");
    }
}

fn clean(json: String) -> String {
    json.split_whitespace().collect::<Vec<_>>().join("")
}

#[test]
fn saved_configuration_members() {
    member_rules::<SavedConfiguration>(
        &configuration(true),
        &[
            ("base_url", false, false),
            ("requested_model", false, false),
            ("remote_consent", false, false),
            ("credential_used", false, false),
            ("custom_ca", false, false),
            ("engagement", false, true),
            ("idle_timeout", false, false),
            ("total_timeout", true, false),
            ("max_tokens", false, false),
            ("temperature", false, true),
            ("response_format", false, false),
        ],
    );
}

#[test]
fn run_manifest_members() {
    let sample = clean(format!(
        r#"{{"configuration":{},"configuration_file_sha256":"{D}","runtime":{{"python":"3"}},"baseline_sha256":"{D}","jd_sha256":"{D}",
        "code_sha256":{{"a.py":"{D}"}},"clauses_source":"owner-reviewed-file","approval":{{"reviewer":"r","jd_sha256":"{D}","clauses_sha256":"{D}"}},"mode":"m"}}"#,
        configuration(true)
    ));
    member_rules::<RunManifest>(
        &sample,
        &[
            ("configuration", false, false),
            ("configuration_file_sha256", true, true),
            ("runtime", true, true),
            ("baseline_sha256", false, false),
            ("jd_sha256", false, false),
            ("code_sha256", false, false),
            ("clauses_source", false, false),
            ("approval", true, true),
            ("mode", false, false),
        ],
    );
}

#[test]
fn compare_manifest_and_reference_members() {
    let compare = format!(
        r#"{{"fixture":"f","configuration":{},"system_prompt_sha256":"{D}","system_prompt_source":"built-in"}}"#,
        configuration(true)
    );
    member_rules::<CompareManifest>(
        &clean(compare),
        &[
            ("fixture", false, false),
            ("configuration", false, false),
            ("system_prompt_sha256", true, false),
            ("system_prompt_source", true, false),
        ],
    );
    let reference = format!(
        r#"{{"served_model":"m","configuration":{},"assessments":[]}}"#,
        configuration(true)
    );
    member_rules::<Reference>(
        &clean(reference),
        &[
            ("served_model", false, true),
            ("configuration", false, false),
            ("assessments", false, false),
        ],
    );
}

#[test]
fn status_members_by_variant() {
    member_rules::<RunStatus>(
        r#"{"status":"running","usable_as_complete_proposal":false}"#,
        &[("status", false, false), ("usable_as_complete_proposal", false, false)],
    );
    member_rules::<RunStatus>(
        r#"{"status":"awaiting-clause-review","inputs_unchanged":true,"clauses_extracted":1}"#,
        &[
            ("status", false, false),
            ("inputs_unchanged", false, false),
            ("clauses_extracted", false, false),
        ],
    );
    member_rules::<RunStatus>(
        r#"{"status":"complete-unreviewed","inputs_unchanged":true,"clauses_assessed":1}"#,
        &[
            ("status", false, false),
            ("inputs_unchanged", false, false),
            ("clauses_assessed", false, false),
        ],
    );
    member_rules::<RunStatus>(
        r#"{"status":"failed","usable_as_complete_proposal":false,"stage":"s","category":"c","http_status":502,"inputs_changed_or_unreadable":true}"#,
        &[
            ("status", false, false),
            ("usable_as_complete_proposal", false, false),
            ("stage", true, false),
            ("category", true, false),
            ("http_status", true, true),
            ("inputs_changed_or_unreadable", true, false),
        ],
    );
}

#[test]
fn report_call_result_and_summary_members() {
    member_rules::<Report>(
        r#"{"status":"s","coverage_review":"c","semantic_review":"m","skipped_context":["r001"],"clauses":[],"assessments":[],"cited_wordings":[],"warning":"w"}"#,
        &[
            ("status", false, false),
            ("coverage_review", false, false),
            ("semantic_review", false, false),
            ("skipped_context", true, false),
            ("clauses", false, false),
            ("assessments", false, false),
            ("cited_wordings", false, false),
            ("warning", false, false),
        ],
    );
    member_rules::<CallResult>(
        r#"{"served_model":"m","usage":{"t":1},"seconds":1.5,"status":"ok"}"#,
        &[
            ("served_model", false, false),
            ("usage", false, true),
            ("seconds", false, false),
            ("status", false, false),
        ],
    );
    member_rules::<ReferenceSummary>(
        r#"{"clauses":1,"valid_answers":1,"failed":{},"rejected_claiming_direct":[],"exact_agreement":1,"candidate_stronger_than_reference":{},"candidate_weaker_than_reference":{},"mean_citation_jaccard":0.5}"#,
        &[
            ("clauses", false, false),
            ("valid_answers", false, false),
            ("failed", false, false),
            ("rejected_claiming_direct", false, false),
            ("exact_agreement", false, false),
            ("candidate_stronger_than_reference", false, false),
            ("candidate_weaker_than_reference", false, false),
            ("mean_citation_jaccard", false, true),
        ],
    );
    member_rules::<ProbeSummary>(
        r#"{"kind":"probe","by_category":{},"failed":{},"rejected_claiming_direct":[],"rejected_overclaims_on_negatives_near_misses_traps":{},"false_support_on_negatives":{},"direct_on_near_misses_or_traps":{},"direct_on_traps":{},"controls_not_direct":{},"controls_direct_without_expected_citation":[],"note":"n"}"#,
        &[
            ("kind", false, false),
            ("by_category", false, false),
            ("failed", false, false),
            ("rejected_claiming_direct", true, false),
            ("rejected_overclaims_on_negatives_near_misses_traps", true, false),
            ("false_support_on_negatives", false, false),
            ("direct_on_near_misses_or_traps", true, false),
            ("direct_on_traps", true, false),
            ("controls_not_direct", false, false),
            ("controls_direct_without_expected_citation", false, false),
            ("note", false, false),
        ],
    );
}

#[test]
fn probe_fixture_and_candidate_members() {
    member_rules::<ProbeSpecRow>(
        r#"{"id":"r001","text":"t","category":"trap","importance":"required","label":"l","absent_terms":[],"citations":[],"note":"n","related_baseline":"b"}"#,
        &[
            ("id", false, false),
            ("text", false, false),
            ("category", false, false),
            ("importance", true, false),
            ("label", true, false),
            ("absent_terms", true, false),
            ("citations", true, false),
            ("note", true, false),
            ("related_baseline", true, false),
        ],
    );
    member_rules::<ExpectedProbe>(
        r#"{"label":"l","category":"trap","acceptable":["adjacent"],"citations":[],"note":"n"}"#,
        &[
            ("label", false, false),
            ("category", false, false),
            ("acceptable", false, false),
            ("citations", false, false),
            ("note", false, false),
        ],
    );
    member_rules::<ProbeFixture>(
        &clean(format!(
            r#"{{"kind":"probe","files":{{"a":"{D}"}},"labels":"l","created":"c"}}"#
        )),
        &[
            ("kind", false, false),
            ("files", false, false),
            ("labels", false, false),
            ("created", false, false),
        ],
    );
    member_rules::<ReferenceFixture>(
        &clean(format!(
            r#"{{"kind":"reference","files":{{"a":"{D}"}},"source_run":"r","created":"c"}}"#
        )),
        &[
            ("kind", true, false),
            ("files", false, false),
            ("source_run", false, false),
            ("created", false, false),
        ],
    );
    member_rules::<ClauseResult>(
        r#"{"status":"ok","support":"direct","citations":[]}"#,
        &[
            ("status", false, false),
            ("support", false, false),
            ("citations", false, false),
        ],
    );
    member_rules::<ClauseResult>(
        r#"{"status":"failed","category":"c"}"#,
        &[("status", false, false), ("category", false, false)],
    );
}

#[test]
fn evidence_and_segment_members() {
    member_rules::<LegacyEvidence>(
        r#"{"id":"a.default","text":"t","section":"s","role":"r","era":"e"}"#,
        &[
            ("id", false, false),
            ("text", false, false),
            ("section", false, false),
            ("role", false, false),
            ("era", false, false),
        ],
    );
    member_rules::<Segment>(
        r#"{"id":1,"start":0,"end":1,"text":"t"}"#,
        &[
            ("id", false, false),
            ("start", false, false),
            ("end", false, false),
            ("text", false, false),
        ],
    );
}
