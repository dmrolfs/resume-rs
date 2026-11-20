//! ABOUTME: Library for resume-rs: parses Markdown resumes and typesets them to PDF with embedded Typst.
//! ABOUTME: The `resume` binary is a thin CLI over this crate's public API.
//!
//! Typesets Markdown resumes into print-quality PDFs.
//!
//! The pipeline parses a resume written in Markdown into a validated, typed model, reports
//! located diagnostics for anything that doesn't follow the conventions, and renders the model
//! through a runtime-loadable Typst style.

pub mod cli;
pub mod commands;
pub mod diagnostics;
pub mod doctor;
pub mod error;
pub mod layout;
pub mod markdown;
pub mod model;
pub mod output;
pub mod pipeline;
pub mod style;
pub mod typeset;
