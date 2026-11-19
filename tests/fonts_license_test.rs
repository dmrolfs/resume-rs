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
        files.len() >= 12,
        "expected the bundled fallback fonts, found {files:?}"
    );
    for (_, relative) in files {
        assert!(
            licenses.contains(&format!("`{relative}`")),
            "{relative} is not listed in fonts/LICENSES.md"
        );
    }
}

#[test]
fn every_font_family_directory_keeps_its_license_text_beside_the_fonts() {
    for (dir, relative) in font_files() {
        let has_license = ["OFL.txt", "GUST-FONT-LICENSE.txt"]
            .iter()
            .any(|name| dir.join(name).is_file());
        assert!(has_license, "{relative} has no license text beside it");
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
fn every_license_text_is_the_real_ofl() {
    for (dir, relative) in font_files() {
        let text = std::fs::read_to_string(dir.join("OFL.txt")).expect("OFL.txt is readable");
        assert!(
            text.contains("SIL OPEN FONT LICENSE Version 1.1"),
            "{relative}: OFL.txt is not the OFL"
        );
        assert!(text.contains("Copyright"), "{relative}: OFL.txt has no copyright line");
        assert!(text.len() > 3_000, "{relative}: OFL.txt is truncated");
    }
}
