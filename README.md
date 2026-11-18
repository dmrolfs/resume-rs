<!-- ABOUTME: Overview of resume-rs, a CLI that typesets Markdown resumes into print-quality PDFs. -->
<!-- ABOUTME: Stub for now; the full guide (writing, styling, commands) lands in task rsm-141.13. -->

# resume-rs

`resume` typesets resumes written in Markdown into print-quality PDFs using embedded
[Typst](https://typst.app). It ships styles modeled on the templates that
*The Tech Resume Inside Out* and Standard Resume recommend for experienced engineers and leaders.

Status: under development. See `ref/Resume-RS-Architecture.md` for the design.

## Development

```bash
just check   # cargo fmt --check, cargo clippy --all-targets -D warnings, cargo test
```

Requires Rust 1.92 or later.
