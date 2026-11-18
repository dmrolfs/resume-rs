//! ABOUTME: The fonts a render may use, in a fixed order, served lazily to Typst.
//! ABOUTME: For now only the fonts embedded in typst-kit; layering and role resolution come in rsm-141.18.

use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst_kit::fonts::FontStore;

/// An ordered set of fonts shared by every render.
///
/// Order matters: Typst breaks a tie between faces of one family toward the earliest index, so
/// the set is built once, in a fixed order, and shared behind an `Arc`.
pub struct FontSet {
    store: FontStore,
}

impl FontSet {
    /// The fonts embedded in typst-kit: Libertinus Serif, New Computer Modern, and DejaVu Sans Mono.
    #[must_use]
    pub fn bundled() -> Self {
        let mut store = FontStore::new();
        store.extend(typst_kit::fonts::embedded());
        Self { store }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn bundled_set_contains_libertinus_serif() {
        let families = FontSet::bundled().families();
        assert!(families.contains(&"Libertinus Serif".to_string()), "{families:?}");
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
    fn every_font_in_the_book_loads() {
        let set = FontSet::bundled();
        let count = set.book().families().map(|(_, infos)| infos.count()).sum::<usize>();
        assert!(count > 0);
        for index in 0..count {
            assert!(set.font(index).is_some(), "font {index} did not load");
        }
        assert!(set.font(count).is_none());
    }
}
