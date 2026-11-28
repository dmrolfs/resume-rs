//! ABOUTME: Typed readers for the files the Python spike saved: clauses, approvals, results, fixtures, probes, summaries.
//! ABOUTME: Strict by file family; decoding is shape only, never validity, and never an approval.
//!
//! # The rules
//!
//! - Read every artifact with [`decode`](crate::contract::decode), from the bytes of the file. It refuses a repeated member name at any depth,
//!   a decimal outside the range of an `f64` (`1e999`), a lone surrogate, invalid UTF-8, nesting past 127 containers and trailing data.
//!   Never read an artifact into `serde_json::Value`: with `raw_value` on, it takes an object whose first key is
//!   `$serde_json::private::RawValue` for a raw value.
//! - Files the Python code checks with an exact key set (clauses, extraction rows, assessments, approvals) refuse an
//!   unknown member. Metadata, manifests, statuses, fixtures, probes and summaries keep unknown members in
//!   [`Extra`](crate::contract::Extra), as written, and write them back.
//! - A missing member, a `null` and a value are three states where the file has all three: [`Field`](crate::contract::Field) keeps them apart
//!   (`source_start`, `approval`, `skipped_context`, a fixture's `kind`). A member the writer always emits but may set
//!   to `null` is `Option<T>` and required.
//! - Positions in a job description are [`CharOffset`](crate::text::CharOffset), counted in characters as Python does.
//! - A digest is [`Sha256Digest`](crate::digest::Sha256Digest) of the bytes of the file. An approval binds to those
//!   bytes, so a decoded and rewritten file is a different file. The types here only read an approval; nothing in this
//!   program writes one.
//! - Provider data this program stores but does not read (`usage`, `runtime`) is a [`JsonValue`](crate::contract::JsonValue), kept exactly as
//!   written, so a number of any length survives.
//! - A saved endpoint, model name or credential flag is data. Reading one never authorises a connection.
//!
//! The files outside these families (the spike's one-off experiments, other tools' manifests) are not read.

mod json;
mod metadata;
mod open;
mod runs;
mod v0;
pub use json::{DecodeError, Field, JsonValue, decode};
pub use metadata::*;
pub use open::Extra;
pub use runs::*;
pub use v0::*;
#[cfg(test)]
mod metadata_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod v0_tests;
