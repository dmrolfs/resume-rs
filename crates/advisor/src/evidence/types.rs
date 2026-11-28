//! ABOUTME: The types of an evidence projection: what a model sees, what stays local, and why a baseline is refused.
//! ABOUTME: `ModelPayload` is built field by field from `Evidence`, so nothing local can reach a request.

use std::ops::Range;

use resume::model::{NodeKind, Status};
use serde::Serialize;
use sha2::{Digest, Sha256};
use snafu::{ResultExt, Snafu};

/// The schema name written into every artifact. A change of policy or text form is a new name.
pub const SCHEMA: &str = "resume-evidence/v1";

/// A statement the model may cite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, strum::Display, strum::EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum EvidenceKind {
    /// A paragraph or bullet the owner wrote.
    BodyStatement,
    /// An employer, an optional title and a period, and nothing inferred from them.
    EmploymentRecord,
}

/// Setting the model may read but never cite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, strum::Display, strum::EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ContextKind {
    Section,
    Era,
    /// A role's blurb, or a wording alternative of it.
    RoleDescription,
}

/// Why a node is left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, strum::Display)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ExcludedStatus {
    Draft,
    Verify,
}

impl From<Status> for ExcludedStatus {
    fn from(status: Status) -> Self {
        match status {
            Status::Draft => Self::Draft,
            Status::Verify => Self::Verify,
        }
    }
}

/// Where a record sits: the IDs of the context and the employment record that hold it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Parents {
    pub section: Option<String>,
    pub role: Option<String>,
    pub era: Option<String>,
}

/// What the model sees of one record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Record<K> {
    pub id: String,
    pub kind: K,
    pub text: String,
    /// The visible title of the section, the role (`Company` or `Company · Title`) and the era, as the model reads them.
    pub section: Option<String>,
    pub role: Option<String>,
    pub era: Option<String>,
    pub group: Option<String>,
    pub variant: Option<String>,
    pub parents: Parents,
}

/// Where a record came from. It stays on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Provenance {
    pub node_index: usize,
    pub node_kind: NodeKind,
    /// UTF-8 byte offsets into the baseline text.
    pub byte_range: Range<usize>,
    /// 1-based, counting `\n`, `\r\n` and a lone `\r`.
    pub line: usize,
    /// The bullet this one is nested in, when it is.
    pub source_parent_node: Option<usize>,
    /// The name the owner gave the item with `&name`, kept for navigation and never used as an evidence ID.
    pub item: Option<String>,
}

/// A record and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Local<K> {
    pub record: Record<K>,
    pub provenance: Provenance,
}

/// The node that makes another one unfit as evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Blocker {
    pub node_index: usize,
    pub status: ExcludedStatus,
}

/// A node kept out of the evidence, and why. Its text is not copied here; the baseline holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Excluded {
    pub id: String,
    pub provenance: Provenance,
    /// The node's own status.
    pub own: Option<ExcludedStatus>,
    /// The outermost node above it that is excluded.
    pub inherited: Option<Blocker>,
}

/// The projection of one baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Evidence {
    pub schema: &'static str,
    /// SHA-256 of the baseline's exact bytes, in lower-case hex.
    pub source_id: String,
    pub records: Vec<Local<EvidenceKind>>,
    pub context: Vec<Local<ContextKind>>,
    pub excluded: Vec<Excluded>,
}

/// What a model may be sent. Built field by field from [`Evidence`], so nothing local can leak into it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelPayload {
    pub schema: &'static str,
    pub records: Vec<Record<EvidenceKind>>,
    pub context: Vec<Record<ContextKind>>,
}

/// The exact bytes a model is sent, and their digest. The bytes are fixed once built, so the digest cannot drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelArtifact {
    bytes: Vec<u8>,
    sha256: String,
}

impl ModelArtifact {
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// SHA-256 of [`Self::bytes`], in lower-case hex.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

impl Evidence {
    /// What a model may be sent: the citable records and the context they use, field by field.
    #[must_use]
    pub fn model_payload(&self) -> ModelPayload {
        ModelPayload {
            schema: self.schema,
            records: self.records.iter().map(|l| l.record.clone()).collect(),
            context: self.context.iter().map(|l| l.record.clone()).collect(),
        }
    }

    /// The payload as one compact JSON document, and the digest of exactly those bytes.
    ///
    /// # Errors
    /// - [`EvidenceError::SerializePayload`] - the payload cannot be written as JSON.
    pub fn model_artifact(&self) -> Result<ModelArtifact, EvidenceError> {
        let bytes = serde_json::to_vec(&self.model_payload()).context(SerializePayloadSnafu)?;
        let sha256 = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        Ok(ModelArtifact { bytes, sha256 })
    }
}

/// Why a baseline cannot be projected.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(super)))]
pub enum EvidenceError {
    #[snafu(display("the baseline is {actual_bytes} bytes; the limit is {max_bytes}"))]
    InputOverLimit { actual_bytes: usize, max_bytes: usize },

    #[snafu(display("the baseline is not a valid resume ({errors} errors); `resume doctor` lists them"))]
    InvalidResume { errors: usize },

    #[snafu(display(
        "the parser reported {nodes} nodes, {ranges} ranges, {lines} lines and {nesting} nesting entries"
    ))]
    NodeMetadataLengthMismatch {
        nodes: usize,
        ranges: usize,
        lines: usize,
        nesting: usize,
    },

    #[snafu(display("node {node_index} has the source range {range:?}, which does not fit {input_bytes} bytes"))]
    InvalidSourceRange {
        node_index: usize,
        range: Range<usize>,
        input_bytes: usize,
    },

    #[snafu(display("node {node_index} starts on line {parser_line}, but its range starts on line {counted_line}"))]
    SourceLineMismatch {
        node_index: usize,
        parser_line: usize,
        counted_line: usize,
    },

    #[snafu(display("node {node_index} names {parent} as its parent, which is not an earlier node"))]
    InvalidSourceParent { node_index: usize, parent: usize },

    #[snafu(display("the era at node {node_index} does not match the eras of its role"))]
    EraCursorMismatch { node_index: usize },

    #[snafu(display("node {node_index} sits outside any {missing}"))]
    MissingStructure { node_index: usize, missing: &'static str },

    #[snafu(display("the evidence id {id} is used by nodes {first_node} and {second_node}"))]
    DuplicateId {
        id: String,
        first_node: usize,
        second_node: usize,
    },

    #[snafu(display("the generated id {id} at node {node_index} reads as a group.variant"))]
    GeneratedIdIsGrouped { id: String, node_index: usize },

    #[snafu(display("the baseline has no statement a model may cite"))]
    EmptyEvidenceSet {
        excluded_nodes: usize,
        context_nodes: usize,
    },

    #[snafu(display("the model payload cannot be written as JSON"))]
    SerializePayload { source: serde_json::Error },
}
