//! ABOUTME: Guards the license bookkeeping for the fonts embedded in the binary.
//! ABOUTME: Every font file must be listed in fonts/LICENSES.md and have a license text beside it.

use std::path::PathBuf;

fn fonts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fonts")
}

/// Every `.ttf` and `.otf` under `fonts/`, as `family-dir/file-name`.
fn font_files() -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    for family in std::fs::read_dir(fonts_dir()).expect("fonts/ exists").flatten() {
        if !family.path().is_dir() {
            continue;
        }
        for file in std::fs::read_dir(family.path())
            .expect("a family directory is readable")
            .flatten()
        {
            let name = file.file_name().to_string_lossy().into_owned();
            if name.ends_with(".ttf") || name.ends_with(".otf") {
                found.push((
                    family.path(),
                    format!("{}/{name}", family.file_name().to_string_lossy()),
                ));
            }
        }
    }
    found.sort();
    found
}

#[test]
fn every_embedded_font_file_is_listed_in_licenses_md() {
    let licenses = std::fs::read_to_string(fonts_dir().join("LICENSES.md")).expect("fonts/LICENSES.md exists");
    let files = font_files();
    assert!(
        files.len() == 16,
        "expected the bundled fallback fonts, found {files:?}"
    );
    for (_, relative) in files {
        assert!(
            licenses.contains(&format!("`{relative}`")),
            "{relative} is not listed in fonts/LICENSES.md"
        );
    }
}

/// The license text each family directory must keep beside its fonts.
fn expected_license(family_dir: &str) -> &'static str {
    match family_dir {
        "pagella" => "GUST-FONT-LICENSE.txt",
        _ => "OFL.txt",
    }
}

#[test]
fn every_font_family_directory_keeps_its_own_license_text_beside_the_fonts() {
    for (dir, relative) in font_files() {
        let family = relative.split('/').next().expect("family/file");
        let name = expected_license(family);
        assert!(dir.join(name).is_file(), "{relative} has no {name} beside it");
    }
}

fn sha256_hex(path: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).expect("a font file is readable");
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn every_font_file_is_the_unmodified_file_listed_in_licenses_md() {
    // "Unmodified" is the Reserved Font Name obligation for Carlito, so the bytes are tied to the table.
    let licenses = std::fs::read_to_string(fonts_dir().join("LICENSES.md")).expect("fonts/LICENSES.md exists");
    for (_, relative) in font_files() {
        let digest = sha256_hex(&fonts_dir().join(&relative));
        let listed = licenses.contains(&format!("`{relative}` `{digest}`"));
        assert!(
            listed,
            "{relative} does not match the SHA-256 in fonts/LICENSES.md (now {digest})"
        );
    }
}

#[test]
fn every_ofl_license_text_is_the_real_ofl() {
    for (dir, relative) in font_files()
        .into_iter()
        .filter(|(dir, _)| dir.join("OFL.txt").is_file())
    {
        let text = std::fs::read_to_string(dir.join("OFL.txt")).expect("OFL.txt is readable");
        assert!(
            text.contains("SIL OPEN FONT LICENSE Version 1.1"),
            "{relative}: OFL.txt is not the OFL"
        );
        assert!(text.contains("Copyright"), "{relative}: OFL.txt has no copyright line");
        assert!(text.len() > 3_000, "{relative}: OFL.txt is truncated");
    }
}

#[test]
fn pagella_keeps_the_gust_license_readme_and_manifest_beside_the_fonts() {
    let dir = fonts_dir().join("pagella");
    let license = std::fs::read_to_string(dir.join("GUST-FONT-LICENSE.txt")).expect("the GUST license is kept");
    assert!(license.contains("version 1.0, dated 22 June 2009, of the GUST Font License"));
    assert!(license.contains("LaTeX Project Public License"));
    for name in ["README-TeX-Gyre-Pagella.txt", "MANIFEST-TeX-Gyre-Pagella.txt"] {
        let text = std::fs::read_to_string(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(text.contains("Version: 2.501"), "{name} is not the 2.501 file");
    }
}

/// The text of one `## ` section of `LICENSES.md`, from its heading to the next one.
fn section_of(licenses: &str, heading: &str) -> String {
    let start = licenses
        .find(heading)
        .unwrap_or_else(|| panic!("LICENSES.md has no section `{heading}`"));
    let rest = &licenses[start + heading.len()..];
    rest.split("\n## ").next().unwrap_or(rest).to_string()
}

#[test]
fn licenses_md_states_the_three_lppl_conditions_for_pagella() {
    let licenses = std::fs::read_to_string(fonts_dir().join("LICENSES.md")).expect("fonts/LICENSES.md exists");
    // The conditions are in the section about them and the entry is in Pagella's own section, so another
    // family's text cannot satisfy either check.
    let conditions = section_of(&licenses, "## What the GUST Font License and the LPPL ask");
    for needle in [
        "No change was made",
        "No endorsement by the authors is implied",
        "https://www.gust.org.pl/projects/e-foundry/tex-gyre/pagella",
        "https://ctan.org/pkg/tex-gyre",
    ] {
        assert!(conditions.contains(needle), "the Pagella conditions lack `{needle}`");
    }
    let entry = section_of(&licenses, "## TeX Gyre Pagella 2.501");
    for needle in [
        "Copyright 2006-2018 for TeX Gyre extensions by B. Jackowski, J.M. Nowacki, et al.",
        "GUST Font License v1.0",
        "pagella/GUST-FONT-LICENSE.txt",
    ] {
        assert!(entry.contains(needle), "the Pagella entry lacks `{needle}`");
    }
}

#[test]
fn the_pagella_notice_files_are_whole_and_listed_with_their_checksums() {
    // A truncated README or MANIFEST would still say `Version: 2.501`; the checksum notices that it is not the file.
    let licenses = std::fs::read_to_string(fonts_dir().join("LICENSES.md")).expect("fonts/LICENSES.md exists");
    for name in [
        "GUST-FONT-LICENSE.txt",
        "README-TeX-Gyre-Pagella.txt",
        "MANIFEST-TeX-Gyre-Pagella.txt",
    ] {
        let digest = sha256_hex(&fonts_dir().join("pagella").join(name));
        assert!(
            licenses.contains(&format!("`pagella/{name}` `{digest}`")),
            "pagella/{name} does not match the SHA-256 in fonts/LICENSES.md (now {digest})"
        );
    }
}

// ============================================================================
// The notices the binary prints (`resume licenses`) cannot drift from the fonts it embeds
// ============================================================================

use claims::{assert_ok, assert_some};
use resume::typeset::fonts::{FontSet, bundled_font_notices};

/// The license text a bundled family directory keeps beside its fonts: the GUST Font License for TeX Gyre
/// Pagella, the OFL for the others.
fn license_text(family_dir: &str) -> String {
    let name = if family_dir == "pagella" { "GUST-FONT-LICENSE.txt" } else { "OFL.txt" };
    assert_ok!(std::fs::read_to_string(fonts_dir().join(family_dir).join(name)))
}

fn vendored_notice() -> String {
    assert_ok!(std::fs::read_to_string(fonts_dir().join("typst-assets").join("NOTICE")))
}

#[test]
fn the_notices_carry_the_typst_assets_notice_in_full() {
    // Apache-2.0 section 4(d): redistributing typst-assets means carrying a readable copy of its NOTICE, which
    // is where the licenses of the fonts Typst embeds (Libertinus, New Computer Modern, DejaVu) are.
    let notice = vendored_notice();
    assert!(notice.len() > 50_000, "the vendored NOTICE is truncated");
    assert!(bundled_font_notices().contains(&notice));
}

#[test]
fn the_notices_carry_every_bundled_license_text_in_full() {
    let notices = bundled_font_notices();
    for family in ["carlito", "gelasio", "arimo", "pagella"] {
        assert!(
            notices.contains(&license_text(family)),
            "{family}'s license text is not in the notices"
        );
    }
}

#[test]
fn the_notices_cover_every_bundled_family() {
    // For each family: the name its license text uses, and which text carries it (the vendored NOTICE for the
    // fonts Typst embeds, or the family's own `OFL.txt`). A family that is not in this table has no license
    // text in the binary yet, so adding a bundled font fails here until its license is added and listed.
    const TYPST_ASSETS: &str = "typst-assets";
    const COVERED: &[(&str, &str, &str)] = &[
        ("arimo", "Arimo", "arimo"),
        ("carlito", "Carlito", "carlito"),
        ("gelasio", "Gelasio", "gelasio"),
        ("tex gyre pagella", "GUST Font License", "pagella"),
        ("libertinus serif", "Libertinus", TYPST_ASSETS),
        ("new computer modern", "NewComputerModern", TYPST_ASSETS),
        ("new computer modern math", "NewComputerModern", TYPST_ASSETS),
        ("dejavu sans mono", "DejaVu", TYPST_ASSETS),
    ];
    let notices = bundled_font_notices();
    let families = FontSet::bundled().families();
    assert!(
        families.len() >= 8,
        "expected the bundled and Typst fonts, found {families:?}"
    );
    for family in families {
        let (_, needle, source) = assert_some!(
            COVERED.iter().find(|(name, ..)| *name == family.to_lowercase()),
            "the bundled family `{family}` has no license text in the notices: add it, then list it here"
        );
        let text = if *source == TYPST_ASSETS {
            vendored_notice()
        } else {
            license_text(source)
        };
        assert!(
            text.contains(needle),
            "the license text for {family} does not mention {needle}"
        );
        assert!(
            notices.contains(&text),
            "the notices do not carry the license text for {family}"
        );
    }
}

#[test]
fn the_vendored_typst_assets_notice_is_the_one_of_the_locked_crate() {
    // A Typst upgrade that changes the notice (a new font, a new license) fails here until it is vendored again.
    let output = assert_ok!(
        std::process::Command::new(env!("CARGO"))
            .args(["metadata", "--format-version", "1", "--locked", "--offline"])
            .output()
    );
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = assert_ok!(serde_json::from_slice(&output.stdout));
    let packages = assert_some!(metadata["packages"].as_array());
    let locked: Vec<&serde_json::Value> = packages.iter().filter(|p| p["name"] == "typst-assets").collect();
    assert!(!locked.is_empty(), "typst-assets is not in the dependency graph");
    for package in locked {
        let manifest = assert_some!(package["manifest_path"].as_str());
        let notice = std::path::Path::new(manifest).with_file_name("NOTICE");
        assert_eq!(
            assert_ok!(std::fs::read(&notice)),
            assert_ok!(std::fs::read(fonts_dir().join("typst-assets").join("NOTICE"))),
            "fonts/typst-assets/NOTICE differs from {}",
            notice.display()
        );
    }
}

#[test]
fn the_vendored_notice_is_never_line_ending_converted() {
    // Compared byte for byte with the crate's file, so a Windows checkout must not touch it.
    let attributes = assert_ok!(std::fs::read_to_string(
        fonts_dir()
            .parent()
            .expect("fonts/ has a parent")
            .join(".gitattributes")
    ));
    assert!(
        attributes
            .lines()
            .any(|line| line.trim() == "fonts/typst-assets/NOTICE -text"),
        ".gitattributes does not stop line-ending conversion of the vendored NOTICE"
    );
    assert!(!vendored_notice().contains('\r'));
}
