//! ABOUTME: The font set's public API on any machine: bundled mode is machine-independent, Auto never reorders bundled faces.
//! ABOUTME: Tests that need exact layers use injected sources inside `src/typeset/fonts.rs`, so they never read this machine's fonts.

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
use resume::model::options::{FontOrigin, FontsMode};
use resume::typeset::fonts::{FontResolution, FontRole, FontSet, Metrics};

fn embedded_bytes(family: &str) -> Vec<u8> {
    let (font, _) = assert_some!(typst_kit::fonts::embedded().find(|(_, info)| info.family == family));
    font.data().as_slice().to_vec()
}

#[test]
fn the_bundled_mode_set_has_only_bundled_faces_on_any_machine() {
    let bundled = assert_ok!(FontSet::builder().mode(FontsMode::Bundled).build());
    assert_eq!(bundled.origins().len(), FontSet::bundled().origins().len());
    assert!(bundled.origins().iter().all(|o| *o == FontOrigin::Bundled));
    assert_eq!(bundled.mode(), FontsMode::Bundled);
}

#[test]
fn bundled_mode_ignores_font_paths_the_environment_and_bad_directories() {
    let dir = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::write(
        dir.path().join("libertinus.otf"),
        embedded_bytes("Libertinus Serif")
    ));
    let set = assert_ok!(
        FontSet::builder()
            .mode(FontsMode::Bundled)
            .font_path(dir.path())
            .font_path("/definitely/not/here")
            .env_paths(Some(dir.path().as_os_str()))
            .build()
    );
    assert_eq!(
        set.origins().len(),
        FontSet::bundled().origins().len(),
        "a face was added in bundled mode"
    );
    assert!(set.origins().iter().all(|o| *o == FontOrigin::Bundled));
}

#[test]
fn auto_mode_keeps_the_bundled_faces_first_whatever_is_installed() {
    // Informational on a machine without fonts; the exact layering is tested with injected sources.
    let auto = assert_ok!(FontSet::builder().build());
    let bundled = FontSet::bundled().origins().len();
    assert!(auto.origins().len() >= bundled);
    assert!(auto.origins()[..bundled].iter().all(|o| *o == FontOrigin::Bundled));
    assert!(auto.origins()[bundled..].iter().all(|o| *o != FontOrigin::Bundled));
    assert_eq!(auto.mode(), FontsMode::Auto);
}

#[test]
fn a_role_resolves_through_the_public_api() {
    let set = FontSet::bundled();
    let roles = [
        FontRole::new("body", "Libertinus Serif", "New Computer Modern", Metrics::Different),
        FontRole::new("code", "Not Installed", "DejaVu Sans Mono", Metrics::Compatible),
        FontRole::new("label", "Not Installed", "Also Not Installed", Metrics::Compatible),
    ];
    let resolved = set.resolve(&roles);
    assert!(
        matches!(
            &resolved[0],
            FontResolution::Preferred {
                origin: FontOrigin::Bundled,
                ..
            }
        ),
        "{resolved:?}"
    );
    assert!(
        matches!(&resolved[1], FontResolution::Fallback { family, .. } if family == "DejaVu Sans Mono"),
        "{resolved:?}"
    );
    assert!(matches!(&resolved[2], FontResolution::Missing { .. }), "{resolved:?}");
    assert_eq!(
        resolved.iter().map(FontResolution::role).collect::<Vec<_>>(),
        ["body", "code", "label"]
    );
}
