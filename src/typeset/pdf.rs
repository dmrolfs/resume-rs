//! ABOUTME: PDF export settings that make output a pure function of the document and the file stem.
//! ABOUTME: No timestamp, a caller-supplied identifier, a fixed creator string, and tagged output.

use typst::diag::SourceResult;
use typst::foundations::Smart;
use typst_layout::PagedDocument;
use typst_pdf::PdfOptions;

/// The `/Creator` entry written into every PDF.
fn creator() -> String {
    format!("resume-rs {}", env!("CARGO_PKG_VERSION"))
}

/// Options for a deterministic export. `ident` should be stable for the same logical document.
pub(crate) fn pdf_options(ident: &str) -> PdfOptions {
    PdfOptions {
        ident: Smart::Custom(ident.to_string()),
        creator: Smart::Custom(Some(creator())),
        timestamp: None,
        tagged: true,
        ..PdfOptions::default()
    }
}

pub(crate) fn export(document: &PagedDocument, ident: &str) -> SourceResult<Vec<u8>> {
    typst_pdf::pdf(document, &pdf_options(ident))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn pdf_options_are_deterministic() {
        let options = pdf_options("jane-doe");
        assert_eq!(options.ident, Smart::Custom("jane-doe".to_string()));
        assert_eq!(
            options.creator,
            Smart::Custom(Some(format!("resume-rs {}", env!("CARGO_PKG_VERSION"))))
        );
        assert!(
            options.timestamp.is_none(),
            "a timestamp would make output date-dependent"
        );
        assert!(options.tagged, "tagged output keeps the reading order accessible");
        assert!(options.page_ranges.is_none());
    }
}
