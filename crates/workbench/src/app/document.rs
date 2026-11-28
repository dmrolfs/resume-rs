//! A watcher may update disk state, never overwrite an unsaved buffer.
use std::sync::Arc;

/// Last observed bytes, or a file that cannot currently be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiskState {
    Text(Arc<str>),
    Unavailable,
}

pub struct Document {
    available: bool,
    text: Arc<str>,
    disk: DiskState,
    conflict: bool,
}

impl Document {
    pub fn new(text: Arc<str>) -> Self {
        Self {
            available: true,
            disk: DiskState::Text(text.clone()),
            text,
            conflict: false,
        }
    }

    pub fn unavailable() -> Self {
        Self {
            available: false,
            text: Arc::from(""),
            disk: DiskState::Unavailable,
            conflict: false,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn is_dirty(&self) -> bool {
        self.available && self.disk != DiskState::Text(self.text.clone())
    }
    pub fn has_conflict(&self) -> bool {
        self.conflict
    }

    /// All editor changes go through this method; undo may restore clean state.
    pub fn edit(&mut self, text: Arc<str>) {
        self.available = true;
        self.text = text;
        if !self.is_dirty() {
            self.conflict = false;
        }
    }

    pub fn observe(&mut self, disk: DiskState) {
        if disk == self.disk {
            return;
        }
        let replace = !self.is_dirty() && !self.conflict;
        self.disk = disk;
        if self.disk == DiskState::Text(self.text.clone()) {
            self.conflict = false;
        } else if replace && matches!(self.disk, DiskState::Text(_)) {
            self.take_disk();
        } else {
            self.conflict = true;
        }
    }

    pub fn keep_mine(&mut self) {
        self.conflict = false;
    }

    /// A missing or unreadable file is never interpreted as an empty document.
    pub fn take_disk(&mut self) -> bool {
        let DiskState::Text(text) = &self.disk else {
            return false;
        };
        self.available = true;
        self.text = text.clone();
        self.conflict = false;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{DiskState, Document};
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    #[test]
    fn clean_reload_and_identical_notifications() {
        let mut doc = Document::new(Arc::from("original"));
        doc.observe(DiskState::Text(Arc::from("changed")));
        assert_eq!(doc.text(), "changed");
        assert!(!doc.is_dirty());
        doc.edit(Arc::from("mine"));
        doc.observe(DiskState::Text(Arc::from("changed")));
        assert!(!doc.has_conflict());
        assert_eq!(doc.text(), "mine");
    }

    #[test]
    fn conflict_keeps_latest_disk_version_until_explicit_choice() {
        let mut doc = Document::new(Arc::from("original"));
        doc.edit(Arc::from("mine"));
        doc.observe(DiskState::Text(Arc::from("first")));
        doc.observe(DiskState::Text(Arc::from("second")));
        assert_eq!(doc.text(), "mine");
        assert!(doc.has_conflict());
        doc.keep_mine();
        assert!(doc.is_dirty());
        assert!(!doc.has_conflict());
        doc.observe(DiskState::Text(Arc::from("second")));
        assert!(!doc.has_conflict());
        doc.observe(DiskState::Text(Arc::from("third")));
        assert!(doc.take_disk());
        assert_eq!(doc.text(), "third");
        assert!(!doc.is_dirty());
    }

    #[test]
    fn deletion_or_unreadability_never_erases_even_a_clean_buffer() {
        for dirty in [false, true] {
            let mut doc = Document::new(Arc::from("original"));
            if dirty {
                doc.edit(Arc::from("mine"));
            }
            let before = doc.text().to_owned();
            doc.observe(DiskState::Unavailable);
            assert!(doc.has_conflict());
            assert!(!doc.take_disk());
            assert_eq!(doc.text(), before);
            doc.keep_mine();
            assert!(doc.is_dirty());
            doc.observe(DiskState::Text(Arc::from("returned")));
            assert!(doc.has_conflict());
            assert_eq!(doc.text(), before);
        }
    }

    #[test]
    fn undo_to_disk_text_is_clean_and_external_matching_edit_is_not_conflict() {
        let mut doc = Document::new(Arc::from("original"));
        doc.edit(Arc::from("mine"));
        doc.edit(Arc::from("original"));
        assert!(!doc.is_dirty());
        doc.edit(Arc::from("same"));
        doc.observe(DiskState::Text(Arc::from("same")));
        assert!(!doc.is_dirty());
        assert!(!doc.has_conflict());
    }
}
