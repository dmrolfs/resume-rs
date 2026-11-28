//! Saved metadata is inert: reading a gateway-era endpoint never authorises a connection.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::open::open_object;
use super::{Approval, Assessment, Clause, Field, JsonValue};
use crate::digest::Sha256Digest;
use crate::text::CharOffset;

open_object! {
    /// A legacy evidence row. Heading strings are opaque; no v0/v1 conversion happens here.
    pub struct LegacyEvidence {
        id: String = req,
        text: String = req,
        section: String = req,
        role: String = req,
        era: String = req,
    }
}

open_object! {
    /// An unchanged JD span. Both positions count Unicode characters.
    pub struct Segment {
        id: u32 = req,
        start: CharOffset = req,
        end: CharOffset = req,
        text: String = req,
    }
}

open_object! {
    /// Public configuration recorded by Python, not executable configuration.
    /// Timeout fields count whole seconds; temperature is a required nullable number.
    pub struct SavedConfiguration {
        base_url: String = req,
        requested_model: String = req,
        remote_consent: bool = req,
        credential_used: bool = req,
        custom_ca: bool = req,
        engagement: Option<String> = req,
        idle_timeout: u32 = req,
        total_timeout: Field<u32> = opt,
        max_tokens: u32 = req,
        temperature: Option<f64> = req,
        response_format: String = req,
    }
}

/// Where saved clauses came from. A development case is not an owner approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::Display, strum::EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ClausesSource {
    OwnerReviewedFile,
    ModelExtractedUnreviewed,
    SuppliedDevelopmentCase,
}

open_object! {
    /// `manifest.json` of an advise run, including historical missing metadata.
    pub struct RunManifest {
        configuration: SavedConfiguration = req,
        configuration_file_sha256: Field<Option<Sha256Digest>> = opt,
        runtime: Field<JsonValue> = opt,
        baseline_sha256: Sha256Digest = req,
        jd_sha256: Sha256Digest = req,
        code_sha256: BTreeMap<String, Sha256Digest> = req,
        clauses_source: ClausesSource = req,
        approval: Field<Option<Approval>> = opt,
        mode: String = req,
    }
}

open_object! {
    /// Manifest of a fixture replay, not the manifest of an extraction/assessment run.
    pub struct CompareManifest {
        fixture: String = req,
        configuration: SavedConfiguration = req,
        system_prompt_sha256: Field<Sha256Digest> = opt,
        system_prompt_source: Field<String> = opt,
    }
}

/// The saved outcome of a call. `ok` is read-only compatibility for the early extraction experiments, not a promise
/// that a proposal passed the current validator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::Display, strum::EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum CallStatus {
    StructureValid,
    Ok,
}

open_object! {
    /// `result.json` of one call. Usage is required but may be null; its value is provider-defined JSON.
    /// `seconds` is an elapsed duration, potentially fractional, not a timeout.
    pub struct CallResult {
        served_model: String = req,
        usage: Option<JsonValue> = req,
        seconds: f64 = req,
        status: CallStatus = req,
    }
}

open_object! {
    /// `report.json`. A decoded report is still an unreviewed proposal.
    pub struct Report {
        status: String = req,
        coverage_review: String = req,
        semantic_review: String = req,
        skipped_context: Field<Vec<String>> = opt,
        clauses: Vec<Clause> = req,
        assessments: Vec<Assessment> = req,
        cited_wordings: Vec<String> = req,
        warning: String = req,
    }
}

open_object! {
    /// `reference.json` from a frozen fixture. Python's discovery helper may have found no served model.
    pub struct Reference {
        served_model: Option<String> = req,
        configuration: SavedConfiguration = req,
        assessments: Vec<Assessment> = req,
    }
}
