//! ABOUTME: Reads the labeled metadata a style's components leave in the laid-out document.
//! ABOUTME: Font roles come from `<resume-fonts>`; role and era placement from `<resume-role>` and `<resume-era>`.

use std::num::NonZeroUsize;

use typst::foundations::{Content, Dict, Label, Selector, Str, Value};
use typst::introspection::{Introspector, MetadataElem};
use typst::utils::PicoStr;
use typst_layout::PagedDocument;

use crate::error::{Error, ProbeSnafu};
use crate::typeset::fonts::{FontRole, Metrics};

/// A role or era heading and where it landed.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub kind: ProbeKind,
    /// 1-based page.
    pub page: NonZeroUsize,
    /// Distance from the top of the page, in points.
    pub y: f64,
    /// Height of that page, in points.
    pub page_height: f64,
}

/// Which heading a [`Probe`] marks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeKind {
    Role {
        /// Position of the role in the document, from 0.
        index: usize,
        company: String,
        title: Option<String>,
    },
    Era {
        /// The index of the role this era belongs to.
        role: usize,
        /// Position of the era within its role, from 0.
        index: usize,
        name: String,
    },
}

const FONTS: &str = "resume-fonts";
const ROLE: &str = "resume-role";
const ERA: &str = "resume-era";

/// A probe whose metadata does not have the documented shape. A style is user-editable code, so this is
/// an error the user can fix, not a bug.
fn malformed(label: &'static str, detail: impl std::fmt::Display) -> Error {
    ProbeSnafu {
        label,
        detail: detail.to_string(),
    }
    .build()
}

fn select(label: &'static str) -> Selector {
    Selector::Label(Label::new(PicoStr::intern(label)).expect("labels are not empty"))
}

fn metadata_value(content: &Content, label: &'static str) -> Result<Value, Error> {
    content
        .to_packed::<MetadataElem>()
        .map(|m| m.value.clone())
        .ok_or_else(|| malformed(label, "the labeled element is not metadata"))
}

fn dict(value: Value, label: &'static str) -> Result<Dict, Error> {
    value.cast::<Dict>().map_err(|e| malformed(label, e.message()))
}

fn field(d: &Dict, key: &str, label: &'static str) -> Result<Value, Error> {
    d.get(key).cloned().map_err(|e| malformed(label, e))
}

fn string(d: &Dict, key: &str, label: &'static str) -> Result<String, Error> {
    field(d, key, label)?
        .cast::<Str>()
        .map(|s| s.to_string())
        .map_err(|e| malformed(label, format!("`{key}`: {}", e.message())))
}

fn index(d: &Dict, key: &str, label: &'static str) -> Result<usize, Error> {
    let n = field(d, key, label)?
        .cast::<i64>()
        .map_err(|e| malformed(label, format!("`{key}`: {}", e.message())))?;
    usize::try_from(n).map_err(|_| malformed(label, format!("`{key}` is negative")))
}

/// The font roles the style declared in `theme.fonts`, in declaration order.
pub(crate) fn font_roles(document: &PagedDocument) -> Result<Vec<FontRole>, Error> {
    let Some(content) = document.introspector().query_first(&select(FONTS)) else {
        return Ok(Vec::new());
    };
    dict(metadata_value(&content, FONTS)?, FONTS)?
        .iter()
        .map(|(name, spec)| {
            let spec = dict(spec.clone(), FONTS)?;
            let compatible = field(&spec, "metric-compatible", FONTS)?
                .cast::<bool>()
                .map_err(|e| malformed(FONTS, format!("`metric-compatible`: {}", e.message())))?;
            Ok(FontRole::new(
                name.as_str(),
                string(&spec, "preferred", FONTS)?,
                string(&spec, "fallback", FONTS)?,
                if compatible {
                    Metrics::Compatible
                } else {
                    Metrics::Different
                },
            ))
        })
        .collect()
}

/// Every role and era heading with its page and position, in document order.
pub(crate) fn probes(document: &PagedDocument) -> Result<Vec<Probe>, Error> {
    let introspector = document.introspector();
    let mut out = Vec::new();
    for label in [ROLE, ERA] {
        for content in introspector.query(&select(label)) {
            let d = dict(metadata_value(&content, label)?, label)?;
            let Some(position) = content.location().and_then(|l| introspector.position(l)) else {
                continue; // not placed on a page, e.g. inside a hidden element
            };
            let kind = if label == ROLE {
                ProbeKind::Role {
                    index: index(&d, "index", label)?,
                    company: string(&d, "company", label)?,
                    title: string(&d, "title", label).ok(),
                }
            } else {
                ProbeKind::Era {
                    role: index(&d, "role", label)?,
                    index: index(&d, "index", label)?,
                    name: string(&d, "name", label)?,
                }
            };
            let page_height = document.pages()[position.page.get() - 1].frame.height().to_pt();
            out.push(Probe {
                kind,
                page: position.page,
                y: position.point.y.to_pt(),
                page_height,
            });
        }
    }
    out.sort_by(|a, b| a.page.cmp(&b.page).then(a.y.total_cmp(&b.y)));
    Ok(out)
}
