//! The stand-up as lines and as the text to paste, links included.

use super::*;

/// A line of the stand-up, before its format.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Line {
    Title(String),
    /// "Since …", "Today", "Waiting on others".
    Part(String),
    Heading(Section),
    /// An MR, in Markdown.
    Item(String),
    /// "Nothing on GitLab.": a part with nothing to say.
    Nothing(String),
    Gap,
}

/// The lines of `standup`, the sections `settings` hides left out.
pub(super) fn lines(standup: &Standup, settings: &Settings) -> Vec<Line> {
    let mut out = vec![
        Line::Title(tf(
            "Stand-up · {date}",
            &[("date", &standup.date.format("%Y-%m-%d").to_string())],
        )),
        Line::Gap,
        Line::Part(standup.since.clone()),
    ];
    let part = |out: &mut Vec<Line>, sections: &[Section], nothing: &str| {
        let before = out.len();
        for s in sections.iter().filter(|s| settings.shows(**s)) {
            let items = standup.of(*s);
            if !items.is_empty() {
                out.push(Line::Heading(*s));
                out.extend(items.iter().cloned().map(Line::Item));
            }
        }
        if out.len() == before {
            out.push(Line::Nothing(nothing.to_string()));
        }
    };
    part(&mut out, &Section::DONE, tl("Nothing on GitLab."));
    out.extend([Line::Gap, Line::Part(tl("Today").to_string())]);
    part(&mut out, &Section::TODAY, tl("Nothing waits for me."));
    let waiting = standup.of(Section::Waiting);
    if settings.shows(Section::Waiting) && !waiting.is_empty() {
        out.extend([
            Line::Gap,
            Line::Part(Section::Waiting.heading().to_string()),
        ]);
        out.extend(waiting.iter().cloned().map(Line::Item));
    }
    out
}

/// `text`'s Markdown links `[label](url)` cut out: the plain pieces, and the
/// links as (label, url).
pub(super) fn pieces(text: &str) -> Vec<(&str, Option<&str>)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let link = rest[open..].find("](").and_then(|mid| {
            let label = &rest[open + 1..open + mid];
            let after = &rest[open + mid + 2..];
            let close = after.find(')')?;
            let url = &after[..close];
            (!label.contains('[') && url.starts_with("http")).then_some((
                label,
                url,
                open + mid + 2 + close + 1,
            ))
        });
        match link {
            Some((label, url, end)) => {
                if open > 0 {
                    out.push((&rest[..open], None));
                }
                out.push((label, Some(url)));
                rest = &rest[end..];
            }
            None => {
                out.push((&rest[..open + 1], None));
                rest = &rest[open + 1..];
            }
        }
    }
    if !rest.is_empty() {
        out.push((rest, None));
    }
    out
}

/// `text`'s Markdown links written as `format` wants them.
pub(super) fn relink(text: &str, format: Format) -> String {
    if format == Format::Markdown {
        return text.to_string();
    }
    pieces(text)
        .into_iter()
        .map(|(label, url)| match (url, format) {
            (Some(url), Format::Slack) => format!("<{url}|{label}>"),
            _ => label.to_string(),
        })
        .collect()
}

/// The text to paste.
pub(super) fn render(lines: &[Line], format: Format) -> String {
    let bullet = if format == Format::Slack { "•" } else { "-" };
    let mut out = String::new();
    for line in lines {
        let text = match line {
            Line::Title(t) => match format {
                Format::Markdown => format!("**{t}**"),
                Format::Slack => format!("*{t}*"),
                Format::Plain => t.clone(),
            },
            Line::Part(t) => match format {
                Format::Plain => t.clone(),
                _ => format!("_{t}_"),
            },
            Line::Heading(s) => s.heading().to_string(),
            Line::Item(t) => format!("{bullet} {}", relink(t, format)),
            Line::Nothing(t) => format!("{bullet} {t}"),
            Line::Gap => String::new(),
        };
        out.push_str(&text);
        out.push('\n');
    }
    out
}

/// The stand-up's text, as `settings` asks, at `now`.
pub(super) fn text(
    g: &GuiApp,
    items: &[Item],
    settings: &Settings,
    now: chrono::DateTime<chrono::Local>,
) -> String {
    render(
        &lines(&build(g, items, settings, now), settings),
        settings.format,
    )
}
