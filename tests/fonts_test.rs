//! ABOUTME: The layered font set against real files: --font-path, collections, symlinks, bundled mode, and PDFs.
//! ABOUTME: Faces come from the fonts embedded in typst-kit, copied into temp directories; none is committed.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
use resume::model::options::{FontsMode, Paper};
use resume::model::{Resume, StyleName};
use resume::style::StyleSource;
use resume::typeset::fonts::{FontOrigin, FontSet, Metrics};
use resume::typeset::{RenderOptions, Typesetter};
use typst::text::{Font, FontInfo};

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

/// A bundled layer with a single family, so the other families can only come from a directory.
fn only_bundled(family: &str) -> Vec<(Font, FontInfo)> {
    vec![embedded(family)]
}

fn dir_with(files: &[(&str, Vec<u8>)]) -> tempfile::TempDir {
    let dir = assert_ok!(tempfile::tempdir());
    for (name, bytes) in files {
        assert_ok!(std::fs::write(dir.path().join(name), bytes));
    }
    dir
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

fn origins_of(set: &FontSet, family: &str) -> Vec<FontOrigin> {
    set.faces()
        .into_iter()
        .filter(|(f, _)| f == family)
        .map(|(_, origin)| origin)
        .collect()
}

#[test]
fn font_path_adds_fonts() {
    let dir = dir_with(&[("libertinus.otf", bytes_of("Libertinus Serif"))]);
    let set = assert_ok!(
        FontSet::builder()
            .bundled(only_bundled("New Computer Modern"))
            .font_path(dir.path())
            .build()
    );
    assert_eq!(origins_of(&set, "Libertinus Serif"), [FontOrigin::FontPath]);
    assert_eq!(origins_of(&set, "New Computer Modern"), [FontOrigin::Bundled]);
    let index = assert_some!(set.origins().iter().position(|o| *o == FontOrigin::FontPath));
    assert!(set.loads(index), "the face loads from disk");
}

#[test]
fn ttc_collections_load() {
    let ttc = assert_some!(build_ttc(&[
        &bytes_of("Libertinus Serif"),
        &bytes_of("DejaVu Sans Mono")
    ]));
    let dir = dir_with(&[("pair.ttc", ttc)]);
    let set = assert_ok!(
        FontSet::builder()
            .bundled(only_bundled("New Computer Modern"))
            .font_path(dir.path())
            .build()
    );
    assert_eq!(origins_of(&set, "Libertinus Serif"), [FontOrigin::FontPath]);
    assert_eq!(origins_of(&set, "DejaVu Sans Mono"), [FontOrigin::FontPath]);
    for (i, origin) in set.origins().iter().enumerate() {
        assert!(set.loads(i), "face {i} ({origin}) does not load");
    }
}

#[test]
fn fonts_bundled_mode_ignores_installed_fonts_and_font_paths() {
    let dir = dir_with(&[("libertinus.otf", bytes_of("Libertinus Serif"))]);
    let set = assert_ok!(
        FontSet::builder()
            .mode(FontsMode::Bundled)
            .bundled(only_bundled("New Computer Modern"))
            .font_path(dir.path())
            .font_path("/definitely/not/here")
            .env_paths(Some(dir.path().as_os_str()))
            .build()
    );
    assert!(
        set.origins().iter().all(|o| *o == FontOrigin::Bundled),
        "{:?}",
        set.origins()
    );
    assert_eq!(set.families(), ["New Computer Modern"]);
}

#[test]
fn the_default_bundled_mode_set_has_only_bundled_faces_on_any_machine() {
    let bundled = assert_ok!(FontSet::builder().mode(FontsMode::Bundled).build());
    assert_eq!(bundled.origins().len(), FontSet::bundled().origins().len());
    assert!(bundled.origins().iter().all(|o| *o == FontOrigin::Bundled));
}

#[test]
fn auto_mode_keeps_the_bundled_faces_first_whatever_is_installed() {
    let auto = assert_ok!(FontSet::builder().build());
    let bundled = FontSet::bundled().origins().len();
    assert!(auto.origins().len() >= bundled);
    assert!(auto.origins()[..bundled].iter().all(|o| *o == FontOrigin::Bundled));
    assert!(auto.origins()[bundled..].iter().all(|o| *o != FontOrigin::Bundled));
}

#[cfg(unix)]
#[test]
fn a_symlink_into_the_same_directory_adds_no_duplicate_faces() {
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
    let set = assert_ok!(
        FontSet::builder()
            .bundled(only_bundled("New Computer Modern"))
            .font_path(dir.path())
            .build()
    );
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
        FontSet::builder()
            .bundled(only_bundled("New Computer Modern"))
            .font_path(&lower)
            .font_path(&upper)
            .build()
    );
    assert_eq!(origins_of(&set, "Libertinus Serif"), [FontOrigin::FontPath]);
}

// ---- the whole path: set -> typesetter -> PDF ----------------------------------------------------

fn resume() -> Resume {
    let path = crate_path("tests/fixtures/full.md");
    let text = assert_ok!(std::fs::read_to_string(&path));
    assert_some!(resume::markdown::parse(&path, &text).into_parts().0)
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
    let role = |name: &str| resume::typeset::fonts::FontRole {
        name: "body".into(),
        preferred: name.into(),
        fallback: "New Computer Modern".into(),
        metrics: Metrics::Different,
    };
    let dir = dir_with(&[("libertinus.otf", bytes_of("Libertinus Serif"))]);

    // With the preferred font on a --font-path, it is the one embedded.
    let with_path = assert_ok!(
        FontSet::builder()
            .bundled(only_bundled("New Computer Modern"))
            .font_path(dir.path())
            .build()
    );
    let resolved = with_path.resolve(&[role("Libertinus Serif")]);
    assert!(
        matches!(
            &resolved[0],
            resume::typeset::fonts::FontResolution::Preferred {
                origin: FontOrigin::FontPath,
                ..
            }
        ),
        "{resolved:?}"
    );
    let pdf = pdf_with(with_path);
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("+LibertinusSerif"), "the --font-path face was not used");

    // Without it, the stack falls through to the bundled fallback, on any machine.
    let bundled_only = assert_ok!(
        FontSet::builder()
            .mode(FontsMode::Bundled)
            .bundled(only_bundled("New Computer Modern"))
            .build()
    );
    let resolved = bundled_only.resolve(&[role("Libertinus Serif")]);
    assert!(
        matches!(
            &resolved[0],
            resume::typeset::fonts::FontResolution::Fallback {
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

#[test]
fn only_directories_are_font_paths() {
    let dir = dir_with(&[("libertinus.otf", bytes_of("Libertinus Serif"))]);
    let file: &Path = &dir.path().join("libertinus.otf");
    assert!(FontSet::builder().font_path(file).build().is_err());
}
