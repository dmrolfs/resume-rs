//! ABOUTME: Public-API checks for the legacy contracts: a non-ASCII job description keeps its clauses, and approvals bind to bytes.
//! ABOUTME: The optional private smoke test reads the owner's saved files and prints counts only, never a path or a value.

mod contract_support;

use advisor::contract::{Approval, Clauses, decode};
use advisor::digest::Sha256Digest;
use advisor::text::{IndexedText, occurrences};
use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;

/// A job description whose clauses sit after multibyte characters, so a byte offset would land inside or past them.
const JD: &str = "Café · Ünïcode team 😀\nRequires: Rust, 猫 and Go.\nPrefers: Python.\n";

fn clauses_json() -> String {
    let line = "Requires: Rust, 猫 and Go.";
    let parent = JD.chars().count()
        - JD[assert_some!(JD.find(line), "the line is in the JD")..]
            .chars()
            .count();
    format!(
        r#"{{"clauses":[
 {{"id":"r001","quote":"Rust","parent":"{line}","importance":"required","source_start":{parent}}},
 {{"id":"r002","quote":"猫","parent":"{line}","importance":"required","source_start":{parent}}},
 {{"id":"r003","quote":"Python","parent":"Prefers: Python.","importance":"preferred"}}]}}
"#
    )
}

#[test]
fn a_clause_start_counts_characters_so_a_non_ascii_job_description_keeps_its_clauses() {
    let bytes = clauses_json().into_bytes();
    let clauses: Clauses = assert_ok!(decode(&bytes));
    let jd = IndexedText::new(JD);
    for clause in &clauses.clauses {
        let Some(Some(start)) = clause.source_start.as_present().copied() else {
            continue;
        };
        let end = assert_some!(start.checked_add(clause.parent.chars().count()));
        assert_eq!(assert_ok!(jd.slice(start..end)), clause.parent, "{}", clause.id);
        // The same number read as a byte offset is wrong: it lands on other text or inside a character.
        assert_ne!(
            JD.get(start.get()..start.get() + clause.parent.len()),
            Some(clause.parent.as_str())
        );
        assert_eq!(occurrences(&clause.parent, &clause.quote), 1);
    }
    let legacy = &clauses.clauses[2];
    assert!(legacy.source_start.is_missing());
    assert_eq!(
        occurrences(JD, &legacy.parent),
        1,
        "a legacy clause resolves only through a parent that occurs once"
    );
}

#[test]
fn an_approval_binds_to_the_bytes_of_the_files_it_names_not_to_what_they_mean() {
    let clause_bytes = clauses_json().into_bytes();
    let approval_json = format!(
        r#"{{"reviewer":"Ada","jd_sha256":"{}","clauses_sha256":"{}"}}"#,
        Sha256Digest::of(JD.as_bytes()),
        Sha256Digest::of(&clause_bytes)
    );
    let approval: Approval = assert_ok!(decode(approval_json.as_bytes()));
    assert_eq!(approval.clauses_sha256, Sha256Digest::of(&clause_bytes));
    // The same clauses, written another way, are another file: the approval no longer names it.
    let rewritten = assert_ok!(serde_json::to_vec_pretty(&assert_ok!(decode::<Clauses>(&clause_bytes))));
    assert_ne!(approval.clauses_sha256, Sha256Digest::of(&rewritten));
    assert_eq!(
        assert_ok!(decode::<Clauses>(&rewritten)),
        assert_ok!(decode::<Clauses>(&clause_bytes))
    );
}

// ---- private smoke test --------------------------------------------------------------------------------------

/// Reads the owner's saved artifacts. Run it on purpose: `ADVISOR_PRIVATE_FIXTURES=<root> cargo test --package advisor
/// --test contract_test -- --ignored --nocapture`. It is ignored by default, so a plain test run never claims to have
/// read private files; invoked without the variable it fails instead of passing. It prints counts only.
#[test]
#[ignore = "reads the owner's private files; run on purpose with ADVISOR_PRIVATE_FIXTURES"]
fn the_owners_saved_artifacts_decode_and_keep_every_member() {
    let root = std::env::var_os("ADVISOR_PRIVATE_FIXTURES");
    let root = assert_some!(root, "set ADVISOR_PRIVATE_FIXTURES to the private root");
    let scan = contract_support::private_scan::Scan::run(std::path::Path::new(&root));
    println!("decoded: {:?}", scan.decoded);
    println!("refused: {:?}", scan.refused);
    println!("outside the supported layout: {:?}", scan.outside);
    println!("unreadable: {:?}", scan.unreadable);
    contract_support::private_scan::assert_inventory(&scan);
}
