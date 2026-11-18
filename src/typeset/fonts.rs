//! ABOUTME: The ordered, deterministic set of fonts a render may use, and per-role font resolution.
//! ABOUTME: Bundled fonts always; in Auto mode also --font-path, RESUME_FONT_PATH, Office, and system fonts.

use std::collections::{BTreeSet, HashSet};
use std::ffi::OsStr;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use snafu::ResultExt;
use strum::{AsRefStr, Display};
use typst::text::{Font, FontBook, FontInfo, FontVariant};
use typst::utils::LazyHash;
use typst_kit::fonts::{FontPath, FontStore};

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};
use crate::error::{Error, FontDirectorySnafu, FontScanPanickedSnafu};
use crate::model::options::FontsMode;

/// Where a face came from.
///
/// Both `--font-path` and `RESUME_FONT_PATH` directories are `FontPath`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum FontOrigin {
    /// Shipped inside the binary.
    Bundled,
    /// A directory the user named.
    FontPath,
    /// A known application font directory, such as Microsoft Office's on macOS.
    Application,
    /// An operating-system font directory.
    System,
}

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
}

impl std::fmt::Debug for FontSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontSet").field("families", &self.families()).finish()
    }
}

impl FontSet {
    /// Only the bundled fonts: for now those embedded in typst-kit (Libertinus Serif, New Computer
    /// Modern, and DejaVu Sans Mono).
    #[must_use]
    pub fn bundled() -> Self {
        Self::assemble(bundled_faces(), Vec::new())
    }

    /// Starts a set from the bundled fonts; add layers, then [`FontSetBuilder::build`].
    #[must_use]
    pub fn builder() -> FontSetBuilder {
        FontSetBuilder::default()
    }

    /// Bundled faces first, then each group in order; a `(path, index)` already seen is skipped.
    ///
    /// Inside a group faces are sorted by canonical path, then face index, because the scans return
    /// them in directory-enumeration order, which differs between machines.
    fn assemble(bundled: Vec<(Font, FontInfo)>, groups: Vec<Group>) -> Self {
        let mut store = FontStore::new();
        let mut origins: Vec<FontOrigin> = bundled.iter().map(|_| FontOrigin::Bundled).collect();
        store.extend(bundled);
        let mut seen = HashSet::new();
        for Group { origin, faces } in groups {
            let mut keyed: Vec<_> = faces.into_iter().map(|face| (identity(&face.0), face)).collect();
            keyed.sort_by(|a, b| a.0.cmp(&b.0));
            for (key, face) in keyed {
                if seen.insert(key) {
                    store.push(face);
                    origins.push(origin);
                }
            }
        }
        Self { store, origins }
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

    /// Resolves each role to its preferred font, its fallback, or neither.
    #[must_use]
    pub fn resolve(&self, roles: &[FontRole]) -> Vec<FontResolution> {
        roles
            .iter()
            .map(|role| resolve_role(self.book(), &self.origins, role))
            .collect()
    }
}

/// The fonts shipped in the binary, in a fixed order.
fn bundled_faces() -> Vec<(Font, FontInfo)> {
    typst_kit::fonts::embedded().collect()
}

/// The key that makes one face the same face however it was reached (symlinks, `/var` vs
/// `/private/var`, case spellings on case-insensitive volumes).
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

    /// Replaces the default bundled layer.
    #[must_use]
    pub fn bundled(mut self, faces: impl IntoIterator<Item = (Font, FontInfo)>) -> Self {
        self.bundled = Some(faces.into_iter().collect());
        self
    }

    /// Adds a `--font-path` directory; directories keep the order they are added in.
    #[must_use]
    pub fn font_path(mut self, dir: impl Into<PathBuf>) -> Self {
        self.font_paths.push(dir.into());
        self
    }

    /// Adds the entries of a `RESUME_FONT_PATH` value; empty entries are ignored.
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
    fn system_source(mut self, source: impl Fn() -> Vec<Scanned> + Send + Sync + 'static) -> Self {
        self.system = Box::new(source);
        self
    }

    #[cfg(test)]
    fn application_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.application_dirs = dirs;
        self
    }

    /// Scans the requested layers and assembles the set.
    ///
    /// # Errors
    /// - `Error::FontDirectory` - a `--font-path` or `RESUME_FONT_PATH` entry is not a readable directory
    /// - `Error::FontScanPanicked` - typst-kit panicked while scanning (it does when a font file
    ///   disappears mid-scan)
    pub fn build(self) -> Result<FontSet, Error> {
        let bundled = self.bundled.unwrap_or_else(bundled_faces);
        let mut groups = Vec::new();
        if self.mode == FontsMode::Auto {
            for dir in self.font_paths.iter().chain(&self.env_paths) {
                groups.push(Group {
                    origin: FontOrigin::FontPath,
                    faces: scan_dir(dir)?,
                });
            }
            for dir in office_dirs_to_scan(&self.application_dirs) {
                groups.push(Group {
                    origin: FontOrigin::Application,
                    faces: scan_dir(&dir)?,
                });
            }
            groups.push(Group {
                origin: FontOrigin::System,
                faces: guarded(FontOrigin::System, || (self.system)())?,
            });
        }
        Ok(FontSet::assemble(bundled, groups))
    }
}

/// Runs a typst-kit scan, turning its panics into an error.
fn guarded<T>(origin: FontOrigin, scan: impl FnOnce() -> T) -> Result<T, Error> {
    catch_unwind(AssertUnwindSafe(scan)).map_err(|payload| {
        FontScanPanickedSnafu {
            origin,
            message: panic_message(payload.as_ref()),
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
fn scan_dir(dir: &Path) -> Result<Vec<Scanned>, Error> {
    std::fs::read_dir(dir).context(FontDirectorySnafu { path: dir })?;
    guarded(FontOrigin::FontPath, || typst_kit::fonts::scan(dir).collect())
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
/// several times as long and adds nothing.
fn office_dirs_to_scan(candidates: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut chosen = Vec::new();
    for dir in candidates {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let listing: BTreeSet<_> = entries
            .flatten()
            .filter_map(|e| Some((e.file_name(), e.metadata().ok()?.len())))
            .collect();
        if listing.is_subset(&seen) {
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontRole {
    pub name: String,
    /// The template's own font, used when installed.
    pub preferred: String,
    /// The bundled stand-in.
    pub fallback: String,
    pub metrics: Metrics,
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
/// to the closest variant of a family it has, so presence of the family is what matters.
fn resolve_role(book: &FontBook, origins: &[FontOrigin], role: &FontRole) -> FontResolution {
    let pick = |family: &str| {
        let index = book.select(&family.to_lowercase(), FontVariant::default())?;
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
    /// The finding for this resolution, or `None` when the preferred font is used.
    #[must_use]
    pub fn diagnostic(&self) -> Option<Diagnostic> {
        let (severity, code, message, hint) = match self {
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
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok, assert_some};
    use pretty_assertions::assert_eq;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
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
        (
            FontPath {
                path: PathBuf::from(path),
                index,
            },
            info(family, 400),
        )
    }

    fn group(origin: FontOrigin, faces: Vec<Scanned>) -> Group {
        Group { origin, faces }
    }

    /// `(family, origin)` of every face, in font-book order.
    fn families_in_order(set: &FontSet) -> Vec<(String, FontOrigin)> {
        set.faces()
    }

    fn role(preferred: &str, fallback: &str, metrics: Metrics) -> FontRole {
        FontRole {
            name: "body".into(),
            preferred: preferred.into(),
            fallback: fallback.into(),
            metrics,
        }
    }

    /// The bytes of the first embedded face of `family`.
    fn embedded_bytes(family: &str) -> Vec<u8> {
        let (font, _) = assert_some!(typst_kit::fonts::embedded().find(|(_, i)| i.family == family));
        font.data().as_slice().to_vec()
    }

    fn pair(family: &str, origin: FontOrigin) -> (String, FontOrigin) {
        (family.to_string(), origin)
    }

    #[test]
    fn bundled_set_is_exactly_the_typst_kit_fonts() {
        // The font set is an input to every PDF byte, so a typst-kit bump that changes it must fail here.
        assert_eq!(
            FontSet::bundled().families(),
            [
                "DejaVu Sans Mono",
                "Libertinus Serif",
                "New Computer Modern",
                "New Computer Modern Math"
            ]
        );
    }

    #[test]
    fn every_font_in_the_book_loads() {
        let set = FontSet::bundled();
        let count = set.origins().len();
        assert!(count > 0);
        for index in 0..count {
            assert!(set.font(index).is_some(), "font {index} did not load");
        }
        assert!(set.font(count).is_none());
    }

    #[test]
    fn families_are_sorted_and_unique() {
        let families = FontSet::bundled().families();
        let mut sorted = families.clone();
        sorted.sort_by_key(|f| f.to_lowercase());
        sorted.dedup();
        assert_eq!(families, sorted);
    }

    #[test]
    fn bundled_faces_all_have_the_bundled_origin() {
        let set = FontSet::bundled();
        assert!(set.origins().iter().all(|o| *o == FontOrigin::Bundled));
    }

    // ---- ordering ---------------------------------------------------------------------------

    #[test]
    fn layer_order_is_bundled_then_font_path_then_env_then_application_then_system() {
        let user = assert_ok!(tempfile::tempdir());
        let env = assert_ok!(tempfile::tempdir());
        let app = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::write(
            user.path().join("u.otf"),
            embedded_bytes("Libertinus Serif")
        ));
        assert_ok!(std::fs::write(
            env.path().join("e.ttf"),
            embedded_bytes("DejaVu Sans Mono")
        ));
        assert_ok!(std::fs::write(
            app.path().join("a.otf"),
            embedded_bytes("New Computer Modern")
        ));
        let env_value = assert_ok!(std::env::join_paths([env.path()]));
        let bundled: Vec<_> = typst_kit::fonts::embedded()
            .filter(|(_, i)| i.family == "New Computer Modern Math")
            .take(1)
            .collect();
        // Called out of layer order on purpose: the order of the calls must not matter.
        let set = assert_ok!(
            FontSet::builder()
                .system_source(|| vec![face("/sys/s.ttf", 0, "S")])
                .env_paths(Some(env_value.as_os_str()))
                .application_dirs(vec![app.path().to_path_buf()])
                .font_path(user.path())
                .bundled(bundled)
                .build()
        );
        assert_eq!(
            families_in_order(&set),
            [
                pair("New Computer Modern Math", FontOrigin::Bundled),
                pair("Libertinus Serif", FontOrigin::FontPath),
                pair("DejaVu Sans Mono", FontOrigin::FontPath),
                pair("New Computer Modern", FontOrigin::Application),
                pair("S", FontOrigin::System),
            ]
        );
    }

    #[test]
    fn ordering_within_a_group_is_independent_of_enumeration_order() {
        let enumerated_one_way = FontSet::assemble(
            vec![],
            vec![
                group(FontOrigin::System, vec![face("/s/z.ttf", 0, "Z")]),
                group(
                    FontOrigin::FontPath,
                    vec![
                        face("/d/b.ttf", 0, "B"),
                        face("/d/a.ttf", 1, "A1"),
                        face("/d/a.ttf", 0, "A0"),
                    ],
                ),
            ],
        );
        let enumerated_another = FontSet::assemble(
            vec![],
            vec![
                group(FontOrigin::System, vec![face("/s/z.ttf", 0, "Z")]),
                group(
                    FontOrigin::FontPath,
                    vec![
                        face("/d/a.ttf", 0, "A0"),
                        face("/d/b.ttf", 0, "B"),
                        face("/d/a.ttf", 1, "A1"),
                    ],
                ),
            ],
        );
        // Groups keep their given order; faces inside a group are sorted by path, then face index.
        let expected = [
            pair("Z", FontOrigin::System),
            pair("A0", FontOrigin::FontPath),
            pair("A1", FontOrigin::FontPath),
            pair("B", FontOrigin::FontPath),
        ];
        assert_eq!(families_in_order(&enumerated_one_way), expected);
        assert_eq!(families_in_order(&enumerated_another), expected);
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
            families_in_order(&set),
            [pair("A", FontOrigin::FontPath), pair("A2", FontOrigin::System)]
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_same_file_reached_through_a_symlink_is_one_face() {
        let tmp = assert_ok!(tempfile::tempdir());
        let real = tmp.path().join("real");
        assert_ok!(std::fs::create_dir(&real));
        assert_ok!(std::fs::write(real.join("a.otf"), b"x"));
        assert_ok!(std::os::unix::fs::symlink(&real, tmp.path().join("alias")));
        let direct = (
            FontPath {
                path: real.join("a.otf"),
                index: 0,
            },
            info("A", 400),
        );
        let aliased = (
            FontPath {
                path: tmp.path().join("alias/a.otf"),
                index: 0,
            },
            info("A", 400),
        );
        let set = FontSet::assemble(
            vec![],
            vec![
                group(FontOrigin::FontPath, vec![direct]),
                group(FontOrigin::System, vec![aliased]),
            ],
        );
        assert_eq!(families_in_order(&set), [pair("A", FontOrigin::FontPath)]);
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
            set.resolve(&[role("Calibri", "Carlito", Metrics::Compatible)]),
            [FontResolution::Preferred {
                role: "body".into(),
                family: "Calibri".into(),
                origin: FontOrigin::FontPath
            }]
        );
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
    }

    #[test]
    fn a_missing_font_directory_is_an_error_naming_it() {
        let err = assert_err!(FontSet::builder().font_path("/definitely/not/here").build());
        assert!(
            matches!(err, Error::FontDirectory { ref path, .. } if path.ends_with("here")),
            "{err:?}"
        );
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
    fn env_paths_drop_empty_entries_and_keep_order() {
        let builder = FontSet::builder().env_paths(Some(OsStr::new(if cfg!(windows) { ";a;;b;" } else { ":a::b:" })));
        assert_eq!(builder.env_paths, [PathBuf::from("a"), PathBuf::from("b")]);
        assert!(FontSet::builder().env_paths(None).env_paths.is_empty());
    }

    #[test]
    fn a_panicking_system_scan_is_an_error_not_a_crash() {
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
        let resolved = set.resolve(&[role("calibri", "Carlito", Metrics::Compatible)]);
        assert_eq!(
            resolved,
            [FontResolution::Preferred {
                role: "body".into(),
                family: "Calibri".into(),
                origin: FontOrigin::System
            }],
            "the lookup is case-insensitive and reports the book's own spelling"
        );
        assert!(resolved[0].diagnostic().is_none());
    }

    #[test]
    fn role_resolution_falls_back_and_flags_metric_compatibility() {
        let set = FontSet::assemble(
            vec![],
            vec![group(FontOrigin::FontPath, vec![face("/b/k.ttf", 0, "Carlito")])],
        );
        let compatible = &set.resolve(&[role("Calibri", "Carlito", Metrics::Compatible)])[0];
        let different = &set.resolve(&[role("Calibri", "Carlito", Metrics::Different)])[0];
        assert_eq!(
            *compatible,
            FontResolution::Fallback {
                role: "body".into(),
                preferred: "Calibri".into(),
                family: "Carlito".into(),
                origin: FontOrigin::FontPath,
                metrics: Metrics::Compatible
            }
        );
        let note = assert_some!(compatible.diagnostic());
        assert_eq!(
            (note.severity, note.code),
            (Severity::Note, DiagnosticCode::FontFallbackMetric)
        );
        let warning = assert_some!(different.diagnostic());
        assert_eq!(
            (warning.severity, warning.code),
            (Severity::Warning, DiagnosticCode::FontFallbackLayout)
        );
    }

    #[test]
    fn role_resolution_reports_missing_when_neither_font_exists() {
        let resolved = &FontSet::bundled().resolve(&[role("Calibri", "Carlito", Metrics::Compatible)])[0];
        assert!(matches!(resolved, FontResolution::Missing { .. }), "{resolved:?}");
        let error = assert_some!(resolved.diagnostic());
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
            &role("Calibri", "Carlito", Metrics::Compatible),
        );
        assert!(matches!(resolved, FontResolution::Preferred { .. }), "{resolved:?}");
    }

    #[test]
    fn diagnostics_say_what_was_used_and_how_to_fix_it() {
        let fallback = |metrics| FontResolution::Fallback {
            role: "body".into(),
            preferred: "Calibri".into(),
            family: "Carlito".into(),
            origin: FontOrigin::Bundled,
            metrics,
        };
        let metric = assert_some!(fallback(Metrics::Compatible).diagnostic());
        assert_eq!(
            metric.message,
            "body: Calibri not found; using Carlito, which has the same character widths"
        );
        assert!(metric.hints[0].contains("--font-path"), "{:?}", metric.hints);
        let layout = assert_some!(fallback(Metrics::Different).diagnostic());
        assert_eq!(
            layout.message,
            "body: Calibri not found; using Carlito, so line breaks may change"
        );
        let missing = assert_some!(
            FontResolution::Missing {
                role: "body".into(),
                preferred: "Calibri".into(),
                fallback: "Carlito".into()
            }
            .diagnostic()
        );
        assert_eq!(missing.message, "body: neither Calibri nor Carlito is available");
        for d in [metric, layout, missing] {
            assert!(d.span.is_none() && d.fix.is_none());
        }
    }

    #[test]
    fn origins_display_in_kebab_case() {
        assert_eq!(FontOrigin::FontPath.to_string(), "font-path");
        assert_eq!(FontOrigin::Bundled.as_ref() as &str, "bundled");
    }
}
