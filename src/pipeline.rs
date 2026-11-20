//! ABOUTME: The read-only part of building a resume: parse, compile, and measure the layout.
//! ABOUTME: `build` and `doctor` both start here, so what each reports about a resume cannot drift apart.

use std::path::Path;
use std::sync::Arc;

use crate::cli::RenderArgs;
use crate::error::Error;
use crate::layout::LayoutReport;
use crate::markdown::{Parsed, parse};
use crate::style::{self, StyleSource};
use crate::typeset::fonts::{FontSet, FontSetBuilder};
use crate::typeset::{Compiled, RenderOptions, Typesetter};

/// What one run shares across the files it assesses: the style, the fonts, and the typesetter.
///
/// The style and the fonts are results, not values: `build` stops the run when either failed, because it can
/// do nothing without them, while `doctor` reports the failure beside everything else it found.
pub struct Session {
    style: Result<StyleSource, Error>,
    fonts: Result<Arc<FontSet>, Error>,
    typesetter: Option<Typesetter>,
}

impl Session {
    /// Resolves the style and builds the font set once.
    ///
    /// `resume_font_path` is the value of `RESUME_FONT_PATH`, passed in so tests do not touch the process.
    #[must_use]
    pub fn open(render: &RenderArgs, resume_font_path: Option<&std::ffi::OsStr>) -> Self {
        let style = style::resolve(&render.style, render.styles_dir.as_deref());
        let builder: FontSetBuilder = render
            .font_path
            .iter()
            .fold(FontSet::builder().mode(render.fonts), |builder, dir| {
                builder.font_path(dir)
            })
            .env_paths(resume_font_path);
        let fonts = builder.build().map(Arc::new);
        let typesetter = fonts.as_ref().ok().map(|fonts| Typesetter::new(Arc::clone(fonts)));
        Self {
            style,
            fonts,
            typesetter,
        }
    }

    /// The style, or why it could not be resolved.
    ///
    /// # Errors
    /// The style does not exist, has no `style.typ`, or its styles directory cannot be used.
    pub fn style(&self) -> Result<&StyleSource, &Error> {
        self.style.as_ref()
    }

    /// The font set, or why it could not be built.
    ///
    /// # Errors
    /// A `--font-path` or `RESUME_FONT_PATH` entry cannot be read.
    pub fn fonts(&self) -> Result<&Arc<FontSet>, &Error> {
        self.fonts.as_ref()
    }

    /// The first setup failure, if the style or the fonts could not be set up.
    ///
    /// # Errors
    /// The style or the font set failed; the style is reported first.
    pub fn ready(&self) -> Result<(&StyleSource, &Arc<FontSet>), &Error> {
        Ok((self.style.as_ref()?, self.fonts.as_ref()?))
    }

    /// Hands back the session when the style and the fonts are ready, or the first failure by value, so a
    /// command that cannot go on without them can stop with that error.
    ///
    /// # Errors
    /// The style or the font set failed; the style is reported first.
    pub fn require_ready(self) -> Result<Self, Error> {
        let Self {
            style,
            fonts,
            typesetter,
        } = self;
        let style = style?;
        let fonts = fonts?;
        Ok(Self {
            style: Ok(style),
            fonts: Ok(fonts),
            typesetter,
        })
    }

    /// Parses `text` and, when it has no errors, compiles it and measures the layout. Writes nothing.
    #[must_use]
    pub fn assess(&self, file: &Path, text: &str, render: &RenderArgs) -> Assessment {
        let parsed = parse(file, text);
        let rendered = match (parsed.valid(), self.ready()) {
            (None, _) => Rendered::Skipped(Skip::FormatErrors),
            (Some(_), Err(_)) => Rendered::Skipped(Skip::Setup),
            (Some(resume), Ok((style, fonts))) => {
                let typesetter = self
                    .typesetter
                    .as_ref()
                    .expect("fonts are ready, so there is a typesetter");
                match typesetter.compile(resume, style, RenderOptions::new(render.paper)) {
                    Err(error) => Rendered::Failed(error),
                    Ok(compiled) => match LayoutReport::of(&compiled, fonts, render.max_pages) {
                        Ok(layout) => Rendered::Laid {
                            compiled: Box::new(compiled),
                            layout,
                        },
                        // The document compiled, so its warnings and style sources are still worth showing.
                        Err(error) => Rendered::LayoutFailed {
                            compiled: Box::new(compiled),
                            error,
                        },
                    },
                }
            }
        };
        Assessment { parsed, rendered }
    }
}

/// What assessing one file found: the parse, and what came of rendering it.
pub struct Assessment {
    pub parsed: Parsed,
    pub rendered: Rendered,
}

/// What rendering a file came to.
pub enum Rendered {
    /// Nothing was rendered, and why.
    Skipped(Skip),
    /// The style or the layout measurement failed; the error carries any Typst findings.
    Failed(Error),
    /// The document compiled but its layout could not be measured (the style's metadata is malformed); what
    /// compiled is kept, so its warnings and style files can be shown.
    LayoutFailed { compiled: Box<Compiled>, error: Error },
    /// The document compiled and its layout was measured.
    Laid {
        compiled: Box<Compiled>,
        layout: LayoutReport,
    },
}

/// Why nothing was rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// The resume has an error finding, so there is no model to render.
    FormatErrors,
    /// The style or the fonts could not be set up.
    Setup,
}
