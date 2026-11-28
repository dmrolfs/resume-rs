//! Markdown presentation of engine models. Literal field values are escaped, never interpreted as markup.
use ::resume::{
    letter::{Letter, LetterInline},
    model::{Block, Inline, NodeHeading, NodeKind, NodeView, Resume, RoleBlock},
};

pub(super) fn literal(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

pub(super) fn code(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let padded = text.starts_with('`')
        || text.ends_with('`')
        || (text.starts_with(' ') && text.ends_with(' ') && text.chars().any(|c| c != ' '));
    if padded {
        format!("{fence} {text} {fence}")
    } else {
        format!("{fence}{text}{fence}")
    }
}

pub(super) fn inline(nodes: &[Inline]) -> String {
    nodes
        .iter()
        .map(|node| match node {
            Inline::Text { text } => literal(text),
            Inline::Strong { content } => format!("**{}**", inline(content)),
            Inline::Emph { content } => format!("*{}*", inline(content)),
            Inline::Code { text } => code(text),
            Inline::Link { url, content } => format!(
                "[{}](<{}>)",
                inline(content),
                url.as_ref().as_str().replace('<', "%3C").replace('>', "%3E")
            ),
        })
        .collect()
}

pub(super) fn letter_inline(nodes: &[LetterInline]) -> String {
    nodes
        .iter()
        .map(|node| match node {
            LetterInline::Text(text) => literal(text),
            LetterInline::Code(text) => code(text),
            LetterInline::Strong(content) => format!("**{}**", letter_inline(content)),
            LetterInline::Emph(content) => format!("*{}*", letter_inline(content)),
            LetterInline::Link { url, content } => format!(
                "[{}](<{}>)",
                letter_inline(content),
                url.as_ref().as_str().replace('<', "%3C").replace('>', "%3E")
            ),
            LetterInline::Placeholder { key, .. } => format!("{{{{{key}}}}}"),
        })
        .collect()
}

pub(super) fn node(node: &NodeView<'_>) -> String {
    if let Some(heading) = node.heading {
        let text = match heading {
            NodeHeading::Section(title) => title.to_string(),
            NodeHeading::Era(name) => name.to_string(),
            NodeHeading::Role(role) => format!(
                "{}{} · {}",
                role.company,
                role.title.as_ref().map_or(String::new(), |t| format!(" · {t}")),
                role.period.text
            ),
        };
        return format!("{} {}", "#".repeat(node.depth + 2), literal(&text));
    }
    let label = node
        .label
        .map_or(String::new(), |label| format!("**{}:** ", inline(label)));
    format!(
        "{}{label}{}",
        if node.kind == NodeKind::Bullet { "- " } else { "" },
        inline(node.content.unwrap_or_default())
    )
}

pub(super) fn resume(resume: &Resume) -> String {
    let mut parts = vec![format!("# {}", literal(resume.name.as_ref())), inline(&resume.contact)];
    for section in &resume.sections {
        parts.push(format!("## {}", literal(section.title.as_ref())));
        for block in &section.blocks {
            match block {
                Block::Paragraph { content, .. } => parts.push(inline(content)),
                Block::Bullets { items } => parts.push(bullets(items)),
                Block::PageBreak => parts.push("---".into()),
                Block::Role(role) => {
                    parts.push(format!(
                        "### {}{} · {}",
                        literal(role.company.as_ref()),
                        role.title
                            .as_ref()
                            .map_or(String::new(), |t| format!(" · {}", literal(t.as_ref()))),
                        literal(&role.period.text)
                    ));
                    if let Some(blurb) = &role.blurb {
                        parts.push(inline(blurb));
                    }
                    for block in &role.body {
                        match block {
                            RoleBlock::Paragraph { content, .. } => parts.push(inline(content)),
                            RoleBlock::Bullets { items } => parts.push(bullets(items)),
                            RoleBlock::Era {
                                name, period, items, ..
                            } => {
                                parts.push(format!("#### {} · {}", literal(name.as_ref()), literal(&period.text)));
                                parts.push(bullets(items));
                            }
                        }
                    }
                }
            }
        }
    }
    parts.join("\n\n")
}

fn bullets(items: &[::resume::model::Bullet]) -> String {
    items
        .iter()
        .map(|b| {
            let label = b
                .label
                .as_ref()
                .map_or(String::new(), |l| format!("**{}:** ", inline(l)));
            format!("- {label}{}", inline(&b.content))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn letter(letter: &Letter) -> String {
    let mut parts = vec![format!("# {}", literal(letter.name.as_ref())), inline(&letter.contact)];
    parts.extend(letter.date.as_ref().map(|date| literal(&date.to_string())));
    parts.extend(letter.recipient.as_ref().map(|value| literal(value.as_ref())));
    parts.extend(letter.company.as_ref().map(|value| literal(value.as_ref())));
    parts.push(inline(&letter.greeting));
    parts.extend(letter.body.iter().map(|p| inline(p)));
    parts.push(inline(&letter.closing));
    parts.join("\n\n")
}
