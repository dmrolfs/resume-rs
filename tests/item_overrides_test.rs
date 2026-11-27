//! ABOUTME: `[select.override.<id>]` end to end: a spec changes one item's tags or priority for one application.
//! ABOUTME: Real files in a temp directory, run through `build --spec` and `letter --spec`, reading what a user reads.

use std::fs;
use std::path::{Path, PathBuf};

use claims::assert_ok;
use pretty_assertions::assert_eq;
use resume::commands::Outcome;

mod common;

#[path = "common/workspace.rs"]
mod workspace;
use workspace::{Run, Workspace};

const BASELINE: &str = "# Ada Lovelace\n\nLondon, UK · ada@example.com\n\n## Experience\n\n\
### Analytical Engines Ltd · Principal Engineer · 2017 – Present\n\nBuilds mechanical computers for customers.\n\n\
- Led delivery of the engine gateway to 5 customers in 3 months. <!-- &gateway @extra #ai -->\n\
- Cut a weekly run from $39K to $29K by caching 80% of the tables. <!-- &cache #data -->\n\
- Mentored 6 engineers through promotion in 2 years. <!-- &mentoring #leadership -->\n\
- Wrote the first published program in 1843.\n";

const LETTER: &str = "# Ada Lovelace\n\nLondon · ada@example.com\n\n## Greeting\n\nDear team,\n\n## Body\n\n\
I led the gateway work. <!-- &gateway @extra #ai -->\n\nI cut the weekly cost. <!-- &cache #data -->\n\n\
I mentored the team. <!-- &mentoring #leadership -->\n\n## Closing\n\nSincerely,\n";

struct Staged {
    spec: PathBuf,
    profiles: PathBuf,
}

/// A baseline, a letter, an empty profiles directory, and a spec: `head` (top-level keys such as `extends`), then
/// `[build]`, then `body`.
fn stage_with(work: &Workspace, head: &str, body: &str) -> Staged {
    let root = work.path();
    fs::create_dir_all(root.join("positions")).expect("positions");
    fs::create_dir_all(root.join("profiles")).expect("profiles");
    fs::write(root.join("baseline.md"), BASELINE).expect("baseline");
    fs::write(root.join("letter.md"), LETTER).expect("letter");
    let spec = root.join("positions/acme.toml");
    fs::write(&spec, format!("{head}[build]\ncontent = \"../baseline.md\"\n\n{body}")).expect("spec");
    Staged {
        spec,
        profiles: root.join("profiles"),
    }
}

fn stage(work: &Workspace, body: &str) -> Staged {
    stage_with(work, "", body)
}

fn build_spec(work: &Workspace, spec: &Path, extra: &[&str]) -> Run {
    let mut args: Vec<String> = ["build", "--spec"].map(String::from).into();
    args.push(spec.display().to_string());
    args.extend(["--fonts", "bundled", "--out"].map(String::from));
    args.push(work.out().display().to_string());
    args.extend(extra.iter().map(|a| (*a).to_string()));
    work.run(&args)
}

fn letter_spec(work: &Workspace, spec: &Path, extra: &[&str]) -> Run {
    let mut args: Vec<String> = ["letter", "--spec"].map(String::from).into();
    args.push(spec.display().to_string());
    args.extend(extra.iter().map(|a| (*a).to_string()));
    work.run(&args)
}

fn pdf_text(work: &Workspace) -> String {
    let pdf = assert_ok!(fs::read(work.out().join("acme.pdf")));
    common::page_texts(&pdf).join(" ")
}

#[test]
fn a_spec_retags_one_item_so_the_same_selection_keeps_it() {
    // `--tags data`-style selection comes from the spec itself: it selects `data`, and retags the gateway item.
    let without = Workspace::new();
    let staged = stage(&without, "[select]\ntags = [\"data\"]\n");
    assert_eq!(build_spec(&without, &staged.spec, &[]).outcome, Outcome::Success);
    assert!(
        !pdf_text(&without).contains("engine gateway"),
        "an @extra #ai item is not in a data selection"
    );

    let with = Workspace::new();
    let staged = stage(
        &with,
        "[select]\ntags = [\"data\"]\n\n[select.override.gateway]\ntags = [\"data\"]\n",
    );
    let run = build_spec(&with, &staged.spec, &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(
        pdf_text(&with).contains("engine gateway"),
        "the override made it a data item: {}",
        pdf_text(&with)
    );
}

#[test]
fn a_priority_override_makes_an_item_core_for_a_core_only_build() {
    let work = Workspace::new();
    let staged = stage(
        &work,
        "[select]\ncore-only = true\n\n[select.override.mentoring]\npriority = \"core\"\n",
    );
    let run = build_spec(&work, &staged.spec, &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let text = pdf_text(&work);
    assert!(text.contains("Mentored 6 engineers"), "{text}");
    assert!(!text.contains("weekly run"), "an untouched item is not core: {text}");
}

#[test]
fn an_override_in_a_profile_applies_and_the_spec_wins_a_field_it_writes() {
    let work = Workspace::new();
    let staged = stage_with(
        &work,
        "extends = [\"base\"]\n\n",
        "[select]\ncore-only = true\n\n[select.override.cache]\npriority = \"none\"\n",
    );
    fs::write(
        staged.profiles.join("base.toml"),
        "[select.override.cache]\npriority = \"core\"\n\n[select.override.mentoring]\npriority = \"core\"\n",
    )
    .expect("profile");
    let profiles = staged.profiles.display().to_string();
    let run = build_spec(&work, &staged.spec, &["--profiles", &profiles]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let text = pdf_text(&work);
    assert!(text.contains("Mentored 6 engineers"), "from the profile: {text}");
    assert!(
        !text.contains("weekly run"),
        "the spec's `none` beat the profile's `core`: {text}"
    );
}

#[test]
fn an_unknown_target_fails_the_build_naming_the_line_and_writes_no_pdf() {
    let work = Workspace::new();
    let staged = stage(&work, "[select.override.gatway]\ntags = [\"ai\"]\n");
    let run = build_spec(&work, &staged.spec, &[]);
    assert_eq!(run.outcome, Outcome::Failure, "{}", run.stderr);
    assert!(
        run.stderr.contains("acme.toml:5"),
        "the line of the override: {}",
        run.stderr
    );
    assert!(run.stderr.contains("`&gatway`"), "{}", run.stderr);
    assert!(
        !work.out().join("acme.pdf").exists(),
        "no PDF from a spec that names nothing"
    );
}

#[test]
fn an_unknown_target_is_found_even_when_the_item_around_it_would_be_dropped() {
    // The role is a draft, so no build keeps it; an override typo is still found.
    let work = Workspace::new();
    let baseline = format!("{BASELINE}\n### Beta · 2010 – 2014 <!-- @draft -->\n\n- Worked there. <!-- &beta -->\n");
    let staged = stage(&work, "[select.override.betaa]\npriority = \"core\"\n");
    fs::write(work.path().join("baseline.md"), baseline).expect("baseline");
    let run = build_spec(&work, &staged.spec, &[]);
    assert_eq!(run.outcome, Outcome::Failure, "{}", run.stderr);
    assert!(run.stderr.contains("`&betaa`"), "{}", run.stderr);
    // The real id inside the same draft role is a valid target even though selection drops it.
    let ok = Workspace::new();
    let staged = stage(&ok, "[select.override.beta]\npriority = \"core\"\n");
    fs::write(
        ok.path().join("baseline.md"),
        format!("{BASELINE}\n### Beta · 2010 – 2014 <!-- @draft -->\n\n- Worked there. <!-- &beta -->\n"),
    )
    .expect("baseline");
    assert_eq!(build_spec(&ok, &staged.spec, &[]).outcome, Outcome::Success);
    assert!(
        !pdf_text(&ok).contains("Worked there"),
        "a draft stays out after a core override"
    );
}

#[test]
fn a_spec_with_overrides_prints_them_without_opening_the_baseline() {
    let work = Workspace::new();
    let staged = stage(
        &work,
        "[select.override.gateway]\ntags = []\npriority = \"none\"\n\n[select.override.cache]\ntags = [\"ai\"]\n",
    );
    // The baseline is gone: printing a spec must not need it, and must not validate targets against it.
    fs::remove_file(work.path().join("baseline.md")).expect("remove");
    let run = work.run(&[
        "build".into(),
        "--spec".into(),
        staged.spec.display().to_string(),
        "--print-spec".into(),
    ]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(run.stdout.contains("[select.override.cache]"), "{}", run.stdout);
    assert!(
        run.stdout.contains("tags = []"),
        "an explicit clear is printed: {}",
        run.stdout
    );
    assert!(run.stdout.contains("priority = \"none\""), "{}", run.stdout);
}

#[test]
fn a_printed_spec_resolves_and_prints_to_the_same_text_again() {
    let work = Workspace::new();
    let staged = stage(
        &work,
        "[select.override.gateway]\ntags = []\npriority = \"none\"\n\n[select.override.cache]\ntags = [\"ai\"]\npriority = \"core\"\n",
    );
    let print = |spec: &Path| {
        let run = work.run(&[
            "build".into(),
            "--spec".into(),
            spec.display().to_string(),
            "--print-spec".into(),
        ]);
        assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
        run.stdout
    };
    let first = print(&staged.spec);
    let flat = work.path().join("positions/flat.toml");
    fs::write(&flat, &first).expect("flat");
    let second = print(&flat);
    let body = |text: &str| text.lines().skip(2).collect::<Vec<_>>().join("\n");
    assert_eq!(body(&first), body(&second), "two print-parse cycles give one body");
}

#[test]
fn a_spec_without_overrides_builds_the_same_pdf_as_before() {
    let a = Workspace::new();
    let b = Workspace::new();
    let sa = stage(&a, "[select]\ntags = [\"data\"]\n");
    let sb = stage(
        &b,
        "[select]\ntags = [\"data\"]\n\n[select.override.cache]\ntags = [\"data\"]\npriority = \"none\"\n",
    );
    assert_eq!(build_spec(&a, &sa.spec, &[]).outcome, Outcome::Success);
    assert_eq!(build_spec(&b, &sb.spec, &[]).outcome, Outcome::Success);
    // An override that restates what the item already is leaves the bytes alone (same spec name, same identity).
    let (pa, pb) = (
        assert_ok!(fs::read(a.out().join("acme.pdf"))),
        assert_ok!(fs::read(b.out().join("acme.pdf"))),
    );
    assert!(pa == pb, "a no-op override changes nothing in the PDF");
}

// ---- the letter --------------------------------------------------------------------------------------------

/// A `[letter]` table that prints text, so the tests read the letter from stdout without a PDF.
fn letter_body(content: &str) -> String {
    format!("[letter]\ncontent = \"../letter.md\"\noutput-format = \"text\"\n\n{content}")
}

#[test]
fn a_letter_override_retags_a_body_paragraph_in_its_own_table() {
    let work = Workspace::new();
    let staged = stage(
        &work,
        &letter_body("[letter.select]\ntags = [\"data\"]\n\n[letter.select.override.gateway]\ntags = [\"data\"]\n"),
    );
    let run = letter_spec(&work, &staged.spec, &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(run.stdout.contains("I led the gateway work."), "{}", run.stdout);
    let plain = Workspace::new();
    let staged = stage(&plain, &letter_body("[letter.select]\ntags = [\"data\"]\n"));
    let run = letter_spec(&plain, &staged.spec, &[]);
    assert!(!run.stdout.contains("I led the gateway work."), "{}", run.stdout);
}

#[test]
fn a_resume_override_does_not_reach_the_letter_and_the_letters_does_not_reach_the_resume() {
    let work = Workspace::new();
    let staged = stage(
        &work,
        &format!(
            "[select]\ntags = [\"data\"]\n\n[select.override.gateway]\ntags = [\"data\"]\n\n{}",
            letter_body(
                "[letter.select]\ntags = [\"data\"]\n\n[letter.select.override.mentoring]\ntags = [\"data\"]\n"
            )
        ),
    );
    let letter = letter_spec(&work, &staged.spec, &[]);
    assert_eq!(letter.outcome, Outcome::Success, "{}", letter.stderr);
    assert!(
        !letter.stdout.contains("I led the gateway work."),
        "the resume's override is not the letter's: {}",
        letter.stdout
    );
    assert!(
        letter.stdout.contains("I mentored the team."),
        "the letter's own override applies: {}",
        letter.stdout
    );
    let resume = build_spec(&work, &staged.spec, &[]);
    assert_eq!(resume.outcome, Outcome::Success, "{}", resume.stderr);
    let text = pdf_text(&work);
    assert!(
        text.contains("engine gateway"),
        "the resume's own override applies: {text}"
    );
    assert!(
        !text.contains("Mentored 6 engineers"),
        "the letter's override is not the resume's: {text}"
    );
}

#[test]
fn a_letter_override_naming_nothing_fails_before_any_text_is_printed() {
    let work = Workspace::new();
    let staged = stage(&work, &letter_body("[letter.select.override.nope]\ntags = [\"ai\"]\n"));
    let run = letter_spec(&work, &staged.spec, &[]);
    assert_eq!(run.outcome, Outcome::Failure, "{}", run.stderr);
    assert_eq!(run.stdout, "", "no letter text");
    assert!(
        run.stderr.contains("`&nope`") && run.stderr.contains("acme.toml:"),
        "{}",
        run.stderr
    );
}

// ---- the semantics an override must leave alone -------------------------------------------------------------

#[test]
fn promote_ranks_by_the_tags_an_override_gave() {
    // `promote` moves items with a promoted tag up their own list. An override that gives the last bullet the
    // promoted tag must move it up, so promotion reads the effective tags and not the baseline's.
    let work = Workspace::new();
    let staged = stage(
        &work,
        "[select]\npromote = [\"leadership\"]\n\n[select.override.cache]\ntags = [\"leadership\"]\n",
    );
    let run = build_spec(&work, &staged.spec, &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let text = pdf_text(&work);
    let at = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("{needle} in {text}"));
    assert!(
        at("weekly run") < at("engine gateway"),
        "the retagged item rose above an unpromoted one: {text}"
    );
}

#[test]
fn a_draft_item_stays_out_after_an_override_makes_it_core() {
    let work = Workspace::new();
    let staged = stage(&work, "[select.override.drafted]\npriority = \"core\"\n");
    fs::write(
        work.path().join("baseline.md"),
        format!("{BASELINE}- A claim not ready yet. <!-- &drafted @draft -->\n"),
    )
    .expect("baseline");
    let run = build_spec(&work, &staged.spec, &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(!pdf_text(&work).contains("not ready yet"), "{}", pdf_text(&work));
}

#[test]
fn a_tag_on_a_section_still_applies_to_its_items_after_an_item_is_cleared() {
    // Clearing a child's own tags does not mask the tag its section gives it.
    let work = Workspace::new();
    let baseline = "# Ada Lovelace\n\nLondon · ada@example.com\n\n## Skills <!-- #arch -->\n\n- Rust. <!-- &rust #data -->\n- Go.\n\n## Other\n\n- Plain.\n";
    let staged = stage(
        &work,
        "[select]\ntags = [\"data\"]\n\n[select.override.rust]\ntags = []\n",
    );
    fs::write(work.path().join("baseline.md"), baseline).expect("baseline");
    let run = build_spec(&work, &staged.spec, &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let text = pdf_text(&work);
    assert!(
        !text.contains("Rust."),
        "the section's #arch is not `data`, and the child's own tag is cleared: {text}"
    );
    assert!(text.contains("Plain."), "an untagged item elsewhere is kept: {text}");
}

// ---- profiles through the real loader, both documents, and flattening ---------------------------------------

/// What a spec says about one item: its own tags (if said) and its priority (if said), as text.
type Said = (Option<Vec<String>>, Option<String>);

/// The override tables of a resolved spec, as a comparable map: `(document, id) -> what was said`.
fn override_choices(spec: &resume::spec::ResolvedSpec) -> std::collections::BTreeMap<(String, String), Said> {
    let mut out = std::collections::BTreeMap::new();
    let mut take =
        |document: &str,
         overrides: &std::collections::BTreeMap<resume::model::ItemId, resume::spec::merge::ItemOverride>| {
            for (id, o) in overrides {
                out.insert(
                    (document.to_string(), id.to_string()),
                    (
                        o.tags.as_ref().map(|t| t.iter().map(ToString::to_string).collect()),
                        o.priority.map(|p| p.to_string()),
                    ),
                );
            }
        };
    take("resume", &spec.choices.select.overrides);
    if let Some(letter) = spec.letter() {
        take("letter", &letter.choices.overrides);
    }
    out
}

#[test]
fn profile_and_spec_overrides_merge_per_field_for_both_documents_and_survive_flattening() {
    let work = Workspace::new();
    let staged = stage_with(
        &work,
        "extends = [\"base\"]\n\n",
        &format!(
            "[select.override.gateway]\ntags = [\"data\"]\n\n[select.override.cache]\npriority = \"none\"\n\n{}",
            letter_body("[letter.select.override.gateway]\ntags = []\n")
        ),
    );
    fs::write(
        staged.profiles.join("base.toml"),
        "[select.override.gateway]\ntags = [\"ai\"]\npriority = \"core\"\n\n[select.override.cache]\ntags = [\"x\"]\npriority = \"core\"\n\n\
         [letter.select.override.gateway]\ntags = [\"ai\"]\npriority = \"extra\"\n",
    )
    .expect("profile");
    let spec = assert_ok!(resume::spec::resolve(&staged.spec, Some(&staged.profiles)));
    let got = override_choices(&spec);
    let key = |d: &str, i: &str| (d.to_string(), i.to_string());
    // The spec wins the field it writes and leaves the others to the profile, for each target and document.
    assert_eq!(
        got[&key("resume", "gateway")],
        (Some(vec!["data".into()]), Some("core".into()))
    );
    assert_eq!(
        got[&key("resume", "cache")],
        (Some(vec!["x".into()]), Some("none".into()))
    );
    assert_eq!(got[&key("letter", "gateway")], (Some(vec![]), Some("extra".into())));
    // Each field keeps the file that decided it.
    let file = |k: resume::spec::ChoiceKey| spec.origin_of(&k).map(|l| l.path.display().to_string());
    let id = |n: &str| assert_ok!(resume::model::ItemId::try_new(n));
    assert!(file(resume::spec::ChoiceKey::OverrideTags(id("gateway"))).is_some_and(|f| f.ends_with("acme.toml")));
    assert!(file(resume::spec::ChoiceKey::OverridePriority(id("gateway"))).is_some_and(|f| f.ends_with("base.toml")));
    // Flattening and resolving again gives the same choices: nothing was dropped on the way through the printer.
    let flat = work.path().join("positions/flat.toml");
    fs::write(&flat, spec.to_toml()).expect("flat");
    let again = assert_ok!(resume::spec::resolve(&flat, None));
    assert_eq!(
        override_choices(&again),
        got,
        "the printed spec means what the original meant"
    );
}

#[test]
fn an_inherited_letter_override_is_dropped_by_nothing_between_resolving_and_printing() {
    let work = Workspace::new();
    let staged = stage_with(&work, "extends = [\"base\"]\n\n", &letter_body(""));
    fs::write(
        staged.profiles.join("base.toml"),
        "[letter.select.override.opening]\nprio\
         rity = \"core\"\n",
    )
    .expect("profile");
    let spec = assert_ok!(resume::spec::resolve(&staged.spec, Some(&staged.profiles)));
    assert!(
        spec.to_toml().contains("[letter.select.override.opening]"),
        "{}",
        spec.to_toml()
    );
}

#[test]
fn a_group_override_with_a_pick_changes_every_variant_and_the_pick_still_chooses_the_wording() {
    let work = Workspace::new();
    let baseline = "# Ada Lovelace\n\nLondon · ada@example.com\n\n## S\n\n- Wording default. <!-- =g.default #ai -->\n- Wording short. <!-- =g.short #ai -->\n- Other. <!-- #data -->\n";
    let staged = stage(
        &work,
        "[select]\ntags = [\"data\"]\npick = { g = \"short\" }\n\n[select.override.g]\ntags = [\"data\"]\n",
    );
    fs::write(work.path().join("baseline.md"), baseline).expect("baseline");
    let run = build_spec(&work, &staged.spec, &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let text = pdf_text(&work);
    assert!(
        text.contains("Wording short"),
        "the pick still chooses the wording, now kept by the group's new tag: {text}"
    );
    assert!(!text.contains("Wording default"), "{text}");
}

#[test]
fn a_picked_draft_wording_is_still_an_error_when_the_group_is_overridden() {
    let work = Workspace::new();
    let baseline = "# Ada Lovelace\n\nLondon · ada@example.com\n\n## S\n\n- Wording default. <!-- =g.default -->\n- Wording short. <!-- =g.short @draft -->\n";
    let staged = stage(
        &work,
        "[select]\npick = { g = \"short\" }\n\n[select.override.g]\npriority = \"core\"\n",
    );
    fs::write(work.path().join("baseline.md"), baseline).expect("baseline");
    let run = build_spec(&work, &staged.spec, &[]);
    assert_eq!(run.outcome, Outcome::Failure, "{}", run.stderr);
    assert!(run.stderr.contains("draft"), "{}", run.stderr);
    assert!(!work.out().join("acme.pdf").exists());
}

#[test]
fn an_inherited_promotion_tag_follows_an_overridden_container() {
    // Items inherit the section's `leadership`, so with it a promoted item rises above an unpromoted one in its own
    // list. Clearing the section's own tags removes what they inherited: nothing rises, and the spec is told that
    // no node carries the tag any more.
    let baseline = "# Ada Lovelace\n\nLondon · ada@example.com\n\n## Skills <!-- &skills #leadership -->\n\n- First item.\n- Second item. <!-- #leadership -->\n";
    let with_tag = Workspace::new();
    let staged = stage(&with_tag, "[select]\npromote = [\"leadership\"]\n");
    fs::write(with_tag.path().join("baseline.md"), baseline).expect("baseline");
    let kept = build_spec(&with_tag, &staged.spec, &[]);
    assert_eq!(kept.outcome, Outcome::Success, "{}", kept.stderr);
    assert!(
        !kept.stderr.contains("unknown-tag"),
        "the tag is carried: {}",
        kept.stderr
    );

    let cleared = Workspace::new();
    let staged = stage(
        &cleared,
        "[select]\npromote = [\"leadership\"]\n\n[select.override.skills]\ntags = []\n\n[select.override.second]\ntags = []\n",
    );
    fs::write(
        cleared.path().join("baseline.md"),
        baseline.replace("<!-- #leadership -->\n", "<!-- &second #leadership -->\n"),
    )
    .expect("baseline");
    let run = build_spec(&cleared, &staged.spec, &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(
        run.stderr.contains("unknown-tag") && run.stderr.contains("leadership"),
        "with every carrier cleared the promote tag is unknown: {}",
        run.stderr
    );
}

#[test]
fn an_id_only_letter_builds_the_same_text_as_one_with_no_ids_and_a_no_op_override_changes_nothing() {
    let plain = Workspace::new();
    let named = Workspace::new();
    let sp = stage(&plain, &letter_body(""));
    let sn = stage(
        &named,
        &letter_body("[letter.select.override.gateway]\ntags = [\"ai\"]\npriority = \"extra\"\n"),
    );
    fs::write(
        plain.path().join("letter.md"),
        LETTER
            .replace("&gateway ", "")
            .replace("&cache ", "")
            .replace("&mentoring ", ""),
    )
    .expect("plain letter");
    let a = letter_spec(&plain, &sp.spec, &[]);
    let b = letter_spec(&named, &sn.spec, &[]);
    assert_eq!(a.outcome, Outcome::Success, "{}", a.stderr);
    assert_eq!(b.outcome, Outcome::Success, "{}", b.stderr);
    assert_eq!(
        a.stdout, b.stdout,
        "ids and an override that restates the item add nothing to the letter"
    );
}
