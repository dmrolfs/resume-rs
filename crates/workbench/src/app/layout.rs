//! Layouts are app preferences, never files written into the resume folder.
use gpui_kit::component::dock::DockAreaState;
use sha2::{Digest, Sha256};
use snafu::{ResultExt, Snafu};
use std::{
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

#[derive(Debug, Snafu)]
pub enum LayoutError {
    #[snafu(display("Cannot locate the OS configuration directory"))]
    NoConfig,
    #[snafu(display("Cannot read folder layout {}", path.display()))]
    Read { path: PathBuf, source: std::io::Error },
    #[snafu(display("Invalid folder layout {}", path.display()))]
    Decode { path: PathBuf, source: serde_json::Error },
    #[snafu(display("Cannot encode folder layout"))]
    Encode { source: serde_json::Error },
    #[snafu(display("Cannot save folder layout"))]
    Write { source: std::io::Error },
    #[snafu(display("Cannot replace saved layout"))]
    Persist { source: tempfile::PersistError },
    #[snafu(display("Folder layout is not a regular file"))]
    NotAFile,
    #[snafu(display("Folder layout exceeds 1 MiB"))]
    TooLarge,
}

pub fn path(root: &Path) -> Result<PathBuf, LayoutError> {
    let config = dirs::config_dir().ok_or(LayoutError::NoConfig)?;
    let hash: String = Sha256::digest(root.as_os_str().as_encoded_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(config
        .join("resume-workbench")
        .join("folders")
        .join(format!("{hash}.json")))
}

pub fn load(path: &Path) -> Result<Option<DockAreaState>, LayoutError> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(LayoutError::Read {
                path: path.to_owned(),
                source,
            });
        }
    };
    if !metadata.is_file() {
        return Err(LayoutError::NotAFile);
    }
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(LayoutError::Read {
                path: path.to_owned(),
                source,
            });
        }
    };
    let mut bytes = vec![];
    file.take(1_048_577)
        .read_to_end(&mut bytes)
        .context(ReadSnafu { path })?;
    if bytes.len() > 1_048_576 {
        return Err(LayoutError::TooLarge);
    }
    let state: DockAreaState = serde_json::from_slice(&bytes).context(DecodeSnafu { path })?;
    Ok(Some(state))
}

pub fn save(path: &Path, state: &DockAreaState) -> Result<(), LayoutError> {
    let bytes = serde_json::to_vec(state).context(EncodeSnafu)?;
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).context(WriteSnafu)?;
    // NamedTempFile starts private (0600 on Unix), unlike generated PDF outputs.
    let mut file = tempfile::NamedTempFile::new_in(parent).context(WriteSnafu)?;
    file.write_all(&bytes).context(WriteSnafu)?;
    file.as_file().sync_all().context(WriteSnafu)?;
    file.persist(path).context(PersistSnafu)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{load, path};
    use claims::{assert_err, assert_ok};
    #[cfg(unix)]
    #[test]
    fn replacement_keeps_layout_paths_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = assert_ok!(tempfile::tempdir());
        let file = dir.path().join("layout.json");
        for _ in 0..2 {
            assert_ok!(super::save(&file, &Default::default()));
            assert_eq!(assert_ok!(std::fs::metadata(&file)).permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn folders_have_distinct_keys_and_corrupt_state_is_not_silently_accepted() {
        let dir = assert_ok!(tempfile::tempdir());
        assert_ne!(
            assert_ok!(path(&dir.path().join("a"))),
            assert_ok!(path(&dir.path().join("b")))
        );
        let file = dir.path().join("bad.json");
        assert!(assert_ok!(load(&file)).is_none());
        assert_ok!(std::fs::write(&file, "bad"));
        assert_err!(load(&file));
        assert_err!(load(dir.path()));
    }
}
