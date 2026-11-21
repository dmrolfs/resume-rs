//! ABOUTME: Typesets a resume with an embedded Typst compiler and a style, producing PDF bytes.
//! ABOUTME: Compile and PDF export are separate steps so layout can be inspected without writing a file.

mod diag;
pub mod fonts;
pub mod geometry;
pub(crate) mod pdf;
pub mod probe;
pub(crate) mod world;

use std::any::Any;
use std::sync::Arc;

use snafu::ResultExt;
use typst::diag::Warned;
use typst_layout::PagedDocument;

use crate::diagnostics::Diagnostic;
use crate::error::{
    CompileSnafu, Error, ExportPdfSnafu, SerializeResumeSnafu, SpawnTypesetThreadSnafu, Stage, StyleSources,
    TypesetPanickedSnafu,
};
use crate::model::Resume;
use crate::model::options::Paper;
use crate::style::StyleSource;

use self::fonts::FontSet;
use self::world::ResumeWorld;

/// Stack for the thread that compiles and exports.
///
/// Typst parses and evaluates recursively. This is twice the 8 MiB macOS main-thread stack and
/// far above the 1 MiB Windows default; the memory is reserved, not committed. Page layout runs
/// on Typst's own worker threads, which this does not cover.
const TYPESET_STACK_BYTES: usize = 16 * 1024 * 1024;

/// What a render may vary: only the paper size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct RenderOptions {
    pub paper: Paper,
}

impl RenderOptions {
    #[must_use]
    pub fn new(paper: Paper) -> Self {
        Self { paper }
    }
}

/// Compiles resumes with a style; cheap to clone and to share between renders.
#[derive(Clone)]
pub struct Typesetter {
    fonts: Arc<FontSet>,
}

impl std::fmt::Debug for Typesetter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Typesetter").field("fonts", &self.fonts).finish()
    }
}

impl Typesetter {
    #[must_use]
    pub fn new(fonts: Arc<FontSet>) -> Self {
        Self { fonts }
    }

    /// Evicts memoized results not used in the last `max_age` compiles.
    ///
    /// Typst memoizes layout work in a process-wide cache that never shrinks on its own, so a
    /// long-running caller such as `watch` should call this after each build. The cache is shared by every
    /// `Typesetter` in the process, so this trims all of them; that only costs speed, never results.
    pub fn evict_cache(max_age: usize) {
        typst::comemo::evict(max_age);
    }

    /// Compiles `resume` with `style` into a laid-out document.
    ///
    /// # Errors
    /// - `Error::SerializeResume` - the resume cannot be written as JSON
    /// - `Error::SpawnTypesetThread` - the compile thread could not start
    /// - `Error::TypesetPanicked` - Typst panicked
    /// - `Error::Compile` - the style has errors; carries every diagnostic and the style text
    pub fn compile(&self, resume: &Resume, style: &StyleSource, options: RenderOptions) -> Result<Compiled, Error> {
        let json = resume_json(resume, false)?;
        let world = ResumeWorld::new(style.clone(), Arc::clone(&self.fonts), json, options.paper);
        let Warned { output, warnings } = isolated(Stage::Compile, || typst::compile::<PagedDocument>(&world))?;
        let sources = world.served_sources();
        match output {
            Ok(document) => Ok(Compiled {
                document,
                warnings: diag::convert(&world, warnings),
                sources,
                world,
            }),
            Err(errors) => CompileSnafu {
                diagnostics: diag::convert(&world, errors.into_iter().chain(warnings)),
                sources,
            }
            .fail(),
        }
    }
}

/// The resume as the style reads it, as `json("/resume.json")`: the one serialization, so what `export` writes
/// for a preview cannot differ from what the compiler is given. `pretty` indents it and ends it with a newline,
/// for a file a person may open; a style reads both the same.
///
/// # Errors
/// `Error::SerializeResume` - the resume cannot be written as JSON.
pub fn resume_json(resume: &Resume, pretty: bool) -> Result<Vec<u8>, Error> {
    let mut json = if pretty {
        serde_json::to_vec_pretty(resume)
    } else {
        serde_json::to_vec(resume)
    }
    .context(SerializeResumeSnafu)?;
    if pretty {
        json.push(b'\n');
    }
    Ok(json)
}

/// A compiled, laid-out document.
pub struct Compiled {
    document: PagedDocument,
    warnings: Vec<Diagnostic>,
    sources: StyleSources,
    /// Kept so export errors can be located in the style files, like compile errors.
    world: ResumeWorld,
}

impl std::fmt::Debug for Compiled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Compiled")
            .field("pages", &self.page_count())
            .field("warnings", &self.warnings.len())
            .field("sources", &self.sources.len())
            .finish()
    }
}

impl Compiled {
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.document.pages().len()
    }

    /// Typst warnings from the compile.
    #[must_use]
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }

    /// The text of every style file Typst read, for showing excerpts.
    #[must_use]
    pub fn sources(&self) -> &StyleSources {
        &self.sources
    }

    /// The font roles the style declared in `theme.fonts`, in declaration order.
    ///
    /// # Errors
    /// `Error::Probe` when `<resume-fonts>` is not a dictionary of `(preferred, fallback, metric-compatible)`.
    pub fn font_roles(&self) -> Result<Vec<fonts::FontRole>, Error> {
        probe::font_roles(&self.document)
    }

    /// Where the page body is on each page and how far its content reaches, in document order.
    #[must_use]
    pub fn geometry(&self) -> Vec<geometry::PageGeometry> {
        geometry::page_geometry(&self.document)
    }

    /// Where each role and era heading landed, in document order.
    ///
    /// # Errors
    /// `Error::Probe` when a probe's metadata does not have the documented shape.
    pub fn probes(&self) -> Result<Vec<probe::Probe>, Error> {
        probe::probes(&self.document)
    }

    /// Writes the PDF. `ident` should be stable for the same logical document, such as the output file stem.
    ///
    /// # Errors
    /// - `Error::SpawnTypesetThread` / `Error::TypesetPanicked` - as for `compile`
    /// - `Error::ExportPdf` - the PDF could not be produced
    pub fn export_pdf(&self, ident: &str) -> Result<Vec<u8>, Error> {
        isolated(Stage::ExportPdf, || pdf::export(&self.document, ident))?.map_err(|errors| {
            ExportPdfSnafu {
                diagnostics: diag::convert(&self.world, errors),
                sources: self.sources.clone(),
            }
            .build()
        })
    }
}

/// Runs `work` on a named, large-stack thread so a Typst panic becomes an error, not an abort.
fn isolated<T: Send>(stage: Stage, work: impl FnOnce() -> T + Send) -> Result<T, Error> {
    std::thread::scope(|scope| {
        let handle = std::thread::Builder::new()
            .name("resume-typeset".to_string())
            .stack_size(TYPESET_STACK_BYTES)
            .spawn_scoped(scope, work)
            .context(SpawnTypesetThreadSnafu)?;
        handle.join().map_err(|payload| {
            TypesetPanickedSnafu {
                stage,
                message: panic_message(payload.as_ref()),
            }
            .build()
        })
    })
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(ToString::to_string)
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a non-string panic payload".to_string())
}

#[cfg(test)]
mod geometry_tests;

#[cfg(test)]
mod pragmatic_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};

    #[test]
    fn a_panic_becomes_a_typeset_error_with_its_message() {
        let err = assert_err!(isolated(Stage::Compile, || -> () { panic!("boom {}", 7) }));
        match err {
            Error::TypesetPanicked { stage, message } => {
                assert_eq!(stage, Stage::Compile);
                assert_eq!(message, "boom 7");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn a_str_panic_payload_is_reported() {
        let err = assert_err!(isolated(Stage::ExportPdf, || -> () { panic!("plain") }));
        assert!(matches!(err, Error::TypesetPanicked { stage: Stage::ExportPdf, ref message } if message == "plain"));
    }

    #[test]
    fn isolated_work_runs_on_the_named_thread() {
        let name = assert_ok!(isolated(Stage::Compile, || std::thread::current()
            .name()
            .map(str::to_string)));
        assert_eq!(name.as_deref(), Some("resume-typeset"));
    }
}
