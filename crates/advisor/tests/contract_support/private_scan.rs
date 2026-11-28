//! ABOUTME: Reads the owner's saved artifacts through the Rust contracts and reports counts, never a path or a value.
//! ABOUTME: Layout is classified by file name and directory depth, as the spike's two writers produce it; see `Grammar`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use advisor::contract::{
    Approval, Assessment, CallResult, CandidateResults, Clauses, CompareManifest, DecodeError, ExpectedProbes,
    ExtractionProposal, Fixture, LegacyEvidence, ProbeSpec, ProbeSummary, Reference, ReferenceSummary, Report,
    RunManifest, RunStatus, Segment, decode,
};
use claims::{assert_ok, assert_some};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;

use super::fidelity::same_members;

/// The file layout this scan knows, written down so a file outside it is a count, not a silent skip.
///
/// - `runs/<run>/` holds `manifest.json` and, for an advise run, `status.json`, `report.json`, `clauses.json`,
///   `supplied-clauses.json`, `approval.json`, `evidence.json`, `context-blocks.json`, `segments.json`; for a
///   compare run, `results.json`. A run directory with no manifest is outside the layout.
/// - `runs/<run>/<call>/` holds `result.json` and `proposal.json` when the run's manifest is a known kind: an advise
///   run's `extraction-*` calls hold extractions, a call named exactly `extraction` (an earlier writer) holds a clause
///   file, and its other calls hold assessments; a compare run's calls hold assessments. Call directories of other runs are outside the layout.
/// - `fixtures/<name>/` holds `fixture.json` and the files it lists; `fixtures/*.json` may be a probe spec.
/// - A probe or reference summary is any JSON file directly in a run or fixture directory whose members say so.
pub struct Scan {
    pub decoded: BTreeMap<&'static str, usize>,
    /// By family and error category, which files the contracts refused.
    pub refused: BTreeMap<(&'static str, &'static str), usize>,
    /// Files this scan did not try to read, and why.
    pub outside: BTreeMap<&'static str, usize>,
    /// Safe categories of filesystem problems met on the way.
    pub unreadable: BTreeMap<&'static str, usize>,
}

impl Scan {
    pub fn run(root: &Path) -> Self {
        let mut scan = Self {
            decoded: BTreeMap::new(),
            refused: BTreeMap::new(),
            outside: BTreeMap::new(),
            unreadable: BTreeMap::new(),
        };
        for run in scan.dirs(&root.join("runs")) {
            scan.run_dir(&run);
        }
        let fixtures = scan.entries(&root.join("fixtures"));
        for dir in fixtures.iter().filter(|p| p.is_dir()) {
            scan.fixture_dir(dir);
        }
        for spec in fixtures
            .iter()
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "json"))
        {
            if scan
                .object_names(spec)
                .is_some_and(|names| names.iter().any(|n| n == "probes"))
            {
                scan.read::<ProbeSpec>("probe-spec", spec);
            }
        }
        scan
    }

    fn note(&mut self, kind: &'static str) {
        *self.unreadable.entry(kind).or_default() += 1;
    }

    fn outside(&mut self, why: &'static str) {
        *self.outside.entry(why).or_default() += 1;
    }

    fn entries(&mut self, dir: &Path) -> Vec<PathBuf> {
        match fs::read_dir(dir) {
            Ok(entries) => {
                let mut out = Vec::new();
                for entry in entries {
                    match entry {
                        Ok(entry) => out.push(entry.path()),
                        Err(_) => self.note("directory-entry"),
                    }
                }
                out.sort();
                out
            }
            Err(_) => {
                self.note("directory");
                Vec::new()
            }
        }
    }

    fn dirs(&mut self, dir: &Path) -> Vec<PathBuf> {
        self.entries(dir).into_iter().filter(|p| p.is_dir()).collect()
    }

    fn files(&mut self, dir: &Path) -> Vec<PathBuf> {
        self.entries(dir)
            .into_iter()
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "json"))
            .collect()
    }

    /// The member names of the object in `path`, or `None` when it is not an object. A file that cannot be read is
    /// counted, never skipped silently.
    fn object_names(&mut self, path: &Path) -> Option<Vec<String>> {
        let Ok(text) = fs::read_to_string(path) else {
            self.note("file");
            return None;
        };
        let map: BTreeMap<String, Box<RawValue>> = serde_json::from_str(&text).ok()?;
        Some(map.into_keys().collect())
    }

    fn read<T: DeserializeOwned + Serialize>(&mut self, family: &'static str, path: &Path) {
        let Ok(bytes) = fs::read(path) else {
            self.note("file");
            return;
        };
        match decode::<T>(&bytes) {
            Ok(value) => {
                *self.decoded.entry(family).or_default() += 1;
                let written = assert_ok!(serde_json::to_vec(&value), "{family}: a decoded value can be written");
                assert!(
                    same_members(&written, &bytes),
                    "{family}: a member was lost or changed on the way through"
                );
            }
            Err(error) => {
                let category = match error {
                    DecodeError::NotUtf8 { .. } => "not-utf8",
                    DecodeError::Structure { .. } => "structure",
                    DecodeError::Trailing { .. } => "trailing",
                    DecodeError::Shape { .. } => "shape",
                };
                *self.refused.entry((family, category)).or_default() += 1;
            }
        }
    }

    fn run_dir(&mut self, run: &Path) {
        let files = self.files(run);
        let Some(manifest) = files.iter().find(|p| p.ends_with("manifest.json")).cloned() else {
            self.outside("run without a manifest");
            return;
        };
        let names = self.object_names(&manifest).unwrap_or_default();
        let advise = names.iter().any(|n| n == "code_sha256") && names.iter().any(|n| n == "clauses_source");
        let compare = names.iter().any(|n| n == "fixture") && names.iter().any(|n| n == "configuration") && !advise;
        if !advise && !compare {
            self.outside("manifest of another tool");
            return;
        }
        if advise {
            self.read::<RunManifest>("run-manifest", &manifest);
        } else {
            self.read::<CompareManifest>("compare-manifest", &manifest);
        }
        for path in &files {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            match (name, advise) {
                ("status.json", true) => self.read::<RunStatus>("status", path),
                ("report.json", true) => self.read::<Report>("report", path),
                ("clauses.json" | "supplied-clauses.json", true) => self.read::<Clauses>("clauses", path),
                ("approval.json", true) => self.read::<Approval>("approval", path),
                ("evidence.json", true) => self.read::<Vec<LegacyEvidence>>("evidence", path),
                ("context-blocks.json", true) => self.read::<Vec<String>>("context-blocks", path),
                ("segments.json", true) => self.read::<Vec<Segment>>("segments", path),
                ("results.json", false) => self.read::<CandidateResults>("candidate-results", path),
                ("manifest.json", _) => {}
                _ => self.summary_or_outside(path),
            }
        }
        for call in self.dirs(run) {
            self.call_dir(&call, advise);
        }
    }

    /// `result.json` of every call in a run of a known kind. `proposal.json` is an extraction in an advise run's
    /// `extraction-*` directory and an assessment anywhere else.
    fn call_dir(&mut self, call: &Path, advise: bool) {
        let name = call
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        for path in self.files(call) {
            match path.file_name().and_then(|n| n.to_str()).unwrap_or_default() {
                "result.json" => self.read::<CallResult>("call-result", &path),
                "proposal.json" if advise && name.starts_with("extraction-") => {
                    self.read::<ExtractionProposal>("extraction", &path);
                }
                // An earlier writer made one extraction call, named `extraction`, whose proposal is a clause file.
                "proposal.json" if advise && name == "extraction" => {
                    self.read::<Clauses>("single-call-extraction", &path)
                }
                "proposal.json" => self.read::<Assessment>("assessment", &path),
                _ => self.outside("file in a call directory"),
            }
        }
    }

    fn fixture_dir(&mut self, dir: &Path) {
        let files = self.files(dir);
        let Some(fixture) = files.iter().find(|p| p.ends_with("fixture.json")).cloned() else {
            self.outside("fixture directory without fixture.json");
            return;
        };
        self.read::<Fixture>("fixture", &fixture);
        for path in &files {
            match path.file_name().and_then(|n| n.to_str()).unwrap_or_default() {
                "clauses.json" => self.read::<Clauses>("fixture-clauses", path),
                "approval.json" => self.read::<Approval>("fixture-approval", path),
                "reference.json" => self.read::<Reference>("reference", path),
                "expected.json" => self.read::<ExpectedProbes>("expected-probes", path),
                "fixture.json" => {}
                _ => self.summary_or_outside(path),
            }
        }
    }

    fn summary_or_outside(&mut self, path: &Path) {
        let names = self.object_names(path).unwrap_or_default();
        let has = |wanted: &[&str]| wanted.iter().all(|w| names.iter().any(|n| n == w));
        if has(&["by_category", "kind"]) {
            self.read::<ProbeSummary>("probe-summary", path);
        } else if has(&["exact_agreement", "mean_citation_jaccard"]) {
            self.read::<ReferenceSummary>("reference-summary", path);
        } else {
            self.outside("file of another shape");
        }
    }
}

/// What a private root must yield for the scan to count as evidence: at least this many files of each family. Set from
/// a measured scan of the owner's root, so a regression in discovery shows as a shortfall, not as a smaller green count.
pub fn expected_inventory() -> BTreeMap<&'static str, usize> {
    [
        ("run-manifest", 10),
        ("compare-manifest", 4),
        ("status", 10),
        ("report", 5),
        ("clauses", 13),
        ("fixture", 4),
        ("fixture-clauses", 4),
        ("reference", 2),
        ("expected-probes", 2),
        ("probe-spec", 2),
        ("candidate-results", 4),
        ("probe-summary", 4),
        ("call-result", 251),
        ("assessment", 161),
        ("extraction", 11),
        ("single-call-extraction", 1),
    ]
    .into_iter()
    .collect()
}

pub fn assert_inventory(scan: &Scan) {
    for (family, minimum) in expected_inventory() {
        let found = scan.decoded.get(family).copied().unwrap_or(0);
        assert!(
            found >= minimum,
            "{family}: expected at least {minimum} decoded files, found {found}"
        );
    }
    assert!(
        scan.refused.is_empty(),
        "a saved file in a supported family was refused: {:?}",
        scan.refused
    );
    assert!(
        scan.unreadable.is_empty(),
        "files or directories could not be read: {:?}",
        scan.unreadable
    );
    assert_some!(scan.decoded.values().next());
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn write(root: &Path, relative: &str, text: &str) {
        let path = root.join(relative);
        assert_ok!(fs::create_dir_all(assert_some!(path.parent())));
        assert_ok!(fs::write(path, text));
    }

    const D: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn config() -> &'static str {
        r#"{"base_url":"u","requested_model":"m","remote_consent":false,"credential_used":false,"custom_ca":false,"engagement":null,"idle_timeout":1,"max_tokens":128,"temperature":null,"response_format":"json_object"}"#
    }

    fn advise_manifest() -> String {
        format!(
            r#"{{"configuration":{},"baseline_sha256":"{D}","jd_sha256":"{D}","code_sha256":{{"a.py":"{D}"}},"clauses_source":"owner-reviewed-file","mode":"m"}}"#,
            config()
        )
    }

    #[test]
    fn the_scan_counts_what_it_reads_and_what_it_does_not() {
        let dir = assert_ok!(tempfile::tempdir());
        let root = dir.path();
        write(root, "runs/a/manifest.json", &advise_manifest());
        write(
            root,
            "runs/a/status.json",
            r#"{"status":"running","usable_as_complete_proposal":false}"#,
        );
        write(
            root,
            "runs/a/extraction-001/proposal.json",
            r#"{"clauses":[{"segment":1,"quote":"q","importance":"required"}]}"#,
        );
        write(
            root,
            "runs/a/r001/proposal.json",
            r#"{"id":"r001","support":"direct","citations":[],"boundary":"b"}"#,
        );
        write(root, "runs/a/r001/request.json", "{}");
        write(root, "runs/other/manifest.json", r#"{"model":"x"}"#);
        write(root, "runs/bare/status.json", "{}");
        let scan = Scan::run(root);
        assert_eq!(scan.decoded.get("run-manifest"), Some(&1));
        assert_eq!(scan.decoded.get("status"), Some(&1));
        assert_eq!(scan.decoded.get("extraction"), Some(&1));
        assert_eq!(scan.decoded.get("assessment"), Some(&1));
        assert_eq!(scan.outside.get("manifest of another tool"), Some(&1));
        assert_eq!(scan.outside.get("run without a manifest"), Some(&1));
        assert_eq!(scan.outside.get("file in a call directory"), Some(&1));
        assert_eq!(scan.refused, BTreeMap::new());
        // The tree has no `fixtures` directory, and the scan says so; nothing else is unreadable.
        assert_eq!(scan.unreadable, BTreeMap::from([("directory", 1)]));
    }

    #[test]
    fn a_malformed_supported_file_is_a_refusal_not_a_silent_skip() {
        let dir = assert_ok!(tempfile::tempdir());
        let root = dir.path();
        write(root, "runs/a/manifest.json", &advise_manifest());
        // The status is missing its required member, and the call result has no `usage`.
        write(root, "runs/a/status.json", r#"{"status":"complete-unreviewed"}"#);
        write(
            root,
            "runs/a/r001/result.json",
            r#"{"served_model":"m","seconds":1.0,"status":"ok"}"#,
        );
        write(
            root,
            "runs/a/r002/proposal.json",
            r#"{"id":"r002","support":"direct","citations":[],"boundary":"b","extra":1}"#,
        );
        let scan = Scan::run(root);
        assert_eq!(scan.refused.get(&("status", "shape")), Some(&1));
        assert_eq!(scan.refused.get(&("call-result", "shape")), Some(&1));
        assert_eq!(scan.refused.get(&("assessment", "shape")), Some(&1));
        assert!(
            std::panic::catch_unwind(|| assert_inventory(&scan)).is_err(),
            "a refusal must fail the inventory"
        );
    }

    #[test]
    fn missing_directories_and_unreadable_manifests_are_reported() {
        let dir = assert_ok!(tempfile::tempdir());
        let scan = Scan::run(dir.path());
        assert_eq!(scan.unreadable.get("directory"), Some(&2), "{:?}", scan.unreadable);
        assert!(std::panic::catch_unwind(|| assert_inventory(&scan)).is_err());
    }

    #[test]
    fn a_probe_spec_with_extra_members_is_still_found_and_a_malformed_one_refused() {
        let dir = assert_ok!(tempfile::tempdir());
        let root = dir.path();
        write(
            root,
            "fixtures/spec-a.json",
            r#"{"probes":[{"id":"r001","text":"t","category":"trap"}],"note":"kept"}"#,
        );
        write(
            root,
            "fixtures/spec-b.json",
            r#"{"probes":[{"id":"r001","text":"t","category":"unknown"}]}"#,
        );
        let scan = Scan::run(root);
        assert_eq!(scan.decoded.get("probe-spec"), Some(&1));
        assert_eq!(scan.refused.get(&("probe-spec", "shape")), Some(&1));
    }

    #[test]
    fn a_quote_that_is_the_word_id_does_not_change_how_an_extraction_is_read() {
        let dir = assert_ok!(tempfile::tempdir());
        write(dir.path(), "runs/a/manifest.json", &advise_manifest());
        write(
            dir.path(),
            "runs/a/extraction-001/proposal.json",
            r#"{"clauses":[{"segment":1,"quote":"id","importance":"context"}]}"#,
        );
        assert_eq!(Scan::run(dir.path()).decoded.get("extraction"), Some(&1));
    }

    #[test]
    fn a_diagnostic_never_shows_a_path_or_a_value() {
        let dir = assert_ok!(tempfile::tempdir());
        write(dir.path(), "runs/secret-run-name/manifest.json", &advise_manifest());
        write(
            dir.path(),
            "runs/secret-run-name/status.json",
            r#"{"status":"private-label"}"#,
        );
        let scan = Scan::run(dir.path());
        let shown = format!(
            "{:?}{:?}{:?}{:?}",
            scan.decoded, scan.refused, scan.outside, scan.unreadable
        );
        assert!(
            !shown.contains("secret-run-name") && !shown.contains("private-label"),
            "{shown}"
        );
    }
}
