//! ABOUTME: The legacy "v0" file layouts the Python spike wrote, as Rust types.
//! ABOUTME: Exact-key files refuse unknown members; the rest keep them. Decoding is shape only, never validity or approval.

use serde::{Deserialize, Serialize};

use super::Field;
use crate::digest::Sha256Digest;
use crate::text::CharOffset;

/// How much a job-description clause matters. `context` describes the company or role, not an applicant requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::Display, strum::EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum Importance {
    Required,
    Preferred,
    Context,
}

/// A claim a model made about a clause. `Display` is the wire spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::Display, strum::EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum Support {
    Direct,
    Adjacent,
    NotEstablished,
}

/// One requirement from a job description, as reviewed by the owner: `clauses.json` and `supplied-clauses.json`.
///
/// `source_start` is missing in older files and may be `null`; Python resolves both through the clause's parent when
/// that text occurs once. The unit is characters of the job description, never bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clause {
    pub id: String,
    pub quote: String,
    pub parent: String,
    pub importance: Importance,
    #[serde(default, skip_serializing_if = "Field::is_missing")]
    pub source_start: Field<Option<CharOffset>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clauses {
    pub clauses: Vec<Clause>,
}

/// A clause a model proposed from one numbered segment of the job description (`proposal.json` of an extraction call).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractedClause {
    pub segment: u32,
    pub quote: String,
    pub importance: Importance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractionProposal {
    pub clauses: Vec<ExtractedClause>,
}

/// An exact quote from one resume statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Citation {
    pub id: String,
    pub quote: String,
}

/// A model's answer for one clause: `proposal.json` of an assessment call, and each row of `report.json` and
/// `reference.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub id: String,
    pub support: Support,
    pub citations: Vec<Citation>,
    pub boundary: String,
}

/// The owner's attestation that a reviewed clause file is right for one job description: `approval.json` and the
/// `approval` member of a manifest.
///
/// This type only reads one. It is never written by this program, and a value of it is not an approval: whether it
/// matches the bytes of the files it names is checked against those bytes, by the approval check (rsm-eam.5.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub reviewer: String,
    pub jd_sha256: Sha256Digest,
    pub clauses_sha256: Sha256Digest,
}
