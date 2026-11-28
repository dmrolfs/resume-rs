//! ABOUTME: Saved run state, fixtures, probes and comparison results, as the spike's two writers produce them.
//! ABOUTME: Each family reads its own layout; no shape-guessing fallback. A decoded file is a claim, not a verdict.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::open::{open_object, tagged_object};
use super::{Field, Importance, Support};
use crate::digest::Sha256Digest;

open_object! {
    /// An advise run that has started.
    pub struct Running {
        usable_as_complete_proposal: bool = req,
    }
}

open_object! {
    /// Clauses were extracted and wait for the owner's review.
    pub struct AwaitingClauseReview {
        inputs_unchanged: bool = req,
        clauses_extracted: u32 = req,
    }
}

open_object! {
    /// Every clause was assessed. The proposal is still unreviewed.
    pub struct CompleteUnreviewed {
        inputs_unchanged: bool = req,
        clauses_assessed: u32 = req,
    }
}

open_object! {
    /// A failed run. `stage` and `category` are missing in older files; `http_status` may be missing or null.
    /// Both strings are labels, not a closed list, so an unlisted one is kept as written.
    pub struct Failed {
        usable_as_complete_proposal: bool = req,
        stage: Field<String> = opt,
        category: Field<String> = opt,
        http_status: Field<Option<u16>> = opt,
        inputs_changed_or_unreadable: Field<bool> = opt,
    }
}

tagged_object! {
    /// `status.json`.
    pub enum RunStatus tag "status" {
        Running(Running) = "running",
        AwaitingClauseReview(AwaitingClauseReview) = "awaiting-clause-review",
        CompleteUnreviewed(CompleteUnreviewed) = "complete-unreviewed",
        Failed(Failed) = "failed",
    }
}

/// What a probe checks. The categories fix which support labels count as acceptable.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, strum::Display, strum::EnumString,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ProbeCategory {
    Negative,
    NearMiss,
    Trap,
    Control,
}

open_object! {
    /// One probe of a private spec. Every field after `category` is optional in the file; the builder supplies defaults,
    /// not this reader. `related_baseline` is an annotation Python ignores.
    pub struct ProbeSpecRow {
        id: String = req,
        text: String = req,
        category: ProbeCategory = req,
        importance: Field<Importance> = opt,
        label: Field<String> = opt,
        absent_terms: Field<Vec<String>> = opt,
        citations: Field<Vec<String>> = opt,
        note: Field<String> = opt,
        related_baseline: Field<String> = opt,
    }
}

open_object! {
    /// `probes-spec-*.json`.
    pub struct ProbeSpec {
        probes: Vec<ProbeSpecRow> = req,
    }
}

open_object! {
    /// A probe's expected answer in a probe fixture's `expected.json`, keyed by clause id.
    pub struct ExpectedProbe {
        label: String = req,
        category: ProbeCategory = req,
        acceptable: Vec<Support> = req,
        citations: Vec<String> = req,
        note: String = req,
    }
}

/// `expected.json` of a probe fixture.
pub type ExpectedProbes = BTreeMap<String, ExpectedProbe>;

/// The one value `kind` takes in a reference fixture, when present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReferenceKind {
    Reference,
}

/// The one value `kind` takes in a probe fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeKind {
    Probe,
}

open_object! {
    /// `fixture.json` of a frozen run. `kind` is absent in every file written so far; absent means reference.
    pub struct ReferenceFixture {
        kind: Field<ReferenceKind> = opt,
        files: BTreeMap<String, Sha256Digest> = req,
        source_run: String = req,
        created: String = req,
    }
}

open_object! {
    /// `fixture.json` of a probe fixture. Its labels are the assistant's expectations, not the owner's approval.
    pub struct ProbeFixture {
        kind: ProbeKind = req,
        files: BTreeMap<String, Sha256Digest> = req,
        labels: String = req,
        created: String = req,
    }
}

/// `fixture.json`: a reference fixture, or a probe fixture when `kind` says so.
#[derive(Debug, Clone)]
pub enum Fixture {
    Reference(ReferenceFixture),
    Probe(ProbeFixture),
}

impl<'de> Deserialize<'de> for Fixture {
    /// A missing `kind` is a reference fixture. A present one must be `reference` or `probe`, so `null`, a number or an
    /// unknown word is refused instead of being read as a reference.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut members = super::open::Members::read(deserializer)?;
        let fixture = match members
            .peek::<String>("kind")
            .map_err(serde::de::Error::custom)?
            .as_deref()
        {
            Some("probe") => ProbeFixture::from_members(&mut members).map(Self::Probe),
            _ => ReferenceFixture::from_members(&mut members).map(Self::Reference),
        };
        fixture.map_err(serde::de::Error::custom)
    }
}

impl Serialize for Fixture {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Reference(fixture) => fixture.serialize(serializer),
            Self::Probe(fixture) => fixture.serialize(serializer),
        }
    }
}

open_object! {
    /// A clause a candidate answered.
    pub struct AnsweredClause {
        support: Support = req,
        citations: Vec<String> = req,
    }
}

open_object! {
    /// A clause the candidate could not answer. `category` is a fixed label such as `validation`.
    pub struct FailedClause {
        category: String = req,
    }
}

tagged_object! {
    /// One value of a candidate's `results.json`, keyed by clause id.
    pub enum ClauseResult tag "status" {
        Ok(AnsweredClause) = "ok",
        Failed(FailedClause) = "failed",
    }
}

/// `results.json` of a candidate run.
pub type CandidateResults = BTreeMap<String, ClauseResult>;

open_object! {
    /// How many probes of one category there were, were valid, and came out as expected.
    pub struct CategoryCounts {
        n: u32 = req,
        valid: u32 = req,
        as_expected: u32 = req,
    }
}

/// A probe summary's marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeSummaryKind {
    Probe,
}

open_object! {
    /// The summary of a probe comparison. `direct_on_traps` is an earlier writer's name for what the current writer
    /// calls `direct_on_near_misses_or_traps`. They are kept as two members: the older one is not known to count the
    /// same cases, and no reading of the old name should be assumed.
    /// The `rejected_*` members are missing in older summaries; the current writer always writes them.
    pub struct ProbeSummary {
        kind: ProbeSummaryKind = req,
        by_category: BTreeMap<ProbeCategory, CategoryCounts> = req,
        failed: BTreeMap<String, String> = req,
        rejected_claiming_direct: Field<Vec<String>> = opt,
        rejected_overclaims_on_negatives_near_misses_traps: Field<BTreeMap<String, (ProbeCategory, Support)>> = opt,
        false_support_on_negatives: BTreeMap<String, Support> = req,
        direct_on_near_misses_or_traps: Field<BTreeMap<String, (ProbeCategory, Support)>> = opt,
        direct_on_traps: Field<BTreeMap<String, (ProbeCategory, Support)>> = opt,
        controls_not_direct: BTreeMap<String, Support> = req,
        controls_direct_without_expected_citation: Vec<String> = req,
        note: String = req,
    }
}

open_object! {
    /// The summary of a comparison with a frozen reference. The Jaccard mean is `null` when no pair could be compared.
    pub struct ReferenceSummary {
        clauses: u32 = req,
        valid_answers: u32 = req,
        failed: BTreeMap<String, String> = req,
        rejected_claiming_direct: Vec<String> = req,
        exact_agreement: u32 = req,
        candidate_stronger_than_reference: BTreeMap<String, (Support, Support)> = req,
        candidate_weaker_than_reference: BTreeMap<String, (Support, Support)> = req,
        mean_citation_jaccard: Option<f64> = req,
    }
}
