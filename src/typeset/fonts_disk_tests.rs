//! ABOUTME: The layered font set against real files in temp directories, with the system scan switched off.
//! ABOUTME: Faces are copied from the fonts embedded in typst-kit; nothing is committed, and no test reads the machine's fonts.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
use typst::layout::Abs;
use typst::text::FontVariations;

use super::*;
use crate::model::options::Paper;
use crate::model::{Resume, StyleName};
use crate::style::StyleSource;
use crate::typeset::{RenderOptions, Typesetter};

fn crate_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// The first embedded face of `family`.
fn embedded(family: &str) -> (Font, FontInfo) {
    assert_some!(typst_kit::fonts::embedded().find(|(_, info)| info.family == family))
}

fn bytes_of(family: &str) -> Vec<u8> {
    embedded(family).0.data().as_slice().to_vec()
}

/// The first bundled face of `family`, whether typst-kit's or one of ours.
fn shipped(family: &str) -> (Font, FontInfo) {
    assert_some!(bundled_faces().into_iter().find(|(_, info)| info.family == family))
}

/// A builder that sees only what the test gives it: Auto mode, but no system or Office fonts.
fn hermetic() -> FontSetBuilder {
    FontSet::builder().application_dirs(vec![]).system_source(Vec::new)
}

fn with_bundled(family: &str) -> FontSetBuilder {
    hermetic().bundled(vec![embedded(family)])
}

fn dir_with(files: &[(&str, Vec<u8>)]) -> tempfile::TempDir {
    let dir = assert_ok!(tempfile::tempdir());
    for (name, bytes) in files {
        assert_ok!(std::fs::write(dir.path().join(name), bytes));
    }
    dir
}

fn origins_of(set: &FontSet, family: &str) -> Vec<FontOrigin> {
    set.faces()
        .into_iter()
        .filter(|(f, _)| f == family)
        .map(|(_, origin)| origin)
        .collect()
}

/// The path face `index` will be loaded from.
fn source_path(set: &FontSet, index: usize) -> PathBuf {
    let source: &dyn std::any::Any = assert_some!(set.store.source(index));
    assert_some!(source.downcast_ref::<FontPath>()).path.clone()
}

/// A font collection holding the given fonts, each at its own offset with its table offsets shifted.
fn build_ttc(fonts: &[&[u8]]) -> Option<Vec<u8>> {
    let mut out = vec![0u8; 12 + 4 * fonts.len()];
    out[..4].copy_from_slice(b"ttcf");
    out[4..8].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    out[8..12].copy_from_slice(&u32::try_from(fonts.len()).ok()?.to_be_bytes());
    for (i, font) in fonts.iter().enumerate() {
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        let base = u32::try_from(out.len()).ok()?;
        out[12 + 4 * i..16 + 4 * i].copy_from_slice(&base.to_be_bytes());
        out.extend_from_slice(font);
        let tables = usize::from(u16::from_be_bytes([*font.get(4)?, *font.get(5)?]));
        for table in 0..tables {
            // A table record is tag, checksum, offset, length; the offset is from the start of the file.
            let at = usize::try_from(base).ok()? + 12 + 16 * table + 8;
            let offset = u32::from_be_bytes(out.get(at..at + 4)?.try_into().ok()?);
            out[at..at + 4].copy_from_slice(&(offset + base).to_be_bytes());
        }
    }
    Some(out)
}

// ---- layers and order ---------------------------------------------------------------------------

#[test]
fn layer_order_is_bundled_then_font_path_then_env_then_application_then_system() {
    let user = dir_with(&[("u.otf", bytes_of("Libertinus Serif"))]);
    let env = dir_with(&[("e.ttf", bytes_of("DejaVu Sans Mono"))]);
    let app = dir_with(&[("a.otf", bytes_of("New Computer Modern"))]);
    let env_value = assert_ok!(std::env::join_paths([env.path()]));
    let bundled: Vec<_> = typst_kit::fonts::embedded()
        .filter(|(_, i)| i.family == "New Computer Modern Math")
        .take(1)
        .collect();
    // Called out of layer order on purpose: the order of the calls must not matter.
    let set = assert_ok!(
        FontSet::builder()
            .system_source(|| vec![face_of("/sys/s.ttf", "S")])
            .env_paths(Some(env_value.as_os_str()))
            .application_dirs(vec![app.path().to_path_buf()])
            .font_path(user.path())
            .bundled(bundled)
            .build()
    );
    assert_eq!(
        set.faces(),
        [
            ("New Computer Modern Math".to_string(), FontOrigin::Bundled),
            ("Libertinus Serif".to_string(), FontOrigin::FontPath),
            ("DejaVu Sans Mono".to_string(), FontOrigin::FontPath),
            ("New Computer Modern".to_string(), FontOrigin::Application),
            ("S".to_string(), FontOrigin::System),
        ]
    );
}

fn face_of(path: &str, family: &str) -> Scanned {
    (
        FontPath {
            path: PathBuf::from(path),
            index: 0,
        },
        FontInfo {
            family: family.into(),
            ..embedded("Libertinus Serif").1
        },
    )
}

#[test]
fn font_path_directories_keep_command_line_order_and_so_do_env_entries() {
    let (first, second) = (
        dir_with(&[("a.otf", bytes_of("Libertinus Serif"))]),
        dir_with(&[("b.otf", bytes_of("Libertinus Serif"))]),
    );
    let (env_one, env_two) = (
        dir_with(&[("c.otf", bytes_of("Libertinus Serif"))]),
        dir_with(&[("d.otf", bytes_of("Libertinus Serif"))]),
    );
    let env_value = assert_ok!(std::env::join_paths([env_one.path(), env_two.path()]));
    let set = assert_ok!(
        hermetic()
            .bundled(vec![])
            .font_path(second.path())
            .font_path(first.path())
            .env_paths(Some(env_value.as_os_str()))
            .build()
    );
    let canonical = |d: &tempfile::TempDir| assert_ok!(std::fs::canonicalize(d.path()));
    let owners: Vec<PathBuf> = (0..set.origins().len())
        .map(|i| source_path(&set, i).parent().map(Path::to_path_buf).unwrap_or_default())
        .collect();
    assert_eq!(
        owners,
        [
            canonical(&second),
            canonical(&first),
            canonical(&env_one),
            canonical(&env_two)
        ],
        "--font-path in the order given, then RESUME_FONT_PATH in the order given"
    );
}

#[test]
fn faces_on_disk_are_ordered_by_path_whatever_order_the_files_were_made_in() {
    let dir = assert_ok!(tempfile::tempdir());
    for (name, family) in [
        ("z.otf", "Libertinus Serif"),
        ("m.ttf", "DejaVu Sans Mono"),
        ("a.otf", "New Computer Modern"),
    ] {
        assert_ok!(std::fs::write(dir.path().join(name), bytes_of(family)));
    }
    let set = assert_ok!(hermetic().bundled(vec![]).font_path(dir.path()).build());
    let families: Vec<String> = set.faces().into_iter().map(|(f, _)| f).collect();
    assert_eq!(
        families,
        ["New Computer Modern", "DejaVu Sans Mono", "Libertinus Serif"]
    );
}

#[cfg(unix)]
#[test]
fn the_sort_uses_canonical_paths_not_the_spelling_a_symlink_gives() {
    let tmp = assert_ok!(tempfile::tempdir());
    let fonts = tmp.path().join("fonts");
    let elsewhere = tmp.path().join("zzz-real");
    for dir in [fonts.join("mmm"), elsewhere.clone()] {
        assert_ok!(std::fs::create_dir_all(dir));
    }
    assert_ok!(std::fs::write(elsewhere.join("y.otf"), bytes_of("Libertinus Serif")));
    assert_ok!(std::fs::write(fonts.join("mmm/x.ttf"), bytes_of("DejaVu Sans Mono")));
    // Spelled `fonts/aaa/y.otf` it sorts first; its canonical path `zzz-real/y.otf` sorts last.
    assert_ok!(std::os::unix::fs::symlink(&elsewhere, fonts.join("aaa")));
    let set = assert_ok!(hermetic().bundled(vec![]).font_path(&fonts).build());
    let families: Vec<String> = set.faces().into_iter().map(|(f, _)| f).collect();
    assert_eq!(families, ["DejaVu Sans Mono", "Libertinus Serif"]);
}

#[cfg(unix)]
#[test]
fn the_same_file_reached_through_a_symlink_is_one_face() {
    let dir = dir_with(&[]);
    assert_ok!(std::fs::create_dir(dir.path().join("real")));
    assert_ok!(std::fs::write(
        dir.path().join("real/a.otf"),
        bytes_of("Libertinus Serif")
    ));
    assert_ok!(std::os::unix::fs::symlink(
        dir.path().join("real"),
        dir.path().join("alias")
    ));
    let set = assert_ok!(with_bundled("New Computer Modern").font_path(dir.path()).build());
    assert_eq!(
        origins_of(&set, "Libertinus Serif"),
        [FontOrigin::FontPath],
        "the aliased copy was loaded twice"
    );
}

#[cfg(windows)]
#[test]
fn one_file_reached_through_two_case_spellings_is_one_face() {
    let dir = dir_with(&[("libertinus.otf", bytes_of("Libertinus Serif"))]);
    let lower = PathBuf::from(dir.path().to_string_lossy().to_lowercase());
    let upper = PathBuf::from(dir.path().to_string_lossy().to_uppercase());
    let set = assert_ok!(
        with_bundled("New Computer Modern")
            .font_path(&lower)
            .font_path(&upper)
            .build()
    );
    assert_eq!(origins_of(&set, "Libertinus Serif"), [FontOrigin::FontPath]);
}

// ---- what loads ---------------------------------------------------------------------------------

#[test]
fn font_path_adds_fonts() {
    let dir = dir_with(&[("libertinus.otf", bytes_of("Libertinus Serif"))]);
    let set = assert_ok!(with_bundled("New Computer Modern").font_path(dir.path()).build());
    assert_eq!(origins_of(&set, "Libertinus Serif"), [FontOrigin::FontPath]);
    assert_eq!(origins_of(&set, "New Computer Modern"), [FontOrigin::Bundled]);
    let index = assert_some!(set.origins().iter().position(|o| *o == FontOrigin::FontPath));
    assert!(set.loads(index), "the face loads from disk");
}

#[test]
fn ttc_collections_load_with_one_face_per_index() {
    let ttc = assert_some!(build_ttc(&[
        &bytes_of("Libertinus Serif"),
        &bytes_of("DejaVu Sans Mono")
    ]));
    let dir = dir_with(&[("pair.ttc", ttc)]);
    let mut scanned: Vec<_> = typst_kit::fonts::scan(dir.path())
        .map(|(p, i)| (p.index, i.family))
        .collect();
    scanned.sort();
    assert_eq!(
        scanned,
        [(0, "Libertinus Serif".to_string()), (1, "DejaVu Sans Mono".to_string())]
    );

    let set = assert_ok!(hermetic().bundled(vec![]).font_path(dir.path()).build());
    assert_eq!(
        set.faces(),
        [
            ("Libertinus Serif".to_string(), FontOrigin::FontPath),
            ("DejaVu Sans Mono".to_string(), FontOrigin::FontPath)
        ]
    );
    for i in 0..set.origins().len() {
        let font = assert_some!(set.font(i));
        assert_eq!(
            font.info().family,
            set.book().info(i).map(|f| f.family.clone()).unwrap_or_default()
        );
    }
    let indexes: Vec<u32> = (0..set.origins().len())
        .map(|i| {
            let source: &dyn std::any::Any = assert_some!(set.store.source(i));
            assert_some!(source.downcast_ref::<FontPath>()).index
        })
        .collect();
    assert_eq!(indexes, [0, 1], "two faces from one collection, at indexes 0 and 1");
}

#[test]
fn a_font_file_removed_after_the_build_no_longer_loads_and_the_role_falls_back() {
    let dir = dir_with(&[("libertinus.otf", bytes_of("Libertinus Serif"))]);
    let set = assert_ok!(with_bundled("New Computer Modern").font_path(dir.path()).build());
    let role = FontRole::new("body", "Libertinus Serif", "New Computer Modern", Metrics::Different);
    // Removed before anything reads it: a face that was already loaded stays loaded, which is correct.
    assert_ok!(std::fs::remove_file(dir.path().join("libertinus.otf")));
    let index = assert_some!(set.origins().iter().position(|o| *o == FontOrigin::FontPath));
    assert!(!set.loads(index), "a removed file must not count as loadable");
    let after = &set.resolve(&[role])[0];
    assert!(
        matches!(
            after,
            FontResolution::Fallback {
                origin: FontOrigin::Bundled,
                ..
            }
        ),
        "{after:?}"
    );
}

#[test]
fn a_relative_font_path_still_loads_after_the_working_directory_changes() {
    // Cargo runs tests from the package root, so `target/<dir>` is a relative path to a real directory.
    // Cargo runs tests from the package root; `target` may be elsewhere when CARGO_TARGET_DIR is set.
    assert_ok!(std::fs::create_dir_all("target"));
    let tmp = assert_ok!(tempfile::Builder::new().tempdir_in("target"));
    assert_ok!(std::fs::write(tmp.path().join("a.otf"), bytes_of("Libertinus Serif")));
    let relative = PathBuf::from("target").join(assert_some!(tmp.path().file_name()));
    assert!(relative.is_relative());
    let set = assert_ok!(with_bundled("New Computer Modern").font_path(&relative).build());
    let index = assert_some!(set.origins().iter().position(|o| *o == FontOrigin::FontPath));
    assert!(source_path(&set, index).is_absolute(), "{:?}", source_path(&set, index));
    assert!(set.loads(index));
}

#[test]
fn a_directory_with_no_fonts_adds_nothing() {
    let dir = dir_with(&[("notes.txt", b"hello".to_vec()), ("broken.ttf", b"not a font".to_vec())]);
    let set = assert_ok!(with_bundled("New Computer Modern").font_path(dir.path()).build());
    assert_eq!(set.origins(), [FontOrigin::Bundled]);
}

// ---- modes --------------------------------------------------------------------------------------

#[test]
fn bundled_mode_ignores_font_paths_environment_and_bad_directories() {
    let dir = dir_with(&[("libertinus.otf", bytes_of("Libertinus Serif"))]);
    let set = assert_ok!(
        FontSet::builder()
            .mode(FontsMode::Bundled)
            .bundled(vec![embedded("New Computer Modern")])
            .font_path(dir.path())
            .font_path("/definitely/not/here")
            .env_paths(Some(dir.path().as_os_str()))
            .build()
    );
    assert_eq!(set.faces(), [("New Computer Modern".to_string(), FontOrigin::Bundled)]);
}

// ---- the whole path: set -> typesetter -> PDF ---------------------------------------------------

fn resume() -> Resume {
    let path = crate_path("tests/fixtures/full.md");
    let text = assert_ok!(std::fs::read_to_string(&path));
    assert_some!(crate::markdown::parse(&path, &text).into_parts().0)
}

fn pdf_with(set: FontSet) -> Vec<u8> {
    let style = assert_ok!(StyleSource::directory(
        crate_path("tests/fixtures/styles"),
        assert_ok!(StyleName::try_new("font-stack"))
    ));
    let compiled =
        assert_ok!(Typesetter::new(Arc::new(set)).compile(&resume(), &style, RenderOptions::new(Paper::Letter)));
    assert_ok!(compiled.export_pdf("fixture"))
}

#[test]
fn resolved_font_appears_in_the_pdf() {
    let role = FontRole::new("body", "Libertinus Serif", "New Computer Modern", Metrics::Different);
    let dir = dir_with(&[("libertinus.otf", bytes_of("Libertinus Serif"))]);

    // With the preferred font on a --font-path, it is the one embedded.
    let with_path = assert_ok!(with_bundled("New Computer Modern").font_path(dir.path()).build());
    let resolved = with_path.resolve(std::slice::from_ref(&role));
    assert!(
        matches!(
            &resolved[0],
            FontResolution::Preferred {
                origin: FontOrigin::FontPath,
                ..
            }
        ),
        "{resolved:?}"
    );
    let pdf = pdf_with(with_path);
    assert!(
        String::from_utf8_lossy(&pdf).contains("+LibertinusSerif"),
        "the --font-path face was not used"
    );

    // Without it, the stack falls through to the bundled fallback, on any machine.
    let bundled_only = assert_ok!(
        FontSet::builder()
            .mode(FontsMode::Bundled)
            .bundled(vec![embedded("New Computer Modern")])
            .build()
    );
    let resolved = bundled_only.resolve(&[role]);
    assert!(
        matches!(
            &resolved[0],
            FontResolution::Fallback {
                origin: FontOrigin::Bundled,
                ..
            }
        ),
        "{resolved:?}"
    );
    let pdf = pdf_with(bundled_only);
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        text.contains("NewCM10") && !text.contains("LibertinusSerif"),
        "the fallback was not used"
    );
}

// ---- Bundled mode and the fallbacks -------------------------------------------------------------

#[test]
fn bundled_and_hermetic_auto_pdfs_are_byte_identical() {
    // Auto mode with nothing extra found must render exactly like Bundled mode: the mode itself never
    // reaches the output, only the fonts do. The cross-OS PDF gate relies on this.
    let style = assert_ok!(StyleSource::directory(
        crate_path("tests/fixtures/styles"),
        assert_ok!(StyleName::try_new("font-stack-helper"))
    ));
    let render = |set: FontSet| {
        let compiled =
            assert_ok!(Typesetter::new(Arc::new(set)).compile(&resume(), &style, RenderOptions::new(Paper::Letter)));
        assert_ok!(compiled.export_pdf("fixture"))
    };
    let bundled = render(assert_ok!(FontSet::builder().mode(FontsMode::Bundled).build()));
    let hermetic_auto = render(assert_ok!(hermetic().build()));
    assert_eq!(
        bundled.len(),
        hermetic_auto.len(),
        "Bundled and hermetic Auto differ in size"
    );
    assert!(bundled == hermetic_auto, "Bundled and hermetic Auto differ in content");
}

/// Sample text for width comparisons: printable ASCII plus the punctuation a resume uses.
const WIDTH_SAMPLE: &str =
    " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~–—’“”•";

/// Advance of `ch` in em, or `None` when the font has no glyph for it.
fn advance_em(font: &Font, ch: char) -> Option<f64> {
    let instance = font
        .clone()
        .instantiate(font.info().variant, Abs::pt(10.0), &FontVariations::default());
    let glyph = instance.ttf().glyph_index(ch)?.0;
    Some(instance.x_advance(glyph)?.get())
}

/// The font of `family` with this weight and slant in the bundled set.
fn bundled_font(set: &FontSet, family: &str, bold: bool, italic: bool) -> Font {
    let index = assert_some!((0..set.origins().len()).find(|i| {
        set.book().info(*i).is_some_and(|info| {
            info.family == family
                && (info.variant.weight.to_number() >= 600) == bold
                && (info.variant.style == typst::text::FontStyle::Italic) == italic
        })
    }));
    assert_some!(set.font(index))
}

/// Proves the claim the fallbacks exist for: same advance widths as the proprietary originals, so
/// line breaks match. Needs the originals, which are never committed.
#[test]
#[ignore = "needs RESUME_ORIGINAL_FONTS_DIR with Calibri, Georgia, and Arial in four variants named <Family>-<Variant>.ttf"]
fn fallback_widths_match_the_originals() {
    let dir = PathBuf::from(assert_ok!(std::env::var("RESUME_ORIGINAL_FONTS_DIR")));
    let set = FontSet::bundled();
    let variants = [
        ("Regular", false, false),
        ("Bold", true, false),
        ("Italic", false, true),
        ("BoldItalic", true, true),
    ];
    for (original, fallback) in [("Calibri", "Carlito"), ("Georgia", "Gelasio"), ("Arial", "Arimo")] {
        for (name, bold, italic) in variants {
            let path = dir.join(format!("{original}-{name}.ttf"));
            let bytes = assert_ok!(std::fs::read(&path), "{} is required", path.display());
            let original_font = assert_some!(Font::new(Bytes::new(bytes), 0));
            let fallback_font = bundled_font(&set, fallback, bold, italic);
            let worst = WIDTH_SAMPLE
                .chars()
                .filter_map(|ch| Some((ch, advance_em(&original_font, ch)?, advance_em(&fallback_font, ch)?)))
                .map(|(ch, a, b)| (ch, ((a - b) / a).abs()))
                .fold(
                    (' ', 0.0_f64),
                    |worst, next| if next.1 > worst.1 { next } else { worst },
                );
            assert!(
                worst.1 <= 0.002,
                "{fallback} {name} differs from {original} by {:.2}% at {:?}",
                worst.1 * 100.0,
                worst.0
            );
        }
    }
}

// ---- what styles can see, and what Bundled mode keeps out --------------------------------------

/// The PDF of the `font-stack-installed` style (preferred Libertinus Serif, fallback Carlito) over `set`.
fn font_stack_installed_pdf(set: FontSet) -> Vec<u8> {
    let style = assert_ok!(StyleSource::directory(
        crate_path("tests/fixtures/styles"),
        assert_ok!(StyleName::try_new("font-stack-installed"))
    ));
    let compiled =
        assert_ok!(Typesetter::new(Arc::new(set)).compile(&resume(), &style, RenderOptions::new(Paper::Letter)));
    assert!(compiled.warnings().is_empty(), "{:?}", compiled.warnings());
    assert_ok!(compiled.export_pdf("fixture"))
}

#[test]
fn a_font_found_on_a_font_path_reaches_the_style_and_is_used() {
    // Carlito is the only bundled face; Libertinus Serif exists only in the directory.
    let dir = dir_with(&[("libertinus.otf", bytes_of("Libertinus Serif"))]);
    let set = assert_ok!(
        hermetic()
            .bundled(vec![shipped("Carlito")])
            .font_path(dir.path())
            .build()
    );
    assert!(
        crate::typeset::world::installed_families(&set).iter().any(|v| v
            .clone()
            .cast::<typst::foundations::Str>()
            .is_ok_and(|s| s.as_str() == "libertinus serif")),
        "a --font-path family must reach sys.inputs.fonts, lowercased"
    );
    let pdf = font_stack_installed_pdf(set);
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        text.contains("+LibertinusSerif") && !text.contains("+Carlito"),
        "the installed preferred family was not used"
    );
}

#[test]
fn bundled_mode_keeps_a_font_path_family_out_of_the_style_and_the_pdf() {
    let dir = dir_with(&[("libertinus.otf", bytes_of("Libertinus Serif"))]);
    let set = assert_ok!(
        FontSet::builder()
            .mode(FontsMode::Bundled)
            .bundled(vec![shipped("Carlito")])
            .font_path(dir.path())
            .build()
    );
    let pdf = font_stack_installed_pdf(set);
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        text.contains("+Carlito") && !text.contains("LibertinusSerif"),
        "a font path leaked into a bundled-mode PDF"
    );
}
