<!-- ABOUTME: Documents the implemented strict cover-letter parser and approved placeholder policies. -->
<!-- ABOUTME: Covers rsm-5q1.2 only; selection, CLI and output remain later tasks. -->
# Cover-letter Markdown parser

Implementation task: rsm-5q1.2.
This record supersedes verbatim-only placeholder recommendations in the raw Oracle appendices.
The owner approved encoded placeholder support, literal field substitution, actual-comment semantics and errors for escaped/split/unknown placeholder attempts.

## Public API

`resume::markdown::parse_letter(path, text) -> ParsedLetter` performs no file I/O.
`valid()` and `into_parts()` expose a `LetterSource` only when there are no error diagnostics.
Findings contain original-source byte ranges and use the existing bounded diagnostic sink.
The input cap and nesting limits match the resume parser.

```markdown
# Alex Example

## Greeting

Dear {{recipient}},

## Body

I am applying for **{{title}}** at &#123;&#123;company&#125;&#125;.

## Closing

Sincerely,
```

An optional contact paragraph can follow the name.
Exactly Greeting, Body and Closing level-two headings are required, in order, with one greeting/closing paragraph and a nonempty body.
The name supplies the eventual signature.
A visually empty frame such as emphasis around a removed comment is rejected.
Lists, tables, quotes, images, raw HTML, nested headings and code blocks are unsupported rather than silently lost.
Inline code, emphasis and validated links are supported; soft/hard line breaks fold to spaces without resume-specific fix edits.

## Placeholder policy

The closed field vocabulary is company, title, recipient.
Numeric, named and mixed HTML character references may spell any placeholder characters as decoded by CommonMark.
Source maps still point at the original encoded spelling.
Backslash-escaped attempts error rather than interpolating unexpectedly.
A placeholder cannot cross formatting, comment or line-break boundaries, including an opening brace pair assembled by removing a comment.
Single braces and an unmatched closing brace pair remain literal.
Names/contact, code spans, URL destinations and link/reference titles cannot contain placeholder attempts.
Unused reference definitions are checked as well as active links.
Missing field values are a separate resolution step and apply only to selected content.
Substituted values are never parsed, decoded or recursively expanded.

## Annotation safety

Only actual Markdown HTML comments are classified as annotations; code/escaped/entity-written comment lookalikes remain visible text.
A comment's first token starting with @, # or = is reserved and errors until letter tailoring ships.
Ordinary comments disappear, but malformed unescaped comments error instead of accidentally printing hidden text.
A bounded second pulldown event pass observes HTML inside unsupported containers before the regular tree discards their events, and checks reference definitions.
This leaves resume tree behavior unchanged; the cost is a second linear parse within the existing 1 MiB limit.
Comment text uses the existing TextNode mapping for container prefixes and Unicode offsets.
The whole-comment `annotation::is_annotation` signature matches the concurrently developed rsm-141.22.1 helper; retain the upstream full module when merging that work.

## Verification

Synthetic valid/invalid fixtures have model/diagnostic snapshots.
Public tests cover exact entity source spans for LF/CRLF/lone CR, BOM, unsafe placements, hidden comment boundaries, quoted/list multiline annotations, error-gated into_parts, actual-comment semantics and both hard-break forms.
Structured randomized valid letter frames test that exposed models never contain unrecognized opening placeholder markers; arbitrary-input properties test diagnostic ranges and no panic.
Independent code and test reviews found and drove fixes for container-stripped comment offsets, resume contact policy leakage, split-brace assembly and multiple escaped/real openers.
Targeted mutation testing and gate results are recorded in the task's Beads notes.
No letter command, tailoring or text/PDF renderer is supplied by this parser task.
