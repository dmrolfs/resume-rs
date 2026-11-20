//! ABOUTME: The ordered, deterministic set of fonts a render may use, and per-role font resolution.
//! ABOUTME: Bundled fonts always; in Auto mode also --font-path, RESUME_FONT_PATH, Office, and system fonts.

use std::collections::{BTreeSet, HashSet};
use std::ffi::OsStr;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use snafu::ResultExt;
use typst::foundations::Bytes;
use typst::text::{Font, FontBook, FontInfo, FontVariant};
use typst::utils::LazyHash;
use typst_kit::fonts::{FontPath, FontStore};

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::error::{Error, FontDirectorySnafu, FontScanPanickedSnafu};
pub use crate::model::options::FontOrigin;
use crate::model::options::FontsMode;

/// A face found on disk, before it has a place in the font book.
type Scanned = (FontPath, FontInfo);

/// The faces of one directory scan, or of the system scan.
struct Group {
    origin: FontOrigin,
    faces: Vec<Scanned>,
}

type SystemSource = Box<dyn Fn() -> Vec<Scanned> + Send + Sync>;

/// An ordered set of fonts shared by every render.
///
/// Order matters: Typst breaks a tie between equal faces of one family toward the earliest index, so
/// the set is built once, in a fixed order, and shared behind an `Arc`. A later face still wins when
/// it is a closer variant or a variable font, so a bundled family must ship every variant it needs.
pub struct FontSet {
    store: FontStore,
    /// Parallel to the font book: `origins[i]` is where face `i` came from.
    origins: Vec<FontOrigin>,
    mode: FontsMode,
}

impl std::fmt::Debug for FontSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontSet")
            .field("mode", &self.mode)
            .field("families", &self.families())
            .finish()
    }
}

impl FontSet {
    /// Starts a set from the bundled fonts; add layers, then [`FontSetBuilder::build`].
    #[must_use]
    pub fn builder() -> FontSetBuilder {
        FontSetBuilder::default()
    }

    /// Only the bundled fonts: typst-kit's (Libertinus Serif, New Computer Modern, DejaVu Sans Mono) and the
    /// Carlito, Gelasio, Arimo, and TeX Gyre Pagella fallbacks. The same as a [`FontsMode::Bundled`] build, without the builder.
    #[must_use]
    pub fn bundled() -> Self {
        Self::assemble(bundled_faces(), Vec::new())
    }

    /// Bundled faces first, then each group in order; a `(path, index)` already seen is skipped.
    ///
    /// Inside a group faces are sorted by canonical path, then face index, because the scans return
    /// them in directory-enumeration order, which differs between machines. The stored path is the
    /// canonical one, so a relative `--font-path` still loads after the working directory changes.
    fn assemble(bundled: Vec<(Font, FontInfo)>, groups: Vec<Group>) -> Self {
        let mut store = FontStore::new();
        let mut origins: Vec<FontOrigin> = bundled.iter().map(|_| FontOrigin::Bundled).collect();
        store.extend(bundled);
        let mut seen = HashSet::new();
        for Group { origin, faces } in groups {
            let mut keyed: Vec<_> = faces.into_iter().map(|(path, info)| (identity(&path), info)).collect();
            keyed.sort_by(|a, b| a.0.cmp(&b.0));
            for ((canonical, index), info) in keyed {
                if seen.insert((canonical.clone(), index)) {
                    store.push((FontPath { path: canonical, index }, info));
                    origins.push(origin);
                }
            }
        }
        debug_assert_eq!(
            origins.len(),
            store.book().families().map(|(_, ids)| ids.count()).sum::<usize>()
        );
        Self {
            store,
            origins,
            mode: FontsMode::Bundled,
        }
    }

    pub(crate) fn book(&self) -> &LazyHash<FontBook> {
        self.store.book()
    }

    pub(crate) fn font(&self, index: usize) -> Option<Font> {
        self.store.font(index)
    }

    /// Family names as the font book records them, ordered by their lowercase form.
    #[must_use]
    pub fn families(&self) -> Vec<String> {
        self.book().families().map(|(name, _)| name.to_string()).collect()
    }

    /// Where each face came from, in font-book order.
    #[must_use]
    pub fn origins(&self) -> &[FontOrigin] {
        &self.origins
    }

    /// Family and origin of every face, in font-book order.
    #[must_use]
    pub fn faces(&self) -> Vec<(String, FontOrigin)> {
        (0..self.origins.len())
            .map(|i| {
                (
                    self.book().info(i).map(|f| f.family.clone()).unwrap_or_default(),
                    self.origins[i],
                )
            })
            .collect()
    }

    /// True when face `index` can be read; a file removed after the scan cannot.
    #[must_use]
    pub fn loads(&self, index: usize) -> bool {
        self.font(index).is_some()
    }

    /// The mode this set was built in.
    #[must_use]
    pub fn mode(&self) -> FontsMode {
        self.mode
    }

    /// Resolves each role to its preferred font, its fallback, or neither.
    ///
    /// A family counts only if the face Typst would pick for it can be read.
    #[must_use]
    pub fn resolve(&self, roles: &[FontRole]) -> Vec<FontResolution> {
        roles
            .iter()
            .map(|role| resolve_role(self.book(), &self.origins, |i| self.loads(i), role))
            .collect()
    }
}

/// One bundled family: its faces and the license text that must travel with them.
struct FallbackFamily {
    name: &'static str,
    /// The family's license text (`OFL.txt`, or the GUST Font License), which every copy of the fonts must carry.
    license: &'static str,
    faces: &'static [&'static [u8]],
}

/// The free stand-ins for the template fonts, embedded unmodified.
///
/// Carlito stands in for Calibri, Gelasio for Georgia, and Arimo for Arial, each with the same character widths;
/// TeX Gyre Pagella stands in for Book Antiqua, whose widths it does not match (they differ by about 2.4% on
/// average), so a style marks that role `metric-compatible: false`. They are static files, not
/// variable fonts: a later variable face would win a tie against an earlier static one of the same
/// family, and statics need no weight handling. They are never subset or instanced (Carlito has a
/// Reserved Font Name). Sources, pins, and checksums are in `fonts/LICENSES.md`.
const FALLBACK_FAMILIES: &[FallbackFamily] = &[
    FallbackFamily {
        name: "Carlito",
        license: include_str!("../../fonts/carlito/OFL.txt"),
        faces: &[
            include_bytes!("../../fonts/carlito/Carlito-Regular.ttf"),
            include_bytes!("../../fonts/carlito/Carlito-Bold.ttf"),
            include_bytes!("../../fonts/carlito/Carlito-Italic.ttf"),
            include_bytes!("../../fonts/carlito/Carlito-BoldItalic.ttf"),
        ],
    },
    FallbackFamily {
        name: "Gelasio",
        license: include_str!("../../fonts/gelasio/OFL.txt"),
        faces: &[
            include_bytes!("../../fonts/gelasio/Gelasio-Regular.ttf"),
            include_bytes!("../../fonts/gelasio/Gelasio-Bold.ttf"),
            include_bytes!("../../fonts/gelasio/Gelasio-Italic.ttf"),
            include_bytes!("../../fonts/gelasio/Gelasio-BoldItalic.ttf"),
        ],
    },
    FallbackFamily {
        name: "Arimo",
        license: include_str!("../../fonts/arimo/OFL.txt"),
        faces: &[
            include_bytes!("../../fonts/arimo/Arimo-Regular.ttf"),
            include_bytes!("../../fonts/arimo/Arimo-Bold.ttf"),
            include_bytes!("../../fonts/arimo/Arimo-Italic.ttf"),
            include_bytes!("../../fonts/arimo/Arimo-BoldItalic.ttf"),
        ],
    },
    FallbackFamily {
        name: "TeX Gyre Pagella",
        license: include_str!("../../fonts/pagella/GUST-FONT-LICENSE.txt"),
        faces: &[
            include_bytes!("../../fonts/pagella/texgyrepagella-regular.otf"),
            include_bytes!("../../fonts/pagella/texgyrepagella-bold.otf"),
            include_bytes!("../../fonts/pagella/texgyrepagella-italic.otf"),
            include_bytes!("../../fonts/pagella/texgyrepagella-bolditalic.otf"),
        ],
    },
];

/// The vendored `NOTICE` of the `typst-assets` crate (Apache-2.0), which Typst's own embedded fonts come
/// with. Section 4(d) of that license requires a binary that includes them to carry it.
const TYPST_ASSETS_NOTICE: &str = include_str!("../../fonts/typst-assets/NOTICE");

/// The license notices for every font in the binary: the overview in `fonts/LICENSES.md`, each bundled
/// fallback family's license text, and the `NOTICE` of the crate that supplies Typst's own fonts.
///
/// A binary that embeds the fonts must carry these notices, so `resume licenses` prints them.
#[must_use]
pub fn bundled_font_notices() -> String {
    let mut notices = include_str!("../../fonts/LICENSES.md").to_string();
    for family in FALLBACK_FAMILIES {
        notices.push_str(&format!("\n\n---\n\n{} license\n\n{}", family.name, family.license));
    }
    notices.push_str(&format!(
        "\n\n---\n\nNOTICE of the typst-assets crate (Typst's embedded fonts and data)\n\n{TYPST_ASSETS_NOTICE}"
    ));
    notices
}

/// The fonts shipped in the binary, in a fixed order: typst-kit's (Typst's own defaults, `raw`, and
/// math need them, and their indexes must not move), then the fallbacks.
fn bundled_faces() -> Vec<(Font, FontInfo)> {
    let fallbacks = FALLBACK_FAMILIES
        .iter()
        .flat_map(|family| family.faces)
        .flat_map(|bytes| Font::iter(Bytes::new(*bytes)))
        .map(|font| {
            let info = font.info().clone();
            (font, info)
        });
    typst_kit::fonts::embedded().chain(fallbacks).collect()
}

/// The key that makes one face the same face however it was reached (symlinks, `/var` vs
/// `/private/var`, relative spellings, case spellings on case-insensitive volumes).
fn identity(path: &FontPath) -> (PathBuf, u32) {
    (
        std::fs::canonicalize(&path.path).unwrap_or_else(|_| path.path.clone()),
        path.index,
    )
}

/// Collects the inputs of a [`FontSet`]; all scanning happens in [`build`](Self::build), and only as
/// the mode allows.
pub struct FontSetBuilder {
    mode: FontsMode,
    bundled: Option<Vec<(Font, FontInfo)>>,
    font_paths: Vec<PathBuf>,
    env_paths: Vec<PathBuf>,
    application_dirs: Vec<PathBuf>,
    system: SystemSource,
}

impl std::fmt::Debug for FontSetBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontSetBuilder")
            .field("mode", &self.mode)
            .field("font_paths", &self.font_paths)
            .field("env_paths", &self.env_paths)
            .finish_non_exhaustive()
    }
}

impl Default for FontSetBuilder {
    fn default() -> Self {
        Self {
            mode: FontsMode::Auto,
            bundled: None,
            font_paths: Vec::new(),
            env_paths: Vec::new(),
            application_dirs: application_dirs(),
            system: Box::new(|| typst_kit::fonts::system().collect()),
        }
    }
}

impl FontSetBuilder {
    /// Auto (the default) adds every layer; Bundled uses only the bundled fonts and reads neither the
    /// disk nor the environment.
    #[must_use]
    pub fn mode(mut self, mode: FontsMode) -> Self {
        self.mode = mode;
        self
    }

    /// Replaces the bundled layer, which also holds the fonts Typst needs for math and code; tests only.
    #[cfg(test)]
    #[must_use]
    fn bundled(mut self, faces: impl IntoIterator<Item = (Font, FontInfo)>) -> Self {
        self.bundled = Some(faces.into_iter().collect());
        self
    }

    /// Adds a `--font-path` directory; directories keep the order they are added in.
    ///
    /// Ignored, and not validated, in Bundled mode: the CLI should reject that combination.
    #[must_use]
    pub fn font_path(mut self, dir: impl Into<PathBuf>) -> Self {
        self.font_paths.push(dir.into());
        self
    }

    /// Sets the entries of a `RESUME_FONT_PATH` value, replacing any earlier call; empty entries are
    /// ignored. They come after every `--font-path` directory.
    ///
    /// The caller reads the environment and passes the value in, so tests need not touch it.
    #[must_use]
    pub fn env_paths(mut self, value: Option<&OsStr>) -> Self {
        self.env_paths = value
            .map(|v| std::env::split_paths(v).filter(|p| !p.as_os_str().is_empty()).collect())
            .unwrap_or_default();
        self
    }

    #[cfg(test)]
    pub(crate) fn system_source(mut self, source: impl Fn() -> Vec<Scanned> + Send + Sync + 'static) -> Self {
        self.system = Box::new(source);
        self
    }

    #[cfg(test)]
    pub(crate) fn application_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.application_dirs = dirs;
        self
    }

    /// Scans the requested layers and assembles the set.
    ///
    /// # Errors
    /// - `Error::FontDirectory` - a `--font-path` or `RESUME_FONT_PATH` entry is not a readable directory
    /// - `Error::FontScanPanicked` - typst-kit panicked twice in a row while scanning (it panics when
    ///   a font file disappears mid-scan)
    pub fn build(self) -> Result<FontSet, Error> {
        let bundled = self.bundled.unwrap_or_else(bundled_faces);
        let mut groups = Vec::new();
        if self.mode == FontsMode::Auto {
            for dir in self.font_paths.iter().chain(&self.env_paths) {
                groups.push(Group {
                    origin: FontOrigin::FontPath,
                    faces: scan_dir(dir, FontOrigin::FontPath, |d| typst_kit::fonts::scan(d).collect())?,
                });
            }
            for dir in office_dirs_to_scan(&self.application_dirs) {
                groups.push(Group {
                    origin: FontOrigin::Application,
                    faces: scan_dir(&dir, FontOrigin::Application, |d| typst_kit::fonts::scan(d).collect())?,
                });
            }
            groups.push(Group {
                origin: FontOrigin::System,
                faces: guarded(FontOrigin::System, None, || (self.system)())?,
            });
        }
        let mut set = FontSet::assemble(bundled, groups);
        set.mode = self.mode;
        Ok(set)
    }
}

/// Runs a typst-kit scan, retrying once and then turning a panic into an error.
///
/// typst-kit panics when a font file disappears between its two passes over a directory. A panic leaves
/// no state behind (the font database is local to the scan), and the second pass sees the settled
/// directory, so one retry rides out a font manager or installer running at the same moment. This
/// relies on `panic = "unwind"`, the default.
fn guarded<T>(origin: FontOrigin, dir: Option<&Path>, scan: impl Fn() -> T) -> Result<T, Error> {
    let attempt = || catch_unwind(AssertUnwindSafe(&scan));
    attempt().or_else(|_| attempt()).map_err(|payload| {
        let message = panic_message(payload.as_ref());
        FontScanPanickedSnafu {
            origin,
            message: match dir {
                Some(dir) => format!("{}: {message}", dir.display()),
                None => message,
            },
        }
        .build()
    })
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(ToString::to_string)
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a non-string panic payload".to_string())
}

/// Scans an explicit directory; unlike typst-kit, a missing or unreadable one is an error.
fn scan_dir(dir: &Path, origin: FontOrigin, scan: impl Fn(&Path) -> Vec<Scanned>) -> Result<Vec<Scanned>, Error> {
    std::fs::read_dir(dir).context(FontDirectorySnafu { path: dir })?;
    guarded(origin, Some(dir), || scan(dir))
}

/// Office keeps Calibri and Cambria here on macOS; typst-kit's system scan does not look in these.
fn application_dirs() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        ["Word", "Excel", "PowerPoint", "Outlook"]
            .map(|app| PathBuf::from(format!("/Applications/Microsoft {app}.app/Contents/Resources/DFonts")))
            .into_iter()
            .collect()
    } else {
        Vec::new()
    }
}

/// The existing directories, skipping one whose files (name and size) an earlier one already holds.
///
/// Word, Excel, and PowerPoint ship identical font directories, so scanning all of them costs
/// several times as long and adds nothing. A directory with anything but plain files in it is never
/// skipped, because a directory's own size says nothing about what it holds.
fn office_dirs_to_scan(candidates: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut chosen = Vec::new();
    for dir in candidates {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut only_files = true;
        let listing: BTreeSet<_> = entries
            .flatten()
            .filter_map(|e| {
                let metadata = e.metadata().ok()?;
                only_files &= metadata.is_file();
                Some((e.file_name(), metadata.len()))
            })
            .collect();
        if only_files && listing.is_subset(&seen) {
            continue;
        }
        seen.extend(listing);
        chosen.push(dir.clone());
    }
    chosen
}

// ---- roles ------------------------------------------------------------------------------------

/// Whether a fallback font has the preferred font's character widths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metrics {
    /// Same widths: layout is identical and only glyph shapes differ.
    Compatible,
    /// Different widths: line breaks and the page count may change.
    Different,
}

/// One typographic role of a style, such as body text.
///
/// Build it with [`FontRole::new`]; more requirements (such as the variants a role needs) can be
/// added later without breaking callers.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FontRole {
    /// What the role is for, such as `body`; shown in the build summary.
    pub name: String,
    /// The template's own font, used when installed.
    pub preferred: String,
    /// The bundled stand-in.
    pub fallback: String,
    pub metrics: Metrics,
}

impl FontRole {
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        preferred: impl Into<String>,
        fallback: impl Into<String>,
        metrics: Metrics,
    ) -> Self {
        Self {
            name: name.into(),
            preferred: preferred.into(),
            fallback: fallback.into(),
            metrics,
        }
    }
}

/// What a role got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontResolution {
    /// The preferred font; `family` is the font book's spelling.
    Preferred {
        role: String,
        family: String,
        origin: FontOrigin,
    },
    /// The fallback stands in for a preferred font that is not installed.
    Fallback {
        role: String,
        preferred: String,
        family: String,
        origin: FontOrigin,
        metrics: Metrics,
    },
    /// Neither font exists.
    Missing {
        role: String,
        preferred: String,
        fallback: String,
    },
}

/// Picks the face Typst would use for each family, in order, by family name alone: Typst falls back
/// to the closest variant of a family it has, so presence of the family is what matters. A face that
/// cannot be read counts as absent, because Typst skips it too.
fn resolve_role(
    book: &FontBook,
    origins: &[FontOrigin],
    loads: impl Fn(usize) -> bool,
    role: &FontRole,
) -> FontResolution {
    let pick = |family: &str| {
        let index = book.select(&family.to_lowercase(), FontVariant::default())?;
        loads(index).then_some(())?;
        Some((book.info(index)?.family.clone(), *origins.get(index)?))
    };
    if let Some((family, origin)) = pick(&role.preferred) {
        FontResolution::Preferred {
            role: role.name.clone(),
            family,
            origin,
        }
    } else if let Some((family, origin)) = pick(&role.fallback) {
        FontResolution::Fallback {
            role: role.name.clone(),
            preferred: role.preferred.clone(),
            family,
            origin,
            metrics: role.metrics,
        }
    } else {
        FontResolution::Missing {
            role: role.name.clone(),
            preferred: role.preferred.clone(),
            fallback: role.fallback.clone(),
        }
    }
}

impl FontResolution {
    /// The role this resolution is for.
    #[must_use]
    pub fn role(&self) -> &str {
        match self {
            Self::Preferred { role, .. } | Self::Fallback { role, .. } | Self::Missing { role, .. } => role,
        }
    }

    /// Where the chosen face came from; `None` when neither font exists.
    #[must_use]
    pub fn origin(&self) -> Option<FontOrigin> {
        match self {
            Self::Preferred { origin, .. } | Self::Fallback { origin, .. } => Some(*origin),
            Self::Missing { .. } => None,
        }
    }

    /// The finding for this resolution, or `None` when the preferred font is used.
    ///
    /// `mode` only changes the hint: in Bundled mode installed fonts and `--font-path` are ignored, so
    /// telling the user to install a font would not help.
    #[must_use]
    pub fn diagnostic(&self, mode: FontsMode) -> Option<Diagnostic> {
        let (severity, code, message, how_to_fix) = match self {
            Self::Preferred { .. } => return None,
            Self::Fallback {
                role,
                preferred,
                family,
                metrics: Metrics::Compatible,
                ..
            } => (
                Severity::Note,
                DiagnosticCode::FontFallbackMetric,
                format!("{role}: {preferred} not found; using {family}, which has the same character widths"),
                format!("only glyph shapes differ; install {preferred} or pass --font-path <DIR> to use it"),
            ),
            Self::Fallback {
                role,
                preferred,
                family,
                metrics: Metrics::Different,
                ..
            } => (
                Severity::Warning,
                DiagnosticCode::FontFallbackLayout,
                format!("{role}: {preferred} not found; using {family}, so line breaks may change"),
                format!("install {preferred} or pass --font-path <DIR> with its font files"),
            ),
            Self::Missing {
                role,
                preferred,
                fallback,
            } => (
                Severity::Error,
                DiagnosticCode::FontMissing,
                format!("{role}: neither {preferred} nor {fallback} is available"),
                format!("install {preferred} or {fallback}, pass --font-path <DIR>, or fix the style's font list"),
            ),
        };
        let hint = match mode {
            FontsMode::Auto => how_to_fix,
            FontsMode::Bundled => {
                "--fonts bundled uses only the bundled fonts; drop it to use installed fonts or --font-path".to_string()
            }
        };
        Some(Diagnostic {
            severity,
            code,
            message,
            span: None,
            hints: vec![hint],
            fix: None,
        })
    }
}

#[cfg(test)]
#[path = "fonts_disk_tests.rs"]
mod disk_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok, assert_some};
    use pretty_assertions::assert_eq;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use typst::text::{Coverage, FontFlags, FontStretch, FontStyle, FontWeight};

    fn info(family: &str, weight: u16) -> FontInfo {
        FontInfo {
            family: family.into(),
            variant: FontVariant::new(FontStyle::Normal, FontWeight::from_number(weight), FontStretch::NORMAL),
            flags: FontFlags::empty(),
            axes: vec![],
            coverage: Coverage::from_vec(vec![]),
        }
    }

    fn face(path: &str, index: u32, family: &str) -> Scanned {
        scanned(path, index, info(family, 400))
    }

    fn scanned(path: &str, index: u32, info: FontInfo) -> Scanned {
        (
            FontPath {
                path: PathBuf::from(path),
                index,
            },
            info,
        )
    }

    fn group(origin: FontOrigin, faces: Vec<Scanned>) -> Group {
        Group { origin, faces }
    }

    fn pair(family: &str, origin: FontOrigin) -> (String, FontOrigin) {
        (family.to_string(), origin)
    }

    fn role(preferred: &str, fallback: &str, metrics: Metrics) -> FontRole {
        FontRole::new("body", preferred, fallback, metrics)
    }

    /// Resolution over synthetic faces, whose files do not exist: treat every face as readable.
    fn resolve_synthetic(set: &FontSet, roles: &[FontRole]) -> Vec<FontResolution> {
        roles
            .iter()
            .map(|r| resolve_role(set.book(), &set.origins, |_| true, r))
            .collect()
    }

    fn fallback(metrics: Metrics) -> FontResolution {
        FontResolution::Fallback {
            role: "body".into(),
            preferred: "Calibri".into(),
            family: "Carlito".into(),
            origin: FontOrigin::Bundled,
            metrics,
        }
    }

    fn missing() -> FontResolution {
        FontResolution::Missing {
            role: "body".into(),
            preferred: "Calibri".into(),
            fallback: "Carlito".into(),
        }
    }

    // ---- the bundled set --------------------------------------------------------------------

    #[test]
    fn bundled_set_lists_typst_kit_fonts_plus_the_fallback_families() {
        // The font set is an input to every PDF byte, so any change to it must fail here.
        let set = FontSet::bundled();
        assert_eq!(
            set.families(),
            [
                "Arimo",
                "Carlito",
                "DejaVu Sans Mono",
                "Gelasio",
                "Libertinus Serif",
                "New Computer Modern",
                "New Computer Modern Math",
                "TeX Gyre Pagella"
            ]
        );
        // typst-kit's 17 faces first (so their indexes never move), then 4 variants of each of 4 families.
        assert_eq!(set.origins().len(), 17 + 16, "a face was added or removed");
        assert!(set.origins().iter().all(|o| *o == FontOrigin::Bundled));
        assert_eq!(set.mode(), FontsMode::Bundled);
    }

    #[test]
    fn every_font_file_under_fonts_is_embedded() {
        let on_disk = std::fs::read_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fonts"))
            .map(|dirs| {
                dirs.flatten()
                    .filter(|d| d.path().is_dir())
                    .flat_map(|d| std::fs::read_dir(d.path()).into_iter().flatten().flatten())
                    .filter(|f| matches!(f.path().extension().and_then(|e| e.to_str()), Some("ttf" | "otf")))
                    .count()
            })
            .unwrap_or_default();
        let embedded: usize = FALLBACK_FAMILIES.iter().map(|f| f.faces.len()).sum();
        assert_eq!(
            on_disk, embedded,
            "a font file is on disk but not embedded, or the reverse"
        );
    }

    #[test]
    fn font_notices_carry_the_overview_and_each_familys_license_text() {
        let notices = bundled_font_notices();
        assert!(
            notices.contains("# Bundled fonts"),
            "the LICENSES.md overview is missing"
        );
        for family in FALLBACK_FAMILIES {
            assert!(notices.contains(family.name), "{} is missing", family.name);
        }
        // One OFL text per OFL family (all but TeX Gyre Pagella, which is under the GUST Font License), and the
        // ones the typst-assets NOTICE carries for itself.
        let ofl = "SIL OPEN FONT LICENSE Version 1.1";
        assert_eq!(
            notices.matches(ofl).count(),
            FALLBACK_FAMILIES.len() - 1 + TYPST_ASSETS_NOTICE.matches(ofl).count()
        );
        assert!(
            notices.contains(TYPST_ASSETS_NOTICE),
            "the typst-assets NOTICE is missing"
        );
        assert!(notices.contains("version 1.0, dated 22 June 2009, of the GUST Font License"));
    }

    #[test]
    fn typst_kit_faces_come_first_in_their_own_order_so_their_indexes_never_move() {
        let set = FontSet::bundled();
        let expected: Vec<(String, FontOrigin)> = typst_kit::fonts::embedded()
            .map(|(_, info)| (info.family, FontOrigin::Bundled))
            .collect();
        assert_eq!(set.faces()[..expected.len()], expected[..]);
    }

    #[test]
    fn every_fallback_family_has_all_four_variants_and_is_static() {
        let set = FontSet::bundled();
        for family in ["Arimo", "Carlito", "Gelasio", "TeX Gyre Pagella"] {
            let mut variants: Vec<(FontStyle, u16)> = (0..set.origins().len())
                .filter_map(|i| set.book().info(i))
                .filter(|info| info.family == family)
                .inspect(|info| {
                    assert!(
                        !info.flags.contains(FontFlags::VARIABLE),
                        "{family} must be static: a later variable face would beat an earlier static one"
                    )
                })
                .map(|info| (info.variant.style, info.variant.weight.to_number()))
                .collect();
            variants.sort_by_key(|(style, weight)| (*style as u8, *weight));
            assert_eq!(
                variants,
                [
                    (FontStyle::Normal, 400),
                    (FontStyle::Normal, 700),
                    (FontStyle::Italic, 400),
                    (FontStyle::Italic, 700)
                ],
                "{family} needs regular, bold, italic, and bold italic"
            );
        }
    }

    #[test]
    fn every_font_in_the_book_loads() {
        let set = FontSet::bundled();
        let count = set.origins().len();
        for index in 0..count {
            assert!(set.loads(index), "font {index} did not load");
        }
        assert!(!set.loads(count));
    }

    // ---- ordering ---------------------------------------------------------------------------

    #[test]
    fn ordering_within_a_group_is_independent_of_enumeration_order() {
        let sorted = |faces: Vec<Scanned>| {
            FontSet::assemble(
                vec![],
                vec![
                    group(FontOrigin::System, vec![face("/s/z.ttf", 0, "Z")]),
                    group(FontOrigin::FontPath, faces),
                ],
            )
        };
        let one = sorted(vec![
            face("/d/b.ttf", 0, "B"),
            face("/d/a.ttf", 1, "A1"),
            face("/d/a.ttf", 0, "A0"),
        ]);
        let another = sorted(vec![
            face("/d/a.ttf", 0, "A0"),
            face("/d/b.ttf", 0, "B"),
            face("/d/a.ttf", 1, "A1"),
        ]);
        // Groups keep their given order; faces inside a group are sorted by path, then face index.
        let expected = [
            pair("Z", FontOrigin::System),
            pair("A0", FontOrigin::FontPath),
            pair("A1", FontOrigin::FontPath),
            pair("B", FontOrigin::FontPath),
        ];
        assert_eq!(one.faces(), expected);
        assert_eq!(another.faces(), expected);
    }

    #[test]
    fn duplicate_path_and_index_is_loaded_once_and_the_earlier_layer_wins() {
        let set = FontSet::assemble(
            vec![],
            vec![
                group(FontOrigin::FontPath, vec![face("/d/a.ttf", 0, "A")]),
                group(
                    FontOrigin::System,
                    vec![face("/d/a.ttf", 0, "A"), face("/d/a.ttf", 1, "A2")],
                ),
            ],
        );
        assert_eq!(
            set.faces(),
            [pair("A", FontOrigin::FontPath), pair("A2", FontOrigin::System)]
        );
    }

    #[test]
    fn font_path_beats_installed_font_of_same_family() {
        let set = FontSet::assemble(
            vec![],
            vec![
                group(FontOrigin::FontPath, vec![face("/user/Calibri.ttf", 0, "Calibri")]),
                group(FontOrigin::System, vec![face("/sys/calibri.ttf", 0, "Calibri")]),
            ],
        );
        assert_eq!(
            resolve_synthetic(&set, &[role("Calibri", "Carlito", Metrics::Compatible)]),
            [FontResolution::Preferred {
                role: "body".into(),
                family: "Calibri".into(),
                origin: FontOrigin::FontPath
            }]
        );
    }

    /// The traps rsm-141.7 must design around: "the earliest face wins" holds only among equal static faces.
    #[test]
    fn a_later_variable_face_beats_an_earlier_static_one() {
        let mut variable = info("Carlito", 400);
        variable.flags = FontFlags::VARIABLE;
        let set = FontSet::assemble(
            vec![],
            vec![
                group(FontOrigin::Bundled, vec![face("/b/carlito.ttf", 0, "Carlito")]),
                group(FontOrigin::System, vec![scanned("/s/carlito-var.ttf", 0, variable)]),
            ],
        );
        let resolved = resolve_synthetic(&set, &[role("Carlito", "x", Metrics::Compatible)]);
        assert_eq!(resolved[0].origin(), Some(FontOrigin::System), "{resolved:?}");
    }

    #[test]
    fn a_later_closer_variant_beats_an_earlier_one() {
        let set = FontSet::assemble(
            vec![],
            vec![
                group(
                    FontOrigin::Bundled,
                    vec![scanned("/b/carlito-bold.ttf", 0, info("Carlito", 700))],
                ),
                group(
                    FontOrigin::System,
                    vec![scanned("/s/carlito-regular.ttf", 0, info("Carlito", 400))],
                ),
            ],
        );
        let resolved = resolve_synthetic(&set, &[role("Carlito", "x", Metrics::Compatible)]);
        assert_eq!(resolved[0].origin(), Some(FontOrigin::System), "{resolved:?}");
    }

    #[test]
    fn a_relative_directory_is_stored_as_the_canonical_path() {
        // Cargo runs tests from the package root; `target` may be elsewhere when CARGO_TARGET_DIR is set.
        assert_ok!(std::fs::create_dir_all("target"));
        let tmp = assert_ok!(tempfile::Builder::new().tempdir_in("target"));
        assert_ok!(std::fs::write(tmp.path().join("a.otf"), b"x"));
        let relative_dir = PathBuf::from("target").join(assert_some!(tmp.path().file_name()));
        let relative = (
            FontPath {
                path: relative_dir.join("a.otf"),
                index: 0,
            },
            info("A", 400),
        );
        assert!(relative.0.path.is_relative(), "the test needs a relative path");
        let set = FontSet::assemble(vec![], vec![group(FontOrigin::FontPath, vec![relative])]);
        let stored = assert_some!(set.store.source(0));
        let any: &dyn std::any::Any = stored;
        let path = &assert_some!(any.downcast_ref::<FontPath>()).path;
        assert!(path.is_absolute(), "{path:?}");
        assert_eq!(*path, assert_ok!(std::fs::canonicalize(relative_dir.join("a.otf"))));
    }

    // ---- the builder ------------------------------------------------------------------------

    #[test]
    fn bundled_mode_never_asks_the_system_or_validates_directories() {
        let asked = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&asked);
        let set = assert_ok!(
            FontSet::builder()
                .mode(FontsMode::Bundled)
                .font_path("/definitely/not/here")
                .system_source(move || {
                    flag.store(true, Ordering::SeqCst);
                    vec![face("/sys/calibri.ttf", 0, "Calibri")]
                })
                .build()
        );
        assert!(!asked.load(Ordering::SeqCst), "the system scan ran in bundled mode");
        assert!(set.origins().iter().all(|o| *o == FontOrigin::Bundled));
        assert_eq!(set.origins().len(), FontSet::bundled().origins().len());
        assert_eq!(set.mode(), FontsMode::Bundled);
    }

    #[test]
    fn auto_mode_asks_the_system_source() {
        let set = assert_ok!(
            FontSet::builder()
                .application_dirs(vec![])
                .system_source(|| vec![face("/sys/calibri.ttf", 0, "Calibri")])
                .build()
        );
        assert!(set.origins().contains(&FontOrigin::System));
        assert_eq!(set.mode(), FontsMode::Auto);
    }

    #[test]
    fn a_missing_font_directory_is_an_error_naming_it() {
        let err = assert_err!(FontSet::builder().font_path("/definitely/not/here").build());
        assert!(
            matches!(err, Error::FontDirectory { ref path, .. } if path.ends_with("here")),
            "{err:?}"
        );
        assert!(err.to_string().contains("here"), "{err}");
    }

    #[test]
    fn a_file_given_as_a_font_directory_is_an_error() {
        let dir = assert_ok!(tempfile::tempdir());
        let file = dir.path().join("x.ttf");
        assert_ok!(std::fs::write(&file, b"x"));
        let err = assert_err!(FontSet::builder().font_path(&file).build());
        assert!(matches!(err, Error::FontDirectory { .. }), "{err:?}");
    }

    #[test]
    fn a_bad_env_entry_is_an_error_too() {
        let value = assert_ok!(std::env::join_paths(["/definitely/not/here"]));
        let err = assert_err!(FontSet::builder().env_paths(Some(value.as_os_str())).build());
        assert!(matches!(err, Error::FontDirectory { .. }), "{err:?}");
    }

    #[test]
    fn env_paths_drop_empty_entries_keep_order_and_replace_earlier_calls() {
        let value = if cfg!(windows) { ";a;;b;" } else { ":a::b:" };
        let builder = FontSet::builder()
            .env_paths(Some(OsStr::new("old")))
            .env_paths(Some(OsStr::new(value)));
        assert_eq!(builder.env_paths, [PathBuf::from("a"), PathBuf::from("b")]);
        assert!(FontSet::builder().env_paths(None).env_paths.is_empty());
    }

    #[test]
    fn a_system_scan_that_panics_twice_is_an_error_not_a_crash() {
        let err = assert_err!(
            FontSet::builder()
                .application_dirs(vec![])
                .system_source(|| panic!("database must contain this font"))
                .build()
        );
        assert!(
            matches!(err, Error::FontScanPanicked { origin: FontOrigin::System, ref message } if message.contains("database must contain")),
            "{err:?}"
        );
    }

    #[test]
    fn a_scan_that_panics_once_is_retried() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let set = assert_ok!(
            FontSet::builder()
                .application_dirs(vec![])
                .system_source(move || {
                    assert!(counter.fetch_add(1, Ordering::SeqCst) > 0, "a file vanished mid-scan");
                    vec![face("/sys/calibri.ttf", 0, "Calibri")]
                })
                .build()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(set.origins().contains(&FontOrigin::System));
    }

    #[test]
    fn a_directory_scan_panic_names_the_directory_and_the_layer() {
        let dir = assert_ok!(tempfile::tempdir());
        for origin in [FontOrigin::FontPath, FontOrigin::Application] {
            let err = assert_err!(scan_dir(dir.path(), origin, |_| panic!("boom")));
            match err {
                Error::FontScanPanicked { origin: got, message } => {
                    assert_eq!(got, origin);
                    assert!(
                        message.contains("boom") && message.contains(&dir.path().display().to_string()),
                        "{message}"
                    );
                }
                other => panic!("unexpected error: {other:?}"),
            }
        }
    }

    #[test]
    fn application_directories_are_the_four_office_apps_on_macos_and_none_elsewhere() {
        let dirs = application_dirs();
        if cfg!(target_os = "macos") {
            let apps: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
            assert_eq!(
                apps,
                ["Word", "Excel", "PowerPoint", "Outlook"]
                    .map(|a| format!("/Applications/Microsoft {a}.app/Contents/Resources/DFonts"))
            );
        } else {
            assert!(dirs.is_empty());
        }
    }

    #[test]
    fn an_application_directory_whose_files_are_already_scanned_is_skipped() {
        let tmp = assert_ok!(tempfile::tempdir());
        let make = |name: &str, files: &[(&str, &[u8])]| {
            let dir = tmp.path().join(name);
            assert_ok!(std::fs::create_dir(&dir));
            for (file, bytes) in files {
                assert_ok!(std::fs::write(dir.join(file), bytes));
            }
            dir
        };
        let word = make("word", &[("a.ttf", b"1"), ("b.ttf", b"22")]);
        let excel = make("excel", &[("a.ttf", b"1"), ("b.ttf", b"22")]); // same names and sizes as Word
        let outlook = make("outlook", &[("a.ttf", b"1")]); // a subset
        let different = make("different", &[("a.ttf", b"333")]); // same name, different size
        let absent = tmp.path().join("absent");
        let chosen = office_dirs_to_scan(&[word.clone(), excel, outlook, different.clone(), absent]);
        assert_eq!(chosen, [word, different]);
    }

    #[test]
    fn a_directory_holding_anything_but_plain_files_is_never_skipped() {
        let tmp = assert_ok!(tempfile::tempdir());
        let make = |name: &str| {
            let dir = tmp.path().join(name);
            assert_ok!(std::fs::create_dir_all(dir.join("sub")));
            assert_ok!(std::fs::write(dir.join("a.ttf"), b"1"));
            dir
        };
        // Identical listings, so the second would be skipped if a subdirectory's own size proved anything.
        let (one, two) = (make("one"), make("two"));
        assert_eq!(office_dirs_to_scan(&[one.clone(), two.clone()]), [one, two]);
    }

    // ---- resolution -------------------------------------------------------------------------

    #[test]
    fn role_resolution_prefers_installed_original() {
        let set = FontSet::assemble(
            vec![],
            vec![group(
                FontOrigin::System,
                vec![face("/s/c.ttf", 0, "Calibri"), face("/s/k.ttf", 0, "Carlito")],
            )],
        );
        let resolved = resolve_synthetic(&set, &[role("calibri", "Carlito", Metrics::Compatible)]);
        assert_eq!(
            resolved,
            [FontResolution::Preferred {
                role: "body".into(),
                family: "Calibri".into(),
                origin: FontOrigin::System
            }],
            "the lookup is case-insensitive and reports the book's own spelling"
        );
        assert!(resolved[0].diagnostic(FontsMode::Auto).is_none());
        assert_eq!(
            (resolved[0].role(), resolved[0].origin()),
            ("body", Some(FontOrigin::System))
        );
    }

    #[test]
    fn role_resolution_falls_back_and_flags_metric_compatibility() {
        let set = FontSet::assemble(
            vec![],
            vec![group(FontOrigin::FontPath, vec![face("/b/k.ttf", 0, "Carlito")])],
        );
        let resolved = resolve_synthetic(
            &set,
            &[
                role("Calibri", "Carlito", Metrics::Compatible),
                role("Calibri", "Carlito", Metrics::Different),
            ],
        );
        assert_eq!(
            resolved[0],
            FontResolution::Fallback {
                role: "body".into(),
                preferred: "Calibri".into(),
                family: "Carlito".into(),
                origin: FontOrigin::FontPath,
                metrics: Metrics::Compatible
            }
        );
        let note = assert_some!(resolved[0].diagnostic(FontsMode::Auto));
        assert_eq!(
            (note.severity, note.code),
            (Severity::Note, DiagnosticCode::FontFallbackMetric)
        );
        let warning = assert_some!(resolved[1].diagnostic(FontsMode::Auto));
        assert_eq!(
            (warning.severity, warning.code),
            (Severity::Warning, DiagnosticCode::FontFallbackLayout)
        );
    }

    #[test]
    fn role_resolution_reports_missing_when_neither_font_exists() {
        let resolved = &FontSet::bundled().resolve(&[role("Not Installed", "Also Missing", Metrics::Compatible)])[0];
        assert_eq!(
            *resolved,
            FontResolution::Missing {
                role: "body".into(),
                preferred: "Not Installed".into(),
                fallback: "Also Missing".into()
            }
        );
        assert_eq!(resolved.origin(), None);
        let error = assert_some!(resolved.diagnostic(FontsMode::Auto));
        assert_eq!(
            (error.severity, error.code),
            (Severity::Error, DiagnosticCode::FontMissing)
        );
    }

    #[test]
    fn a_family_present_only_in_another_variant_still_resolves_as_typst_would_use_it() {
        let mut bold_italic = info("Calibri", 700);
        bold_italic.variant.style = FontStyle::Italic;
        let book = FontBook::from_infos([bold_italic]);
        let resolved = resolve_role(
            &book,
            &[FontOrigin::System],
            |_| true,
            &role("Calibri", "Carlito", Metrics::Compatible),
        );
        assert!(matches!(resolved, FontResolution::Preferred { .. }), "{resolved:?}");
    }

    #[test]
    fn a_face_that_cannot_be_read_does_not_count() {
        let book = FontBook::from_infos([info("Calibri", 400), info("Carlito", 400)]);
        let origins = [FontOrigin::System, FontOrigin::Bundled];
        let r = role("Calibri", "Carlito", Metrics::Compatible);
        let calibri_gone = resolve_role(&book, &origins, |i| i != 0, &r);
        assert!(
            matches!(
                calibri_gone,
                FontResolution::Fallback {
                    origin: FontOrigin::Bundled,
                    ..
                }
            ),
            "{calibri_gone:?}"
        );
        assert_eq!(resolve_role(&book, &origins, |_| false, &r), missing());
    }

    #[test]
    fn diagnostics_say_what_was_used_and_how_to_fix_it() {
        let metric = assert_some!(fallback(Metrics::Compatible).diagnostic(FontsMode::Auto));
        assert_eq!(
            metric.message,
            "body: Calibri not found; using Carlito, which has the same character widths"
        );
        assert_eq!(
            metric.hints,
            ["only glyph shapes differ; install Calibri or pass --font-path <DIR> to use it"]
        );
        let layout = assert_some!(fallback(Metrics::Different).diagnostic(FontsMode::Auto));
        assert_eq!(
            layout.message,
            "body: Calibri not found; using Carlito, so line breaks may change"
        );
        assert_eq!(
            layout.hints,
            ["install Calibri or pass --font-path <DIR> with its font files"]
        );
        let gone = assert_some!(missing().diagnostic(FontsMode::Auto));
        assert_eq!(gone.message, "body: neither Calibri nor Carlito is available");
        assert_eq!(
            gone.hints,
            ["install Calibri or Carlito, pass --font-path <DIR>, or fix the style's font list"]
        );
        for d in [metric, layout, gone] {
            assert!(d.span.is_none() && d.fix.is_none());
        }
    }

    #[test]
    fn in_bundled_mode_the_hint_does_not_suggest_what_bundled_mode_ignores() {
        for resolution in [fallback(Metrics::Compatible), fallback(Metrics::Different), missing()] {
            let d = assert_some!(resolution.diagnostic(FontsMode::Bundled));
            assert_eq!(
                d.hints,
                ["--fonts bundled uses only the bundled fonts; drop it to use installed fonts or --font-path"]
            );
        }
    }

    #[test]
    fn font_roles_are_built_with_new() {
        let r = FontRole::new(
            String::from("body"),
            "Calibri",
            String::from("Carlito"),
            Metrics::Different,
        );
        assert_eq!(
            (r.name.as_str(), r.preferred.as_str(), r.fallback.as_str()),
            ("body", "Calibri", "Carlito")
        );
        assert_eq!(r.metrics, Metrics::Different);
    }
}
