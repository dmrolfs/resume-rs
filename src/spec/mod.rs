//! ABOUTME: Position specs and profiles: strict TOML files that record the choices for one application.
//! ABOUTME: A spec and its profile chain are folded into one `ResolvedSpec`, so a sent resume can be rebuilt.
//!
//! A *profile* is a reusable, named set of choices; a *position spec* is one application, which extends profiles
//! and names its content file. Both are small TOML files whose unknown keys are errors. `extends` lists profiles
//! from the least to the most specific, and the spec is the most specific of all: when two files choose the same
//! thing, the later one wins. See [`resolve`].

mod adapt;
pub mod chain;
pub mod error;
pub mod layer;
pub mod letter;
pub mod merge;
pub mod names;
pub mod overrides;
mod raw;
mod read;
pub mod resolved;

use std::path::Path;

pub use self::chain::{DirSource, ProfileSource};
pub use self::error::{Located, PickProblem, PickProblemKind, SpecError};
pub use self::layer::{Kind, Layer, Position, SpecLayer};
pub use self::letter::{LetterSettings, ResolvedLetter};
pub use self::merge::{ChoiceKey, Choices, SelectChoices};
pub use self::overrides::{
    Admitted, Applied, AppliedLetter, Chosen, Explicit, LetterChoices, LetterExplicit, Origin, Override, OverrideFlag,
    OverridePolicy, Refused, Replaced, ScopeFlag, Setting, apply, apply_letter, paper_name,
};
pub use self::resolved::{ChainLink, ContentRef, ResolvedSpec};

/// Resolves the spec at `spec`: loads it and the profiles it extends, orders them, and merges their choices.
///
/// Profiles are looked for in `profiles` when given, and otherwise in `profiles` beside the directory that holds
/// the spec (`positions/acme.toml` finds `profiles/`). A spec that extends nothing never touches that directory.
///
/// # Errors
/// A [`SpecError`] naming the file and, where it can, the line and key.
pub fn resolve(spec: &Path, profiles: Option<&Path>) -> Result<ResolvedSpec, SpecError> {
    let layer = SpecLayer::load(spec)?;
    if layer.layer().extends.is_empty() {
        return resolve_with(layer, &DirSource::new(std::path::PathBuf::new()));
    }
    let dir = match profiles {
        Some(given) if !given.is_dir() => {
            return error::ProfilesPathNotADirectorySnafu { given }.fail();
        }
        Some(given) => given.to_path_buf(),
        None => {
            let default = DirSource::default_for(spec);
            if !default.is_dir() {
                return error::ProfilesDirectoryMissingSnafu {
                    spec,
                    searched: error::searched_path(&default),
                }
                .fail();
            }
            default
        }
    };
    resolve_with(layer, &DirSource::new(dir))
}

/// Resolves an already loaded `spec` against any source of profiles.
///
/// # Errors
/// A [`SpecError`] from finding or ordering the chain.
pub fn resolve_with(spec: SpecLayer, source: &dyn ProfileSource) -> Result<ResolvedSpec, SpecError> {
    Ok(ResolvedSpec::from_chain(chain::order(spec, source)?))
}
