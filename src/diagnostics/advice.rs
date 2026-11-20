//! ABOUTME: The advice catalog: one summary, rationale, fix, and example per diagnostic code.
//! ABOUTME: Every code has an entry; a test fails when one is missing or badly formed.

use super::code::{Advice, DiagnosticCode};

impl DiagnosticCode {
    /// The advice for this code.
    ///
    /// The match is exhaustive with no wildcard, so adding a code forces its advice to be written here.
    #[must_use]
    pub const fn advice(self) -> &'static Advice {
        use DiagnosticCode::*;
        match self {
            MissingName => &Advice {
                summary: "The resume has no `# Name` heading.",
                why: "Project convention: the first-level heading is the name; styles use it as the title and the footer text.",
                fix: "Start the file with `# Your Name`; comments may come before it.",
                example: Some((
                    "## Summary\n\nSenior engineer.",
                    "# Ada Lovelace\n\nLondon · ada@example.com\n\n## Summary\n\nSenior engineer.",
                )),
            },
            DuplicateName => &Advice {
                summary: "A resume has exactly one `# ` heading.",
                why: "Project convention: the single `# ` heading is the name; sections start at `## `.",
                fix: "Change the extra `# ` heading to `## `, or remove it.",
                example: Some(("# Experience", "## Experience")),
            },
            ContentBeforeSection => &Advice {
                summary: "Content appears before the first `## ` section.",
                why: "Project convention: only the name and one contact line come before the first section.",
                fix: "Move the content under a `## ` section.",
                example: Some((
                    "# Ada\n\nLondon\n\nSenior engineer.\n\n## Experience",
                    "# Ada\n\nLondon\n\n## Summary\n\nSenior engineer.\n\n## Experience",
                )),
            },
            RoleLineShape => &Advice {
                summary: "This `###` heading is not a role line.",
                why: "Project convention: `###` is reserved for roles, written `Company · Title · Period` or `Company · Period`. The book asks for experience marked clearly with dates.",
                fix: "Write `### Company · Title · 2019 – 2023`; use `## ` for a section or `**Name · Period**` for an era.",
                example: Some((
                    "### Staff Engineer at Acme, 2019-2023",
                    "### Acme · Staff Engineer · 2019 – 2023",
                )),
            },
            RoleSeparator => &Advice {
                summary: "Role parts are separated by something other than ` · `.",
                why: "Project convention: the separator is a middle dot (U+00B7) with a space on each side. The previous tool silently rendered other separators as a plain heading.",
                fix: "Type ` · ` (Option+Shift+9 on a Mac) between the company, the title, and the dates.",
                example: Some(("### Acme | Engineer | 2015 – 2017", "### Acme · Engineer · 2015 – 2017")),
            },
            EmptyRolePart => &Advice {
                summary: "A part of the role line is empty.",
                why: "Project convention: every part of `Company · Title · Period` needs text; a title may be left out only together with its separator.",
                fix: "Fill in the empty part, or remove it together with its separator.",
                example: Some(("### Acme ·  · 2019 – 2023", "### Acme · Staff Engineer · 2019 – 2023")),
            },
            EraShape => &Advice {
                summary: "The bold line looks like an era but is not `**Name · Period**`.",
                why: "Project convention: an era is a paragraph that is only bold text: a name, ` · `, and dates. It shows progression inside one employer.",
                fix: "Make the whole paragraph bold with a name, ` · `, and dates; move any other text into its own paragraph.",
                example: Some((
                    "**AI platforms · 2026 – Present** Led the platform team.",
                    "**AI platforms · 2026 – Present**\n\nLed the platform team.",
                )),
            },
            EraOutsideRole => &Advice {
                summary: "An era line is outside a role.",
                why: "Project convention: eras show progression inside one employer, so they belong under a `###` role line.",
                fix: "Move the era under the role it belongs to, or make it plain text.",
                example: Some((
                    "## Experience\n\n**AI platforms · 2026 – Present**",
                    "## Experience\n\n### Acme · 2026 – Present\n\n**AI platforms · 2026 – Present**",
                )),
            },
            PeriodUnparseable => &Advice {
                summary: "The dates can't be read.",
                why: "The Tech Resume Inside Out asks for consistent, easy-to-read dates. The parser accepts a small closed set of forms so every style renders them the same way.",
                fix: "Write `Aug 2017 – Present`, `2015 – 2017`, or `Fall 2019 – Spring 2021`, with years from 1950 to 2100.",
                example: Some(("### Acme · 08/2017 until now", "### Acme · Aug 2017 – Present")),
            },
            PeriodEndBeforeStart => &Advice {
                summary: "The period ends before it starts.",
                why: "Project convention: a period runs from its start to its end; a reversed range is almost always a typo.",
                fix: "Swap the dates, or correct the year.",
                example: Some(("### Acme · Engineer · 2020 – 2018", "### Acme · Engineer · 2018 – 2020")),
            },
            PeriodHyphen => &Advice {
                summary: "Use an en dash with a space on each side between dates.",
                why: "Project convention: one separator everywhere keeps dates consistent, which the book asks for, and lets styles align them.",
                fix: "Replace the separator with ` – ` (an en dash; Option+hyphen on a Mac).",
                example: Some(("### Acme · Engineer · 2015 - 2017", "### Acme · Engineer · 2015 – 2017")),
            },
            PeriodPresentSpelling => &Advice {
                summary: "Write `Present` for an ongoing role.",
                why: "Project convention: a single spelling keeps dates consistent, which the book asks for.",
                fix: "Replace `Current`, `Now`, or another capitalization with `Present`.",
                example: Some((
                    "### Acme · Engineer · Aug 2017 – Current",
                    "### Acme · Engineer · Aug 2017 – Present",
                )),
            },
            UnsupportedHtml => &Advice {
                summary: "Raw HTML can't be typeset.",
                why: "Project convention: only comments and the page-break marker are allowed; other HTML would be dropped or break the layout.",
                fix: "Use Markdown. A comment is `<!-- ... -->`; a page break is `<div class=\"page-break\"></div>` alone on its lines, with blank lines around it.",
                example: Some(("Seattle<br>WA", "Seattle, WA")),
            },
            InvalidLink => &Advice {
                summary: "The link target is not a valid address.",
                why: "The Tech Resume Inside Out says links should be clickable, and a link needs a full address to be.",
                fix: "Write a full address such as `https://example.com`, with descriptive link text.",
                example: Some(("[Notes](notes.md)", "[Notes](https://example.com/notes)")),
            },
            UnsupportedLink => &Advice {
                summary: "Only http, https, and mailto links are supported.",
                why: "Project convention: links are limited to schemes that every PDF viewer and recruiter can open.",
                fix: "Use an `https://` or `mailto:` link.",
                example: Some((
                    "[Resume](ftp://example.com/resume.pdf)",
                    "[Resume](https://example.com/resume.pdf)",
                )),
            },
            NestedList => &Advice {
                summary: "Nested bullets are flattened to one level.",
                why: "The Tech Resume Inside Out: sub-bullets clutter a resume and make it harder to read; stick with one level.",
                fix: "Fold the detail into the parent bullet, or make it a sibling bullet.",
                example: Some((
                    "- Led the platform\n  - cut cost 40%",
                    "- Led the platform, cutting cost 40%",
                )),
            },
            RoleOrder => &Advice {
                summary: "Roles are not listed newest first.",
                why: "The Tech Resume Inside Out: list experience in reverse chronological order, with the latest on top.",
                fix: "Move the later role above the earlier one.",
                example: Some((
                    "### Old Co · 2010 – 2012\n\n### New Co · 2018 – 2020",
                    "### New Co · 2018 – 2020\n\n### Old Co · 2010 – 2012",
                )),
            },
            MidSentenceBold => &Advice {
                summary: "Bold appears inside a sentence.",
                why: "The Tech Resume Inside Out: don't bold anything inside a sentence; bold only a job title, company name, date, or header.",
                fix: "Remove the bold, or write a lead-in as `**Label:** text` with the colon inside the bold.",
                example: Some((
                    "Cut cost by **40%** with autoscaling",
                    "Cut cost by 40% with autoscaling",
                )),
            },
            Image => &Advice {
                summary: "Images are not rendered.",
                why: "The Tech Resume Inside Out: don't include photos on a resume.",
                fix: "Remove the image.",
                example: Some(("![Me](me.png)", "")),
            },
            TooManyContactItems => &Advice {
                summary: "The contact line has more than four items.",
                why: "The Tech Resume Inside Out: don't share more than four contact details or social profiles.",
                fix: "Keep location, email, phone, and one or two links; drop the rest.",
                example: Some((
                    "# Ada Lovelace\n\nSeattle, WA · 206-555-0100 · ada@example.com · [LinkedIn](https://l.in/a) · [GitHub](https://g.h/a)",
                    "# Ada Lovelace\n\nSeattle, WA · ada@example.com · [LinkedIn](https://l.in/a) · [GitHub](https://g.h/a)",
                )),
            },
            MissingContact => &Advice {
                summary: "The name has no contact line.",
                why: "The Tech Resume Inside Out: put basic contact details at the top, and keep them short.",
                fix: "Add a line right after the name with location, email, phone, and links, separated by ` · `.",
                example: Some(("# Ada\n\n## Summary", "# Ada\n\nLondon · ada@example.com\n\n## Summary")),
            },
            UnsupportedMarkdown => &Advice {
                summary: "This Markdown construct can't be represented.",
                why: "Project convention: a resume is a name, a contact line, sections, roles with bullets, and eras. Quotes, tables, code blocks, numbered lists, and headings below level 3 have no place in the model and would be lost.",
                fix: "Rewrite it as a paragraph, a `- ` bullet, a `**Label:**` lead-in, or an era line.",
                example: Some(("1. Led the team", "- Led the team")),
            },
            HardBreak => &Advice {
                summary: "A hard line break is rendered as a space.",
                why: "Project convention: paragraphs and bullets flow as one block; trailing spaces or a backslash at a line end are usually accidental.",
                fix: "Remove the trailing spaces or backslash (delete the whole line if it holds only the marker); use a blank line to start a new paragraph.",
                example: Some(("Led the team  \nand shipped", "Led the team\nand shipped")),
            },
            TextTooLong => &Advice {
                summary: "A heading or role part is too long.",
                why: "Project convention: names and headings are short. The limits are 80 characters for a name or era, 120 for a company or title, and 60 for a section.",
                fix: "Shorten it; move the detail into the bullets.",
                example: None,
            },
            EmptyHeading => &Advice {
                summary: "A heading has no text.",
                why: "Project convention: the name, every section, and every role line need text.",
                fix: "Write the heading text, or remove the heading.",
                example: Some(("##", "## Experience")),
            },
            HeadingMarkerSpacing => &Advice {
                summary: "A line starts with `#` but has no space after it, so it is text, not a heading.",
                why: "Project convention: Markdown needs a space after the `#` markers (`### Acme`); without one the hashes are printed and the line does not start a role or section.",
                fix: "Add a space after the `#` markers, or remove them if the line is meant to be text.",
                example: Some((
                    "###Acme · Staff Engineer · 2019 – 2023",
                    "### Acme · Staff Engineer · 2019 – 2023",
                )),
            },
            PlaceholderUnresolved => &Advice {
                summary: "A selected letter placeholder has no application field value.",
                why: "Project convention: every selected placeholder must resolve before text or PDF output.",
                fix: "Supply the named field, or select wording that does not use it.",
                example: None,
            },
            InputLimit => &Advice {
                summary: "The input exceeds a size or nesting limit.",
                why: "Project convention: limits protect the tool from pathological files: 1 MiB of Markdown, 8 levels of nested emphasis or lists, and 500 findings.",
                fix: "Split the file, flatten the nesting, or fix the earlier findings first.",
                example: None,
            },
            FontFallbackMetric => &Advice {
                summary: "A bundled font stands in for the template's font; layout is unchanged.",
                why: "Project convention: each style names the template's own font and a free bundled substitute with the same character widths (Calibri and Carlito, Georgia and Gelasio), so line breaks and the page count match and only glyph shapes differ.",
                fix: "No action is needed. To get the original glyph shapes, install the font or pass `--font-path <DIR>` with its font files. `--fonts bundled` always uses the substitute.",
                example: Some((
                    "resume check resume.md --fonts bundled --format json",
                    "resume check resume.md --font-path ~/Fonts",
                )),
            },
            FontFallbackLayout => &Advice {
                summary: "A bundled font with different character widths stands in; line breaks may change.",
                why: "Project convention: the substitute for this role is not metric-compatible (TeX Gyre Pagella for Book Antiqua), so text can wrap differently and the page count can change.",
                fix: "Install the preferred font or pass `--font-path <DIR>` with its font files, rebuild, and check the page count and the last page. Or name a metric-compatible fallback in the style's `theme.typ`.",
                example: Some((
                    r#"body: (preferred: "Calibri", fallback: "Arimo", metric-compatible: false)"#,
                    r#"body: (preferred: "Calibri", fallback: "Carlito", metric-compatible: true)"#,
                )),
            },
            FontMissing => &Advice {
                summary: "Neither the preferred font nor its bundled fallback is available.",
                why: "Project convention: every role in a style names a preferred font and a bundled fallback, so a build never lacks a font; this happens only when a style's font list was edited.",
                fix: "Name a bundled family as the fallback in the style's `theme.typ`, install the font, or pass `--font-path <DIR>`.",
                example: Some((
                    r#"body: (preferred: "Calibri", fallback: "Calbri", metric-compatible: true)"#,
                    r#"body: (preferred: "Calibri", fallback: "Carlito", metric-compatible: true)"#,
                )),
            },
            PageLimitExceeded => &Advice {
                summary: "The resume runs past the page limit given with `--max-pages`.",
                why: "Project convention: `--max-pages` states how many pages the output may have, and the count is taken from the laid-out document, after fonts and line breaks are final, so a build that needs more pages fails.",
                fix: "Cut the oldest or weakest bullets, tighten the long ones, or drop a role's detail until the page count fits, then rebuild. If no limit applies, leave `--max-pages` off.",
                example: None,
            },
            HeadingNearPageBottom => &Advice {
                summary: "A role or era heading sits at the foot of a page with almost nothing under it.",
                why: "Project convention: a heading with only a line or two beneath it at the bottom of a page reads as stranded. The shipped styles keep a heading with the content that follows, so this guards against an edited style.",
                fix: "Add a page break before the heading (`<div class=\"page-break\"></div>` on its own lines), or tighten the text above it so the role moves up. If you edited the style, restore its keep-with-next behavior for headings.",
                example: None,
            },
            SparseLastPage => &Advice {
                summary: "The last page is less than 15% full.",
                why: "Project convention: a last page holding only a few lines looks unfinished and costs a page. The fill is measured against the page body, so it does not depend on the margins.",
                fix: "Trim or tighten about the last page's worth of text, such as a long bullet or an old role's detail, to save the page. If the page count is fine, there is nothing to change.",
                example: None,
            },
            ContentOverflowsPage => &Advice {
                summary: "A block of content is taller than a page and runs off it.",
                why: "Project convention: a bullet or a role description cannot be split across pages, so one taller than the page body runs into the footer and past the page edge, and Typst gives no warning. The text past the page edge is cut from the PDF.",
                fix: "Split the block into several shorter ones, or shorten it until it fits on one page, then rebuild.",
                example: None,
            },
            LongBullet => &Advice {
                summary: "A bullet runs past about three lines.",
                why: "The Tech Resume Inside Out asks for bullets of two lines or fewer: a reader skims a resume, and a long bullet is skipped. The 320-character line is this project's reading of \"about three lines\".",
                fix: "Cut the bullet to its result, or split it into two bullets that each say one thing.",
                example: None,
            },
            UnquantifiedBullet => &Advice {
                summary: "A bullet in a role has no number.",
                why: "The Tech Resume Inside Out says to quantify impact with numbers, because most resumes do not contain them. Years, identifiers like `EC2`, and `Q4` are not counted as numbers by this project's check.",
                fix: "Add a measurable result: a percentage, a team size, a dollar amount, users, or time saved.",
                example: Some((
                    "- Improved the build system.",
                    "- Cut the build from 40 minutes to 9, saving the team about 6 hours a week.",
                )),
            },
            WeakOpener => &Advice {
                summary: "A bullet opens with a phrase that hides what you did.",
                why: "The Tech Resume Inside Out asks for active verbs that show your own contribution, not \"responsible for\" or \"helped\".",
                fix: "Start with what you did: Led, Built, Cut, Shipped, Designed.",
                example: Some((
                    "- Responsible for the build system.",
                    "- Rebuilt the build system and cut build time 80%.",
                )),
            },
            FirstPerson => &Advice {
                summary: "A bullet or summary speaks in the first person.",
                why: "The Tech Resume Inside Out says to drop \"I\" and to avoid \"we\": a resume is already about you, and \"we\" hides your share.",
                fix: "Remove the pronoun and start with the verb; say what you did, not what the team did.",
                example: Some((
                    "- I led the migration and we shipped in 6 weeks.",
                    "- Led the migration; shipped in 6 weeks.",
                )),
            },
            SloppyPhrase => &Advice {
                summary: "A phrase like \"etc.\" or \"various\" says nothing.",
                why: "The Tech Resume Inside Out lists sloppy phrases such as \"etc.\", \"and so on\", \"various\", and \"stuff\": they stand where a specific item should be.",
                fix: "Name the items, or cut the phrase.",
                example: Some(("- Languages: Rust, Java, etc.", "- Languages: Rust, Java, Go")),
            },
            LongSummary => &Advice {
                summary: "The summary is longer than about 120 words.",
                why: "The Tech Resume Inside Out says to keep the summary short and specific; a long one pushes the experience down the page. The 120-word line is this project's reading of \"short\".",
                fix: "Keep two or three sentences about what you do and what you have achieved; move detail into the roles.",
                example: None,
            },
            EducationNotLast => &Advice {
                summary: "Education comes before experience.",
                why: "The Tech Resume Inside Out says to move Education to the end once you have experience, so the top of the page shows your work. Sections like certifications or awards may follow it.",
                fix: "Move the `## Education` section below Experience and Skills.",
                example: None,
            },
            OldRoleDetail => &Advice {
                summary: "An old role has more bullets than your latest one.",
                why: "The Tech Resume Inside Out says older roles take proportionally less space. This project reads \"older\" as ended 15 or more years before the latest year in the resume, and never uses today's date.",
                fix: "Cut the old role to one or two bullets, or fold it into a short line.",
                example: None,
            },
            UnknownStyle => &Advice {
                summary: "No style of this name exists in --styles-dir or among the built-in styles.",
                why: "Project convention: `--style` looks in `--styles-dir` first and then among the built-in styles; a name that is in neither cannot be rendered.",
                fix: "Run `resume styles` to see the names, or fix the spelling. To make your own, run `resume export-style <NAME> <DIR>` and edit the copy.",
                example: Some((
                    "resume check resume.md --style monoo",
                    "resume check resume.md --style mono",
                )),
            },
            StyleMissingMain => &Advice {
                summary: "The style directory has no style.typ, so it cannot be rendered.",
                why: "Project convention: a style is a directory whose `style.typ` is the file Typst compiles; the other files only support it.",
                fix: "Add a `style.typ`, or start from a built-in style with `resume export-style <NAME> <DIR>`.",
                example: None,
            },
            StylesDirUnusable => &Advice {
                summary: "The --styles-dir cannot be read.",
                why: "Project convention: a styles directory must exist and be readable, and each style in it must stay inside it (a symlink out of the directory is refused).",
                fix: "Fix the path, or check its permissions. Remove --styles-dir to use only the built-in styles.",
                example: None,
            },
            FontPathUnusable => &Advice {
                summary: "A --font-path or RESUME_FONT_PATH entry cannot be read.",
                why: "Project convention: every directory named for fonts must exist and be readable, so a typing mistake cannot silently fall back to other fonts.",
                fix: "Fix the path, or remove it. `--fonts bundled` ignores installed fonts and needs no font path.",
                example: None,
            },
            OutputNotWritable => &Advice {
                summary: "The output directory cannot be created or written, so build would fail.",
                why: "Project convention: `build` writes the PDF beside nothing else, in the `--out` directory, creating it if it is missing; a file or a read-only path in the way stops it.",
                fix: "Choose another directory with `-o <DIR>`, or remove what is in the way.",
                example: Some(("resume check resume.md -o resume.md", "resume check resume.md -o out")),
            },
            TypstError => &Advice {
                summary: "The style's Typst code failed to compile.",
                why: "Project convention: styles are Typst files that read the resume as data; an error in one stops the render, and the message carries Typst's own explanation.",
                fix: "Read the excerpt and fix that line. If you edited a copy, compare it with `resume export-style` to see what changed.",
                example: None,
            },
            TypstWarning => &Advice {
                summary: "Typst reported a warning while rendering the style.",
                why: "Project convention: a warning does not stop the render, but it usually means the style asked for something Typst could not do, such as a font it does not have.",
                fix: "Read the excerpt; if the style is yours, fix that line. A built-in style should not warn.",
                example: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Category;

    const BOOK: &str = "The Tech Resume Inside Out";
    use strum::IntoEnumIterator;

    #[test]
    fn every_diagnostic_code_has_advice() {
        // `advice()` returns the entry itself, so a missing one is a compile error; this checks the entries
        // are not placeholders.
        for code in DiagnosticCode::iter() {
            let advice = code.advice();
            assert!(
                advice.summary.len() > 10 && advice.why.len() > 20 && advice.fix.len() > 10,
                "{code}"
            );
        }
    }

    #[test]
    fn the_font_codes_have_advice_that_says_how_to_get_the_font() {
        for code in [
            DiagnosticCode::FontFallbackMetric,
            DiagnosticCode::FontFallbackLayout,
            DiagnosticCode::FontMissing,
        ] {
            let advice = code.advice();
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
            let advice = code.advice();
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
            let advice = code.advice();
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
            let _ = code.advice();
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
            let advice = code.advice();
            assert!(
                advice.fix.len() > 30,
                "{code}: the fix says what to change: {}",
                advice.fix
            );
        }
    }

    #[test]
    fn page_advice_names_the_marker_the_parser_accepts_and_the_sparse_threshold_the_check_uses() {
        let advice = |code: DiagnosticCode| code.advice();
        assert!(
            advice(DiagnosticCode::HeadingNearPageBottom)
                .fix
                .contains(crate::markdown::PAGE_BREAK)
        );
        assert!(advice(DiagnosticCode::PageLimitExceeded).fix.contains("--max-pages"));
        let sparse = format!("{:.0}%", crate::layout::SPARSE_FILL * 100.0);
        assert!(advice(DiagnosticCode::SparseLastPage).summary.contains(&sparse));
    }
}
