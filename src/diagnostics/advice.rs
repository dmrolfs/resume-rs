//! ABOUTME: The advice catalog: one summary, rationale, fix, and example per diagnostic code.
//! ABOUTME: Format codes are filled in; Content, Layout, and Setup advice is written by the tasks that own them.

use super::code::{Advice, DiagnosticCode};

impl DiagnosticCode {
    /// The advice for this code, or `None` until the task that owns it writes it.
    ///
    /// The match is exhaustive with no wildcard, so adding a code forces a decision here.
    #[must_use]
    pub const fn advice(self) -> Option<&'static Advice> {
        use DiagnosticCode::*;
        match self {
            MissingName => Some(&Advice {
                summary: "The resume has no `# Name` heading.",
                why: "Project convention: the first-level heading is the name; styles use it as the title and the footer text.",
                fix: "Start the file with `# Your Name`; comments may come before it.",
                example: Some((
                    "## Summary\n\nSenior engineer.",
                    "# Ada Lovelace\n\nLondon · ada@example.com\n\n## Summary\n\nSenior engineer.",
                )),
            }),
            DuplicateName => Some(&Advice {
                summary: "A resume has exactly one `# ` heading.",
                why: "Project convention: the single `# ` heading is the name; sections start at `## `.",
                fix: "Change the extra `# ` heading to `## `, or remove it.",
                example: Some(("# Experience", "## Experience")),
            }),
            ContentBeforeSection => Some(&Advice {
                summary: "Content appears before the first `## ` section.",
                why: "Project convention: only the name and one contact line come before the first section.",
                fix: "Move the content under a `## ` section.",
                example: Some((
                    "# Ada\n\nLondon\n\nSenior engineer.\n\n## Experience",
                    "# Ada\n\nLondon\n\n## Summary\n\nSenior engineer.\n\n## Experience",
                )),
            }),
            RoleLineShape => Some(&Advice {
                summary: "This `###` heading is not a role line.",
                why: "Project convention: `###` is reserved for roles, written `Company · Title · Period` or `Company · Period`. The book asks for experience marked clearly with dates.",
                fix: "Write `### Company · Title · 2019 – 2023`; use `## ` for a section or `**Name · Period**` for an era.",
                example: Some((
                    "### Staff Engineer at Acme, 2019-2023",
                    "### Acme · Staff Engineer · 2019 – 2023",
                )),
            }),
            RoleSeparator => Some(&Advice {
                summary: "Role parts are separated by something other than ` · `.",
                why: "Project convention: the separator is a middle dot (U+00B7) with a space on each side. The previous tool silently rendered other separators as a plain heading.",
                fix: "Type ` · ` (Option+Shift+9 on a Mac) between the company, the title, and the dates.",
                example: Some(("### Acme | Engineer | 2015 – 2017", "### Acme · Engineer · 2015 – 2017")),
            }),
            EmptyRolePart => Some(&Advice {
                summary: "A part of the role line is empty.",
                why: "Project convention: every part of `Company · Title · Period` needs text; a title may be left out only together with its separator.",
                fix: "Fill in the empty part, or remove it together with its separator.",
                example: Some(("### Acme ·  · 2019 – 2023", "### Acme · Staff Engineer · 2019 – 2023")),
            }),
            EraShape => Some(&Advice {
                summary: "The bold line looks like an era but is not `**Name · Period**`.",
                why: "Project convention: an era is a paragraph that is only bold text: a name, ` · `, and dates. It shows progression inside one employer.",
                fix: "Make the whole paragraph bold with a name, ` · `, and dates; move any other text into its own paragraph.",
                example: Some(("**AI platforms** · 2026 – Present", "**AI platforms · 2026 – Present**")),
            }),
            EraOutsideRole => Some(&Advice {
                summary: "An era line is outside a role.",
                why: "Project convention: eras show progression inside one employer, so they belong under a `###` role line.",
                fix: "Move the era under the role it belongs to, or make it plain text.",
                example: Some((
                    "## Experience\n\n**AI platforms · 2026 – Present**",
                    "## Experience\n\n### Acme · 2026 – Present\n\n**AI platforms · 2026 – Present**",
                )),
            }),
            PeriodUnparseable => Some(&Advice {
                summary: "The dates can't be read.",
                why: "The Tech Resume Inside Out asks for consistent, easy-to-read dates. The parser accepts a small closed set of forms so every style renders them the same way.",
                fix: "Write `Aug 2017 – Present`, `2015 – 2017`, or `Fall 2019 – Spring 2021`, with years from 1950 to 2100.",
                example: Some(("### Acme · 08/2017 until now", "### Acme · Aug 2017 – Present")),
            }),
            PeriodEndBeforeStart => Some(&Advice {
                summary: "The period ends before it starts.",
                why: "Project convention: a period runs from its start to its end; a reversed range is almost always a typo.",
                fix: "Swap the dates, or correct the year.",
                example: Some(("2020 – 2018", "2018 – 2020")),
            }),
            PeriodHyphen => Some(&Advice {
                summary: "Use an en dash with a space on each side between dates.",
                why: "Project convention: one separator everywhere keeps dates consistent, which the book asks for, and lets styles align them.",
                fix: "Replace the separator with ` – ` (an en dash; Option+hyphen on a Mac).",
                example: Some(("2015 - 2017", "2015 – 2017")),
            }),
            PeriodPresentSpelling => Some(&Advice {
                summary: "Write `Present` for an ongoing role.",
                why: "Project convention: a single spelling keeps dates consistent, which the book asks for.",
                fix: "Replace `Current`, `Now`, or another capitalization with `Present`.",
                example: Some(("Aug 2017 – Current", "Aug 2017 – Present")),
            }),
            UnsupportedHtml => Some(&Advice {
                summary: "Raw HTML can't be typeset.",
                why: "Project convention: only comments and the page-break marker are allowed; other HTML would be dropped or break the layout.",
                fix: "Use Markdown. A comment is `<!-- ... -->`; a page break is `<div class=\"page-break\"></div>` alone on its lines, with blank lines around it.",
                example: Some(("Seattle<br>WA", "Seattle, WA")),
            }),
            InvalidLink => Some(&Advice {
                summary: "The link target is not a valid address.",
                why: "The Tech Resume Inside Out says links should be clickable, and a link needs a full address to be.",
                fix: "Write a full address such as `https://example.com`, with descriptive link text.",
                example: Some(("[Notes](notes.md)", "[Notes](https://example.com/notes)")),
            }),
            UnsupportedLink => Some(&Advice {
                summary: "Only http, https, and mailto links are supported.",
                why: "Project convention: links are limited to schemes that every PDF viewer and recruiter can open.",
                fix: "Use an `https://` or `mailto:` link.",
                example: Some((
                    "[Resume](ftp://example.com/resume.pdf)",
                    "[Resume](https://example.com/resume.pdf)",
                )),
            }),
            NestedList => Some(&Advice {
                summary: "Nested bullets are flattened to one level.",
                why: "The Tech Resume Inside Out: sub-bullets clutter a resume and make it harder to read; stick with one level.",
                fix: "Fold the detail into the parent bullet, or make it a sibling bullet.",
                example: Some((
                    "- Led the platform\n  - cut cost 40%",
                    "- Led the platform, cutting cost 40%",
                )),
            }),
            RoleOrder => Some(&Advice {
                summary: "Roles are not listed newest first.",
                why: "The Tech Resume Inside Out: list experience in reverse chronological order, with the latest on top.",
                fix: "Move the later role above the earlier one.",
                example: Some((
                    "### Old Co · 2010 – 2012\n\n### New Co · 2018 – 2020",
                    "### New Co · 2018 – 2020\n\n### Old Co · 2010 – 2012",
                )),
            }),
            MidSentenceBold => Some(&Advice {
                summary: "Bold appears inside a sentence.",
                why: "The Tech Resume Inside Out: don't bold anything inside a sentence; bold only a job title, company name, date, or header.",
                fix: "Remove the bold, or write a lead-in as `**Label:** text` with the colon inside the bold.",
                example: Some((
                    "Cut cost by **40%** with autoscaling",
                    "Cut cost by 40% with autoscaling",
                )),
            }),
            Image => Some(&Advice {
                summary: "Images are not rendered.",
                why: "The Tech Resume Inside Out: don't include photos on a resume.",
                fix: "Remove the image.",
                example: Some(("![Me](me.png)", "")),
            }),
            TooManyContactItems => Some(&Advice {
                summary: "The contact line has more than four items.",
                why: "The Tech Resume Inside Out: don't share more than four contact details or social profiles.",
                fix: "Keep location, email, phone, and one or two links; drop the rest.",
                example: Some((
                    "Seattle, WA · 206-555-0100 · ada@example.com · [LinkedIn](https://l.in/a) · [GitHub](https://g.h/a)",
                    "Seattle, WA · ada@example.com · [LinkedIn](https://l.in/a) · [GitHub](https://g.h/a)",
                )),
            }),
            MissingContact => Some(&Advice {
                summary: "The name has no contact line.",
                why: "The Tech Resume Inside Out: put basic contact details at the top, and keep them short.",
                fix: "Add a line right after the name with location, email, phone, and links, separated by ` · `.",
                example: Some(("# Ada\n\n## Summary", "# Ada\n\nLondon · ada@example.com\n\n## Summary")),
            }),
            UnsupportedMarkdown => Some(&Advice {
                summary: "This Markdown construct can't be represented.",
                why: "Project convention: a resume is a name, a contact line, sections, roles with bullets, and eras. Quotes, tables, code blocks, numbered lists, and headings below level 3 have no place in the model and would be lost.",
                fix: "Rewrite it as a paragraph, a `- ` bullet, a `**Label:**` lead-in, or an era line.",
                example: Some(("1. Led the team", "- Led the team")),
            }),
            HardBreak => Some(&Advice {
                summary: "A hard line break is rendered as a space.",
                why: "Project convention: paragraphs and bullets flow as one block; trailing spaces or a backslash at a line end are usually accidental.",
                fix: "Remove the trailing spaces or backslash (delete the whole line if it holds only the marker); use a blank line to start a new paragraph.",
                example: Some(("Led the team  \nand shipped", "Led the team\nand shipped")),
            }),
            TextTooLong => Some(&Advice {
                summary: "A heading or role part is too long.",
                why: "Project convention: names and headings are short. The limits are 80 characters for a name or era, 120 for a company or title, and 60 for a section.",
                fix: "Shorten it; move the detail into the bullets.",
                example: None,
            }),
            EmptyHeading => Some(&Advice {
                summary: "A heading has no text.",
                why: "Project convention: the name, every section, and every role line need text.",
                fix: "Write the heading text, or remove the heading.",
                example: Some(("##", "## Experience")),
            }),
            HeadingMarkerSpacing => Some(&Advice {
                summary: "A line starts with `#` but has no space after it, so it is text, not a heading.",
                why: "Project convention: Markdown needs a space after the `#` markers (`### Acme`); without one the hashes are printed and the line does not start a role or section.",
                fix: "Add a space after the `#` markers, or remove them if the line is meant to be text.",
                example: Some((
                    "###Acme · Staff Engineer · 2019 – 2023",
                    "### Acme · Staff Engineer · 2019 – 2023",
                )),
            }),
            InputLimit => Some(&Advice {
                summary: "The input exceeds a size or nesting limit.",
                why: "Project convention: limits protect the tool from pathological files: 1 MiB of Markdown, 8 levels of nested emphasis or lists, and 500 findings.",
                fix: "Split the file, flatten the nesting, or fix the earlier findings first.",
                example: None,
            }),
            FontFallbackMetric => Some(&Advice {
                summary: "A bundled font stands in for the template's font; layout is unchanged.",
                why: "Project convention: each style names the template's own font and a free bundled substitute with the same character widths (Calibri and Carlito, Georgia and Gelasio), so line breaks and the page count match and only glyph shapes differ.",
                fix: "No action is needed. To get the original glyph shapes, install the font or pass `--font-path <DIR>` with its font files. `--fonts bundled` always uses the substitute.",
                example: Some(("resume build resume.md", "resume build resume.md --font-path ~/Fonts")),
            }),
            FontFallbackLayout => Some(&Advice {
                summary: "A bundled font with different character widths stands in; line breaks may change.",
                why: "Project convention: the substitute for this role is not metric-compatible (TeX Gyre Pagella for Book Antiqua), so text can wrap differently and the page count can change.",
                fix: "Install the preferred font or pass `--font-path <DIR>` with its font files, rebuild, and check the page count and the last page.",
                example: Some(("resume build resume.md", "resume build resume.md --font-path ~/Fonts")),
            }),
            FontMissing => Some(&Advice {
                summary: "Neither the preferred font nor its bundled fallback is available.",
                why: "Project convention: every role in a style names a preferred font and a bundled fallback, so a build never lacks a font; this happens only when a style's font list was edited.",
                fix: "Name a bundled family as the fallback in the style's `theme.typ`, install the font, or pass `--font-path <DIR>`.",
                example: Some((
                    r#"body: (preferred: "Calibri", fallback: "Calbri")"#,
                    r#"body: (preferred: "Calibri", fallback: "Carlito")"#,
                )),
            }),
            PageLimitExceeded => Some(&Advice {
                summary: "The resume runs past the page limit given with `--max-pages`.",
                why: "Project convention: `--max-pages` states how many pages the output may have, and the count is taken from the laid-out document, after fonts and line breaks are final, so a build that needs more pages fails.",
                fix: "Cut the oldest or weakest bullets, tighten the long ones, or drop a role's detail until the page count fits, then rebuild. If no limit applies, leave `--max-pages` off.",
                example: None,
            }),
            HeadingNearPageBottom => Some(&Advice {
                summary: "A role or era heading sits at the foot of a page with almost nothing under it.",
                why: "Project convention: a heading with only a line or two beneath it at the bottom of a page reads as stranded. The shipped styles keep a heading with the content that follows, so this guards against an edited style.",
                fix: "Add a page break before the heading (`<div class=\"page-break\"></div>` on its own lines), or tighten the text above it so the role moves up. If you edited the style, restore its keep-with-next behavior for headings.",
                example: None,
            }),
            SparseLastPage => Some(&Advice {
                summary: "The last page is less than 15% full.",
                why: "Project convention: a last page holding only a few lines looks unfinished and costs a page. The fill is measured against the page body, so it does not depend on the margins.",
                fix: "Trim or tighten about the last page's worth of text, such as a long bullet or an old role's detail, to save the page. If the page count is fine, there is nothing to change.",
                example: None,
            }),
            ContentOverflowsPage => Some(&Advice {
                summary: "A block of content is taller than a page and runs off it.",
                why: "Project convention: a bullet cannot be split across pages, so one taller than the page body runs past its bottom, and Typst gives no warning. The overflowing text is cut from the PDF.",
                fix: "Split the bullet into several shorter bullets, or shorten it until it fits on one page, then rebuild.",
                example: None,
            }),
            // Content (doctor) and Setup: written by the tasks that own them.
            LongBullet | UnquantifiedBullet | WeakOpener | FirstPerson | SloppyPhrase | LongSummary
            | EducationNotLast | OldRoleDetail | UnknownStyle | StyleMissingMain | OutputNotWritable | TypstError
            | TypstWarning => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Category;

    const BOOK: &str = "The Tech Resume Inside Out";
    use claims::assert_some;
    use strum::IntoEnumIterator;

    #[test]
    fn every_format_code_has_advice() {
        for code in DiagnosticCode::iter().filter(|c| c.category() == Category::Format) {
            assert_some!(code.advice(), "{code} has no advice");
        }
    }

    #[test]
    fn the_font_codes_have_advice_that_says_how_to_get_the_font() {
        for code in [
            DiagnosticCode::FontFallbackMetric,
            DiagnosticCode::FontFallbackLayout,
            DiagnosticCode::FontMissing,
        ] {
            let advice = assert_some!(code.advice(), "{code} has advice");
            assert!(
                advice.fix.contains("--font-path") || advice.fix.contains("theme.typ"),
                "{code}: {}",
                advice.fix
            );
        }
    }

    #[test]
    fn advice_entries_are_well_formed() {
        for code in DiagnosticCode::iter() {
            let Some(advice) = code.advice() else { continue };
            assert!(
                !advice.summary.is_empty() && !advice.summary.contains('\n'),
                "{code}: summary is one line"
            );
            assert!(
                advice.summary.chars().count() <= 100,
                "{code}: summary is at most 100 characters"
            );
            assert!(advice.summary.ends_with('.'), "{code}: summary is a sentence");
            assert!(
                !advice.why.is_empty() && !advice.fix.is_empty(),
                "{code}: why and fix are non-empty"
            );
            if let Some((before, after)) = advice.example {
                assert_ne!(before, after, "{code}: the example must change something");
            }
        }
    }

    #[test]
    fn why_cites_the_book_or_names_a_project_convention() {
        for code in DiagnosticCode::iter() {
            let Some(advice) = code.advice() else { continue };
            let sourced = advice.why.starts_with(BOOK) || advice.why.starts_with("Project convention:");
            assert!(
                sourced,
                "{code}: rationale must cite the book or say it is a project convention: {}",
                advice.why
            );
        }
    }

    #[test]
    fn every_layout_code_has_advice() {
        for code in DiagnosticCode::iter().filter(|c| c.category() == Category::Layout) {
            assert_some!(code.advice(), "{code} has no advice");
        }
    }

    #[test]
    fn the_codes_that_report_a_page_problem_say_how_to_change_the_resume() {
        for code in [
            DiagnosticCode::PageLimitExceeded,
            DiagnosticCode::HeadingNearPageBottom,
            DiagnosticCode::SparseLastPage,
            DiagnosticCode::ContentOverflowsPage,
        ] {
            let advice = assert_some!(code.advice(), "{code} has no advice");
            assert!(
                advice.fix.len() > 30,
                "{code}: the fix says what to change: {}",
                advice.fix
            );
        }
    }

    #[test]
    fn codes_without_advice_are_content_or_setup_codes() {
        // The tasks that write Content and Setup advice flip this to "every code has advice".
        for code in DiagnosticCode::iter().filter(|c| c.advice().is_none()) {
            assert!(
                matches!(code.category(), Category::Content | Category::Setup),
                "{code} ({}) has no advice",
                code.category()
            );
        }
    }
}
