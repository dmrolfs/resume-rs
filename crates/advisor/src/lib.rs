//! ABOUTME: The resume advisor: proposes resume wording for a job description from cited evidence.
//! ABOUTME: A separate package so network-capable code can never link into the `resume` package.

/// Strict readers for the files the Python spike saved (clauses, approvals, results, fixtures).
pub mod contract;
/// SHA-256 identity of exact file bytes.
pub mod digest;
/// Evidence projection from the real resume parser.
pub mod evidence;
/// Unicode character offsets, trimming and counting that match Python's `str`.
pub mod text;
