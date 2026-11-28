//! Resolve profiles against the same bytes displayed by the file inventory.
use resume::spec::{
    SpecError,
    chain::{DirSource, ProfileSource},
    error::Located,
    layer::{Kind, Layer},
    names::ProfileName,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::Read as _,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Default)]
pub struct Snapshots {
    pub text: RefCell<BTreeMap<PathBuf, Arc<str>>>,
    pub failures: RefCell<BTreeMap<PathBuf, Vec<super::problem::Problem>>>,
}

pub struct Profiles<'a> {
    pub directory: DirSource,
    pub snapshots: &'a Snapshots,
}
impl ProfileSource for Profiles<'_> {
    fn find(&self, name: &ProfileName, at: &Located) -> Result<Layer, SpecError> {
        let result = self.directory.find_with(name, at, |open, shown| {
            let key = resume::spec::error::tidy_path(shown);
            let existing = self.snapshots.text.borrow().get(&key).cloned();
            let text = match existing {
                Some(text) => text,
                None => {
                    let text = read(open, &key)?;
                    let bytes: usize = self.snapshots.text.borrow().values().map(|text| text.len()).sum();
                    if bytes + text.len() > 32 * 1024 * 1024 {
                        return Err(SpecError::Read {
                            path: key,
                            source: std::io::Error::other(
                                "Folder and dependency text exceed the 32 MiB display budget",
                            ),
                        });
                    }
                    self.snapshots.text.borrow_mut().insert(key.clone(), text.clone());
                    text
                }
            };
            Layer::from_text(Kind::Profile, &key, &text)
        });
        if let Err(error) = &result {
            let path = match error {
                SpecError::Read { path, .. }
                | SpecError::TooLarge { path, .. }
                | SpecError::NotUtf8 { path, .. }
                | SpecError::NotAFile { path }
                | SpecError::ProfileOutsideRoot { path, .. } => Some(path.as_path()),
                _ => None,
            };
            if let Some(path) = path {
                let key = resume::spec::error::tidy_path(path);
                self.snapshots
                    .failures
                    .borrow_mut()
                    .insert(key, super::problem::Problem::spec(path, error));
            }
        }
        result
    }
}

#[cfg(test)]
#[path = "profiles_tests.rs"]
mod tests;

fn read(open: &Path, path: &Path) -> Result<Arc<str>, SpecError> {
    let metadata = std::fs::metadata(open).map_err(|source| SpecError::Read {
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(SpecError::NotAFile { path: path.to_owned() });
    }
    let file = std::fs::File::open(open).map_err(|source| SpecError::Read {
        path: path.to_owned(),
        source,
    })?;
    let mut bytes = vec![];
    file.take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|source| SpecError::Read {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() > 65_536 {
        return Err(SpecError::TooLarge {
            path: path.to_owned(),
            len: bytes.len() as u64,
        });
    }
    let text = std::str::from_utf8(&bytes).map_err(|e| SpecError::NotUtf8 {
        path: path.to_owned(),
        valid_up_to: e.valid_up_to(),
    })?;
    Ok(Arc::from(text))
}
