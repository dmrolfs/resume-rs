//! ABOUTME: `resume doctor`: content advice, setup checks, fixes, and the report that groups them by category.
//! ABOUTME: Everything here reads a resume and reports; only `fix` changes a file, and only through `output::Writer`.

pub mod content;
pub mod fix;
pub mod report;
pub mod setup;
