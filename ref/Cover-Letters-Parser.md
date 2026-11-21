<!-- ABOUTME: Documents the implemented strict cover-letter parser and approved placeholder policies. -->
<!-- ABOUTME: Covers rsm-5q1.2 and the annotation reading of rsm-vqr.1; selection, CLI and output remain later tasks. -->
# Cover-letter Markdown parser

## Annotations (rsm-vqr.1, supersedes the reservation described under Annotation safety)

A letter now reads annotations with the resume's grammar and group checker (`src/markdown/annotation.rs`); `letter-annotation-unsupported` no longer exists.
A paragraph in Greeting, Body or Closing takes an annotation as a trailing comment or as a comment alone on the line above it.
`annotation::leading`, `trailing` and `combine` report the shared codes (`annotation-detached`, `annotation-without-target`, `annotation-duplicate`, `annotation-unknown-keyword`, `annotation-malformed`, `annotation-conflict`).
The name, contact line, headings, unsupported blocks, and a comment in the middle of a paragraph or inside bold, emphasis or a link are `annotation-misplaced`.
Greeting and closing take `=group.variant` only; priority, status or tags there are `annotation-misplaced`.
`LetterSource` keeps every authored wording: each section item is a `SourceItem`, a plain `Paragraph(Item)` or an `Alternates(AlternateGroup)` of one checked group (a lone `default` is still a group).
An `Item` holds the paragraph's `Piece` with its original spans, its `Annotations`, and the annotation comment's own range.
`annotation::group` runs once for the whole letter with one container per section, so a group never crosses a section and a group name is unique in the document.
The parser chooses nothing: drafts and every variant stay in the source, and selection (rsm-vqr.2) decides.
`TryFrom<LetterSource> for SelectedLetter` replaces the old `From`: it accepts only a letter with no annotations at all and otherwise returns `SelectionRequired` naming the first comment, so a draft or an unselected wording cannot reach field resolution.
Ownership: a comment the interpreter judged (through `leading`, `trailing` or `reject_in`) is recorded by its start offset, and `letter_safety::inspect` then reports only annotation-shaped comments nobody judged, which sit inside unsupported containers or were never closed.

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
A comment's first token starting with @, # or = is an annotation (`annotation::is_annotation`); see Annotations above for where it is read and where it is an error. Until rsm-vqr.1 it was reserved and always an error.
Ordinary comments disappear, but malformed unescaped comments error instead of accidentally printing hidden text.
A bounded second pulldown event pass observes HTML inside unsupported containers before the regular tree discards their events, and checks reference definitions.
This leaves resume tree behavior unchanged; the cost is a second linear parse within the existing 1 MiB limit.
Comment text uses the existing TextNode mapping for container prefixes and Unicode offsets.
The whole-comment `annotation::is_annotation` is the upstream module from rsm-141.22.1, which replaced the stub this task first shipped.

## Verification

Synthetic valid/invalid fixtures have model/diagnostic snapshots.
Public tests cover exact entity source spans for LF/CRLF/lone CR, BOM, unsafe placements, hidden comment boundaries, quoted/list multiline annotations, error-gated into_parts, actual-comment semantics and both hard-break forms.
Structured randomized valid letter frames test that exposed models never contain unrecognized opening placeholder markers; arbitrary-input properties test diagnostic ranges and no panic.
Independent code and test reviews found and drove fixes for container-stripped comment offsets, resume contact policy leakage, split-brace assembly and multiple escaped/real openers.
Targeted mutation testing and gate results are recorded in the task's Beads notes.
No letter command, tailoring or text/PDF renderer is supplied by this parser task.
