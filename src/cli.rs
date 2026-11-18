//! ABOUTME: Command-line argument definitions for the `resume` binary, built with clap derive.
//! ABOUTME: Commands are added here as their tasks land; parsing errors exit with code 2.

use clap::Parser;

/// Command-line arguments for `resume`.
#[derive(Debug, Parser)]
#[command(
    name = "resume",
    // Windows starts the binary as `resume.exe` and clap derives its display name from argv[0],
    // so pin it; otherwise help would print `Usage: resume.exe`.
    bin_name = "resume",
    version,
    about = "Typesets Markdown resumes into print-quality PDFs."
)]
pub struct Cli {}
