//! ABOUTME: Entry point for the `resume-advisor` binary.
//! ABOUTME: Reads a baseline, projects it once, and writes JSON to stdout; all work and policy live in the library.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use advisor::evidence;
use clap::{Parser, Subcommand};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Parser)]
#[command(
    name = "resume-advisor",
    version,
    about = "Proposes resume wording from cited evidence, using a local model"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Export the citable statements, their context and local provenance. Contains private resume text.
    Evidence {
        /// The annotated resume baseline. Output is JSON on stdout; redirect it only to a private location.
        baseline: PathBuf,
        /// Export only what a model may see: no provenance, excluded-item report or source path.
        #[arg(long)]
        model_payload: bool,
    },
}

#[derive(Debug, Snafu)]
enum Error {
    #[snafu(display("cannot read the baseline"))]
    ReadBaseline { source: resume::error::Error },
    #[snafu(display("cannot export evidence"))]
    Project { source: evidence::EvidenceError },
    #[snafu(display("cannot write evidence as JSON"))]
    Serialize { source: serde_json::Error },
    #[snafu(display("cannot deliver evidence on stdout"))]
    Deliver { source: io::Error },
}

fn run(cli: Cli, stdout: &mut impl Write) -> Result<(), Error> {
    let Command::Evidence {
        baseline,
        model_payload,
    } = cli.command;
    let text = resume::input::read_text(&baseline, &baseline).context(ReadBaselineSnafu)?;
    let evidence = evidence::project(&text).context(ProjectSnafu)?;
    // Serialize first. A projection or serialization failure must leave stdout empty.
    let bytes = if model_payload {
        evidence.model_artifact().context(ProjectSnafu)?.bytes().to_vec()
    } else {
        serde_json::to_vec(&evidence).context(SerializeSnafu)?
    };
    stdout.write_all(&bytes).context(DeliverSnafu)?;
    stdout.write_all(b"\n").context(DeliverSnafu)?;
    stdout.flush().context(DeliverSnafu)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli, &mut io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Error::Deliver { source }) if source.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            // `resume`'s input errors already display their source; do not print that cause twice.
            if let Error::ReadBaseline { source } = error {
                eprintln!("error: {source}");
            } else {
                eprintln!("error: {}", snafu::Report::from_error(error));
            }
            ExitCode::FAILURE
        }
    }
}
