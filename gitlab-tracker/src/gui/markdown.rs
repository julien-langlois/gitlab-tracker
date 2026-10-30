//! The MR description as rendered Markdown. `egui_commonmark` draws CommonMark; what
//! GitLab adds or allows on top is rewritten into plain Markdown first (see
//! [`prepare`]): raw HTML, `!1` / `#1` / `@user` references, bare URLs, and links
//! relative to the project. Images become links: they would need the token on a
//! private project, and the sidebar is too narrow to show them well. Lists holding
//! tasks are drawn here (see [`blocks`]): the viewer gives a task both a bullet and a
//! box, and takes the box's look from the theme, where it is round.

use std::ops::Range;

use bevel_theme::Theme;
use egui::{
    Align2, Color32, CornerRadius, FontId, Sense, Shape, Stroke, StrokeKind, TextStyle, Ui, Vec2,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::models::TrackedMr;

/// The options `egui_commonmark` parses with: both passes must agree on what is code.
const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_FOOTNOTES)
    .union(Options::ENABLE_DEFINITION_LIST);

/// The viewer's state, and the shown MR's description once rewritten: doing that
/// on every frame would be wasted.
#[derive(Default)]
pub struct Description {
    cache: CommonMarkCache,
    /// What `blocks` was made from: the description, the MR's URL, its target.
    from: (String, String, String),
    blocks: Vec<Block>,
}

impl Description {
    pub fn show(&mut self, ui: &mut Ui, theme: &Theme, mr: &TrackedMr) {
        let (description, web_url, target) = &self.from;
        if description != &mr.description || web_url != &mr.web_url || target != &mr.target_branch {
            self.from = (
                mr.description.clone(),
                mr.web_url.clone(),
                mr.target_branch.clone(),
            );
            let links = Links::new(&mr.web_url, &mr.target_branch);
            let prepared = prepare(mr.description.trim(), &links);
            self.blocks = blocks(&prepared, &definitions(&prepared));
        }
        ui.scope(|ui| {
            // Rows as high as the text: the viewer draws list markers at that height, and
            // the theme's taller controls would leave them above the text they mark.
            ui.spacing_mut().interact_size.y = ui.text_style_height(&TextStyle::Body);
            let style = ui.style_mut();
            // Back to the theme's secondary text: the theme's override would also take
            // the colour of the links and of the bold text away.
            style.visuals.override_text_color = None;
            // The sidebar is narrow: a title stays close to the text's size.
            style.text_styles.insert(
                TextStyle::Heading,
                FontId::proportional(theme.text_size + 3.0),
            );
            show_blocks(ui, theme, &mut self.cache, &self.blocks);
        });
    }
}

/// A part of a description: text for the viewer, or a list drawn here.
#[derive(Debug, PartialEq)]
enum Block {
    Text(String),
    List(Vec<Item>),
}

#[derive(Debug, PartialEq)]
struct Item {
    marker: Marker,
    body: Vec<Block>,
}

#[derive(Debug, PartialEq)]
enum Marker {
    Task { checked: bool },
    Bullet,
    Number(u64),
}

fn show_blocks(ui: &mut Ui, theme: &Theme, cache: &mut CommonMarkCache, blocks: &[Block]) {
    for block in blocks {
        match block {
            Block::Text(text) => {
                CommonMarkViewer::new().show(ui, cache, text);
            }
            Block::List(items) => {
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    for item in items {
                        ui.horizontal_top(|ui| {
                            show_marker(ui, theme, &item.marker);
                            ui.vertical(|ui| show_blocks(ui, theme, cache, &item.body));
                        });
                    }
                });
            }
        }
    }
}

fn show_marker(ui: &mut Ui, theme: &Theme, marker: &Marker) {
    let height = ui.text_style_height(&TextStyle::Body);
    if let Marker::Number(number) = marker {
        let galley = ui.painter().layout_no_wrap(
            format!("{number}."),
            TextStyle::Body.resolve(ui.style()),
            theme.palette.text_secondary,
        );
        let width = galley.size().x.max(14.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        ui.painter().text(
            rect.right_center(),
            Align2::RIGHT_CENTER,
            format!("{number}."),
            TextStyle::Body.resolve(ui.style()),
            theme.palette.text_secondary,
        );
        return;
    }
    let (rect, _) = ui.allocate_exact_size(Vec2::new(14.0, height), Sense::hover());
    let painter = ui.painter();
    match marker {
        Marker::Task { checked } => {
            let square = egui::Rect::from_center_size(rect.center(), Vec2::splat(13.0));
            let radius = CornerRadius::same(3);
            if *checked {
                painter.rect_filled(square, radius, theme.palette.accent);
                let mark = square.shrink(3.0);
                painter.add(Shape::line(
                    vec![
                        egui::pos2(mark.left(), mark.center().y),
                        egui::pos2(mark.left() + mark.width() * 0.4, mark.bottom()),
                        mark.right_top(),
                    ],
                    Stroke::new(1.6, Color32::WHITE),
                ));
            } else {
                painter.rect_stroke(
                    square,
                    radius,
                    Stroke::new(1.0, theme.palette.text_muted),
                    StrokeKind::Inside,
                );
            }
        }
        _ => {
            painter.circle_filled(rect.center(), 2.0, theme.palette.text_secondary);
        }
    }
}

/// The reference definitions of a text (`[1]: https://a.io`), which every block needs
/// once the text is cut: a link may be far from its definition.
fn definitions(source: &str) -> String {
    let parser = Parser::new_ext(source, OPTIONS).into_offset_iter();
    let spans = parser.reference_definitions().iter();
    let lines: Vec<_> = spans.map(|(_, def)| &source[def.span.clone()]).collect();
    lines.join("\n")
}

/// Cuts a text around its lists that hold tasks; the rest stays for the viewer, whose
/// plain lists and quotes are fine as they are.
fn blocks(source: &str, definitions: &str) -> Vec<Block> {
    let events: Vec<_> = Parser::new_ext(source, OPTIONS)
        .into_offset_iter()
        .collect();
    let mut out = Vec::new();
    let mut copied = 0;
    let mut at = 0;
    while at < events.len() {
        let (event, range) = &events[at];
        if !matches!(event, Event::Start(_)) {
            at += 1;
            continue;
        }
        let end = closing(&events, at);
        if let Event::Start(Tag::List(first)) = event {
            let list = &events[at..=end];
            if list
                .iter()
                .any(|(e, _)| matches!(e, Event::TaskListMarker(_)))
            {
                push_text(&mut out, &source[copied..range.start], definitions);
                out.push(Block::List(items(source, list, *first, definitions)));
                copied = range.end;
            }
        }
        at = end + 1;
    }
    push_text(&mut out, &source[copied..], definitions);
    out
}

fn push_text(out: &mut Vec<Block>, text: &str, definitions: &str) {
    if !text.trim().is_empty() {
        out.push(Block::Text(format!("{}\n\n{definitions}", text.trim_end())));
    }
}

/// The index of the event closing the one opened at `at`.
fn closing(events: &[(Event, Range<usize>)], at: usize) -> usize {
    let mut depth = 0;
    for (index, (event, _)) in events.iter().enumerate().skip(at) {
        match event {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return index;
        }
    }
    events.len() - 1
}

fn items(
    source: &str,
    list: &[(Event, Range<usize>)],
    first: Option<u64>,
    definitions: &str,
) -> Vec<Item> {
    let mut out = Vec::new();
    let mut at = 1;
    while at + 1 < list.len() {
        let end = closing(list, at);
        let task = list[at + 1..=end]
            .iter()
            .take(2)
            .find_map(|(event, range)| match event {
                Event::TaskListMarker(checked) => Some((*checked, range.end)),
                _ => None,
            });
        let number = first.map(|first| first + out.len() as u64);
        let marker = match (task, number) {
            (Some((checked, _)), _) => Marker::Task { checked },
            (None, Some(number)) => Marker::Number(number),
            (None, None) => Marker::Bullet,
        };
        let body = item_body(source, list[at].1.clone(), task.map(|(_, end)| end));
        out.push(Item {
            marker,
            body: blocks(&body, definitions),
        });
        at = end + 1;
    }
    out
}

/// An item's Markdown, without its marker and without the indentation that made its
/// other lines part of it. `task_end` is where the item's `[ ]` ends, if it has one.
fn item_body(source: &str, range: Range<usize>, task_end: Option<usize>) -> String {
    let line_start = source[..range.start].rfind('\n').map_or(0, |at| at + 1);
    let item = &source[range.clone()];
    let marker = item
        .find(|c: char| !c.is_ascii_digit() && !"-+*.)".contains(c))
        .unwrap_or(item.len());
    let padding = item[marker..].len() - item[marker..].trim_start_matches(' ').len();
    let padding = if (1..=4).contains(&padding) {
        padding
    } else {
        1
    };
    let column = range.start - line_start + marker + padding;
    let start = task_end.unwrap_or(range.start + marker);
    let mut lines = source[start..range.end].trim_end().lines();
    let mut body = lines.next().unwrap_or_default().trim_start().to_owned();
    for line in lines {
        let indent = line.len() - line.trim_start_matches(' ').len();
        body.push('\n');
        body.push_str(&line[indent.min(column)..]);
    }
    body
}

/// Where the links of a description lead. The fields are empty when the MR's URL
/// is unusable: only absolute links are kept then.
struct Links<'a> {
    /// `https://host/group/project/-/merge_requests/1`
    mr: &'a str,
    /// `https://host/group/project`
    project: &'a str,
    /// `https://host`
    host: &'a str,
    branch: &'a str,
}

impl<'a> Links<'a> {
    fn new(web_url: &'a str, branch: &'a str) -> Self {
        let project = web_url.split("/-/").next().unwrap_or_default();
        let host = project
            .find("://")
            .filter(|_| project.starts_with("http"))
            .and_then(|scheme| {
                let path = project[scheme + 3..].find('/')?;
                Some(&project[..scheme + 3 + path])
            });
        match host {
            Some(host) => Self {
                mr: web_url,
                project,
                host,
                branch,
            },
            None => Self {
                mr: "",
                project: "",
                host: "",
                branch,
            },
        }
    }

    /// The address a link's destination opens, or `None` for one that must not open:
    /// the text is third-party, and `file:` or `javascript:` have no place in it.
    fn resolve(&self, dest: &str) -> Option<String> {
        let dest = dest.trim();
        if dest.is_empty() {
            return None;
        }
        if dest.split_once(':').is_some_and(|(s, _)| is_scheme(s)) {
            let scheme = dest.split(':').next().unwrap_or_default();
            let allowed = ["http", "https", "mailto"]
                .iter()
                .any(|s| s.eq_ignore_ascii_case(scheme));
            return allowed.then(|| dest.to_owned());
        }
        if self.project.is_empty() {
            return None;
        }
        Some(if dest.starts_with("//") {
            format!("https:{dest}")
        } else if dest.starts_with("/uploads/") {
            format!("{}{dest}", self.project)
        } else if dest.starts_with('/') {
            format!("{}{dest}", self.host)
        } else if dest.starts_with('#') {
            format!("{}{dest}", self.mr)
        } else {
            let path = dest.strip_prefix("./").unwrap_or(dest);
            format!("{}/-/blob/{}/{path}", self.project, self.branch)
        })
    }

    /// The link starting `text`, if any: its length and its Markdown.
    fn at(&self, text: &str) -> Option<(usize, String)> {
        if let Some(found) = bare_url(text) {
            return Some(found);
        }
        if self.project.is_empty() {
            return None;
        }
        let mut chars = text.chars();
        match chars.next()? {
            kind @ ('!' | '#') => {
                let digits = chars.as_str();
                let len = digits.find(|c: char| !c.is_ascii_digit());
                let len = len.unwrap_or(digits.len());
                let after = digits[len..].chars().next();
                if len == 0 || after.is_some_and(is_word) {
                    return None;
                }
                let path = if kind == '!' {
                    "merge_requests"
                } else {
                    "issues"
                };
                let url = format!("{}/-/{path}/{}", self.project, &digits[..len]);
                Some((len + 1, format!("[{}](<{url}>)", &text[..=len])))
            }
            '@' => {
                let name = chars.as_str();
                let len = name.find(|c: char| !is_word(c) && !"-./".contains(c));
                let name = name[..len.unwrap_or(name.len())].trim_end_matches(['-', '.', '/']);
                let first = name.chars().next()?;
                if !is_word(first) || name == "all" {
                    return None;
                }
                let url = format!("{}/{name}", self.host);
                let label = escape(&format!("@{name}"));
                Some((name.len() + 1, format!("[{label}](<{url}>)")))
            }
            _ => None,
        }
    }
}

fn is_scheme(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// A URL starting `text`, as GitLab links it: up to a space, without the punctuation
/// that closes the sentence around it.
fn bare_url(text: &str) -> Option<(usize, String)> {
    let host = text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"))?;
    if !host.starts_with(|c: char| c.is_alphanumeric()) {
        return None;
    }
    let end = text.find(|c: char| c.is_whitespace() || "<>\"".contains(c));
    let mut url = &text[..end.unwrap_or(text.len())];
    loop {
        let trimmed = url.trim_end_matches(['.', ',', ';', ':', '!', '?', '\'', '*', '_', '~']);
        let unbalanced =
            trimmed.ends_with(')') && trimmed.matches(')').count() > trimmed.matches('(').count();
        url = if unbalanced {
            &trimmed[..trimmed.len() - 1]
        } else {
            trimmed
        };
        if !unbalanced {
            return Some((url.len(), format!("<{url}>")));
        }
    }
}

/// The links in `text`, as edits. `before` is what precedes it: a reference starts a
/// word, so `a@b.c`, `C#1` and the entity `&#45;` are left alone.
fn links_in(text: &str, before: Option<char>, links: &Links) -> Vec<(Range<usize>, String)> {
    let mut edits = Vec::new();
    let mut previous = before;
    let mut at = 0;
    while let Some(c) = text[at..].chars().next() {
        let starts_word = !previous.is_some_and(|p| is_word(p) || "-/&\\#!@".contains(p));
        if let Some((len, link)) = links.at(&text[at..]).filter(|_| starts_word) {
            edits.push((at..at + len, link));
            previous = text[at..at + len].chars().next_back();
            at += len;
        } else {
            previous = Some(c);
            at += c.len_utf8();
        }
    }
    edits
}

fn apply(source: &str, edits: &mut [(Range<usize>, String)]) -> String {
    edits.sort_by_key(|(range, _)| range.start);
    let mut out = String::with_capacity(source.len());
    let mut copied = 0;
    for (range, text) in edits.iter() {
        if range.start >= copied {
            out.push_str(&source[copied..range.start]);
            out.push_str(text);
            copied = range.end;
        }
    }
    out.push_str(&source[copied..]);
    out
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\[]*_`<".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A link or an image being walked through.
struct Open {
    /// How many edits were pending at its start: the ones after are inside it.
    mark: usize,
    dest: String,
    /// Its text, without the formatting.
    label: String,
}

/// Rewrites `source` into the Markdown `egui_commonmark` can show. The source is
/// parsed rather than scanned so that code, which must stay as written, is told apart.
fn prepare(source: &str, links: &Links) -> String {
    let parser = Parser::new_ext(source, OPTIONS).into_offset_iter();
    let mut rewriter = Rewriter {
        source,
        links,
        edits: Vec::new(),
        open: Vec::new(),
        run: None,
        code_blocks: 0,
        one_line: 0,
        anchor: None,
        code_tag: false,
    };
    for (_, definition) in parser.reference_definitions().iter() {
        let span = definition.span.clone();
        rewriter.retarget(span, &definition.dest);
    }
    for (event, range) in parser {
        rewriter.event(event, range);
    }
    rewriter.flush();
    apply(source, &mut rewriter.edits)
}

struct Rewriter<'a> {
    source: &'a str,
    links: &'a Links<'a>,
    edits: Vec<(Range<usize>, String)>,
    open: Vec<Open>,
    /// Adjoining text events: the parser splits text at `_`, `!`, `[`…
    run: Option<Range<usize>>,
    code_blocks: usize,
    /// Inside a table or a heading, where a line break would end the element.
    one_line: usize,
    /// The address of the `<a>` tag being walked through.
    anchor: Option<String>,
    /// Inside a `<code>` tag.
    code_tag: bool,
}

impl Rewriter<'_> {
    fn event(&mut self, event: Event, range: Range<usize>) {
        if !matches!(event, Event::Text(_)) {
            self.flush();
        }
        match event {
            Event::Text(text) => {
                self.label(&text);
                if self.literal() {
                    return;
                }
                self.run = match self.run.take() {
                    Some(run) if run.end == range.start => Some(run.start..range.end),
                    Some(_) | None => {
                        self.flush();
                        Some(range)
                    }
                };
            }
            Event::Code(text) => self.label(&text),
            Event::InlineHtml(_) => {
                let html = strip_html(self, &self.source[range.clone()], false);
                self.edits.push((range, html));
            }
            Event::Start(Tag::HtmlBlock) => {
                let html = strip_html(self, &self.source[range.clone()], true);
                self.edits.push((range, html));
            }
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                self.open.push(Open {
                    mark: self.edits.len(),
                    dest: dest_url.into_string(),
                    label: String::new(),
                });
            }
            Event::End(TagEnd::Link) => self.end_link(range),
            Event::End(TagEnd::Image) => self.end_image(range),
            Event::Start(Tag::CodeBlock(_)) => self.code_blocks += 1,
            Event::End(TagEnd::CodeBlock) => self.code_blocks -= 1,
            Event::Start(Tag::Table(_) | Tag::Heading { .. }) => self.one_line += 1,
            Event::End(TagEnd::Table | TagEnd::Heading(_)) => self.one_line -= 1,
            _ => {}
        }
    }

    /// In code or in a link, where a reference is not one.
    fn literal(&self) -> bool {
        self.code_blocks > 0 || self.code_tag || self.anchor.is_some() || !self.open.is_empty()
    }

    fn label(&mut self, text: &str) {
        for open in &mut self.open {
            open.label.push_str(text);
        }
    }

    /// Turns the pending run of text into links.
    fn flush(&mut self) {
        let Some(run) = self.run.take() else {
            return;
        };
        let before = self.source[..run.start].chars().next_back();
        let found = links_in(&self.source[run.clone()], before, self.links);
        for (range, link) in found {
            let at = run.start + range.start..run.start + range.end;
            self.edits.push((at, link));
        }
    }

    /// Points the link written in `span` at its resolved address.
    fn retarget(&mut self, span: Range<usize>, dest: &str) {
        let Some(url) = self.links.resolve(dest).filter(|url| url != dest) else {
            return;
        };
        // From the end: the link's text comes first, and may hold the same words.
        if let Some(at) = self.source[span.clone()].rfind(dest) {
            let start = span.start + at;
            self.edits.push((start..start + dest.len(), url));
        }
    }

    fn end_link(&mut self, range: Range<usize>) {
        let Some(open) = self.open.pop() else {
            return;
        };
        if self.links.resolve(&open.dest).is_some() {
            self.retarget(range, &open.dest);
        } else {
            self.edits.truncate(open.mark);
            self.edits.push((range, escape(&open.label)));
        }
    }

    fn end_image(&mut self, range: Range<usize>) {
        let Some(open) = self.open.pop() else {
            return;
        };
        let label = match open.label.trim() {
            "" => file_name(&open.dest),
            label => label,
        };
        let label = escape(label);
        // GitLab's size attributes: `![](a.png){width=100}`.
        let after = &self.source[range.end..];
        let attributes = after
            .strip_prefix('{')
            .and_then(|rest| rest.split('\n').next()?.find('}'))
            .map_or(0, |close| close + 2);
        self.edits.truncate(open.mark);
        // An image already in a link only gives its text: `[![badge](a.png)](b)`.
        let url = self
            .links
            .resolve(&open.dest)
            .filter(|_| self.open.is_empty());
        let text = match url {
            Some(url) => format!("[{label}](<{url}>)"),
            None => label,
        };
        self.edits.push((range.start..range.end + attributes, text));
    }
}

/// The last part of a URL's path, to name an image without a text.
fn file_name(url: &str) -> &str {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(url)
}

/// The end of the tag opening `text`, which starts with `<`; `None` if it is not one
/// (`a < b`). A `>` between quotes does not close it.
fn tag_end(text: &str) -> Option<usize> {
    let first = text[1..].chars().next()?;
    if !first.is_ascii_alphabetic() && !"/!?".contains(first) {
        return None;
    }
    let mut quote = None;
    for (at, c) in text.char_indices() {
        match (quote, c) {
            (None, '>') => return Some(at),
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), _) if open == c => quote = None,
            _ => {}
        }
    }
    None
}

/// The value of an attribute in a tag's text.
fn attribute<'t>(tag: &'t str, name: &str) -> Option<&'t str> {
    let mut rest = tag;
    loop {
        let at = rest.to_ascii_lowercase().find(name)?;
        let starts = !rest[..at].ends_with(|c: char| !c.is_whitespace());
        let after = rest[at + name.len()..].trim_start();
        if let (true, Some(value)) = (starts, after.strip_prefix('=')) {
            let value = value.trim_start();
            let quote = value.chars().next().filter(|c| "\"'".contains(*c));
            return match quote {
                Some(quote) => value[1..].split(quote).next(),
                None => value.split_whitespace().next(),
            };
        }
        rest = &rest[at + name.len()..];
    }
}

/// HTML as Markdown: comments and scripts go, what Markdown can say is translated,
/// and any other tag only leaves its text, as `<details>` does here (the sidebar
/// shows the content, with its summary as a bold line).
fn strip_html(r: &mut Rewriter, html: &str, block: bool) -> String {
    let mut out = String::new();
    let mut rest = html;
    let mut summary = None;
    while let Some(lt) = rest.find('<') {
        push_html_text(r, &mut out, &rest[..lt], block);
        rest = &rest[lt..];
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.split_once("-->").map_or("", |(_, after)| after);
            continue;
        }
        let Some(end) = tag_end(rest) else {
            out.push('<');
            rest = &rest[1..];
            continue;
        };
        let tag = &rest[1..end];
        rest = &rest[end + 1..];
        let closing = tag.starts_with('/');
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match name.as_str() {
            "br" if r.one_line > 0 => out.push(' '),
            "br" => out.push_str("  \n"),
            "img" => {
                let src = attribute(tag, "src").unwrap_or_default();
                let alt = attribute(tag, "alt").filter(|alt| !alt.trim().is_empty());
                let label = escape(alt.unwrap_or_else(|| file_name(src)));
                match r.links.resolve(src).filter(|_| r.anchor.is_none()) {
                    Some(url) => out.push_str(&format!("[{label}](<{url}>)")),
                    None => out.push_str(&label),
                }
            }
            "a" if closing => {
                if let Some(url) = r.anchor.take() {
                    out.push_str(&format!("](<{url}>)"));
                }
            }
            "a" => {
                let url = attribute(tag, "href").and_then(|href| r.links.resolve(href));
                if url.is_some() && r.anchor.is_none() {
                    out.push('[');
                    r.anchor = url;
                }
            }
            "b" | "strong" => out.push_str("**"),
            "i" | "em" => out.push('*'),
            "s" | "del" | "strike" => out.push_str("~~"),
            "code" | "kbd" | "samp" | "tt" => {
                out.push('`');
                r.code_tag = !closing;
            }
            "summary" if closing => {
                if let Some(start) = summary.take() {
                    let text = out[start..].replace("**", "").trim().to_owned();
                    out.truncate(start);
                    out.push_str(&format!("**{text}**\n\n"));
                }
            }
            "summary" => summary = Some(out.len()),
            "script" | "style" if !closing => {
                let close = format!("</{name}");
                let at = rest.to_ascii_lowercase().find(&close);
                rest = &rest[at.unwrap_or(rest.len())..];
            }
            "td" | "th" if closing => out.push(' '),
            "p" | "div" | "tr" | "li" | "ul" | "ol" | "table" | "h1" | "h2" | "h3" | "h4"
            | "h5" | "h6"
                if closing && block =>
            {
                out.push_str("\n\n");
            }
            "li" if block => out.push_str("- "),
            _ => {}
        }
    }
    push_html_text(r, &mut out, rest, block);
    if block {
        // The tags' indentation would otherwise make a code block of the text left.
        let lines: Vec<_> = out.lines().map(str::trim_start).collect();
        out = lines.join("\n");
        out.push('\n');
    }
    out
}

fn push_html_text(r: &Rewriter, out: &mut String, text: &str, block: bool) {
    if r.code_tag || r.anchor.is_some() || !block {
        out.push_str(text);
    } else {
        let mut edits = links_in(text, out.chars().next_back(), r.links);
        out.push_str(&apply(text, &mut edits));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gitlab(source: &str) -> String {
        let links = Links::new("https://git.io/org/app/-/merge_requests/7", "main");
        prepare(source, &links)
    }

    #[test]
    fn references_become_links() {
        assert_eq!(
            gitlab("See !12, #34 and @jo_ann."),
            "See [!12](<https://git.io/org/app/-/merge_requests/12>), \
             [#34](<https://git.io/org/app/-/issues/34>) and \
             [@jo\\_ann](<https://git.io/jo_ann>)."
        );
    }

    #[test]
    fn what_only_looks_like_a_reference_stays() {
        for text in [
            "a@b.io", "C#1", "hello!12", "#12a", "&#45;", "\\#12", "@all", "# 12",
        ] {
            assert_eq!(gitlab(text), text);
        }
    }

    #[test]
    fn code_stays_as_written() {
        for text in [
            "`#12 <b>`",
            "```\n#12 <!-- x --> https://a.io\n```",
            "    !12 <br>",
        ] {
            assert_eq!(gitlab(text), text);
        }
    }

    #[test]
    fn bare_urls_become_links_without_their_punctuation() {
        assert_eq!(
            gitlab("See https://a.io/b_c/-/issues/1#note_2, (http://a.io/x)."),
            "See <https://a.io/b_c/-/issues/1#note_2>, (<http://a.io/x>)."
        );
        assert_eq!(gitlab("https://a.io/x_(y)"), "<https://a.io/x_(y)>");
    }

    #[test]
    fn a_link_keeps_its_references_as_text() {
        assert_eq!(gitlab("[fix #12](https://a.io)"), "[fix #12](https://a.io)");
        assert_eq!(gitlab("<https://a.io/#12>"), "<https://a.io/#12>");
    }

    #[test]
    fn relative_links_resolve_on_the_project() {
        for (dest, url) in [
            ("/uploads/a/f.pdf", "https://git.io/org/app/uploads/a/f.pdf"),
            ("/org/app/-/issues/1", "https://git.io/org/app/-/issues/1"),
            (
                "./docs/a.md",
                "https://git.io/org/app/-/blob/main/docs/a.md",
            ),
            ("docs/a.md", "https://git.io/org/app/-/blob/main/docs/a.md"),
            ("#why", "https://git.io/org/app/-/merge_requests/7#why"),
            ("//b.io/c", "https://b.io/c"),
        ] {
            assert_eq!(gitlab(&format!("[x]({dest})")), format!("[x]({url})"));
        }
        assert_eq!(
            gitlab("[x][1]\n\n[1]: /uploads/a.pdf"),
            "[x][1]\n\n[1]: https://git.io/org/app/uploads/a.pdf"
        );
    }

    #[test]
    fn links_that_must_not_open_lose_their_address() {
        for dest in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,x",
        ] {
            assert_eq!(gitlab(&format!("[click]({dest})")), "click");
        }
        assert_eq!(gitlab("[m](mailto:a@b.io)"), "[m](mailto:a@b.io)");
    }

    #[test]
    fn images_become_links() {
        assert_eq!(
            gitlab("![shot](/uploads/a/s.png){width=100 height=50} after"),
            "[shot](<https://git.io/org/app/uploads/a/s.png>) after"
        );
        assert_eq!(
            gitlab("![](/uploads/a/s.png)"),
            "[s.png](<https://git.io/org/app/uploads/a/s.png>)"
        );
        // Not left as a link in a link.
        assert_eq!(
            gitlab("[![badge](https://a.io/b.svg)](https://a.io)"),
            "[badge](https://a.io)"
        );
        assert_eq!(gitlab("![x](data:image/png;base64,AA)"), "x");
    }

    #[test]
    fn html_comments_disappear() {
        assert_eq!(gitlab("<!-- a\n\nb -->\nText"), "\nText");
        assert_eq!(gitlab("a <!-- b --> c"), "a  c");
    }

    #[test]
    fn details_keep_their_content() {
        assert_eq!(
            gitlab(
                "<details>\n  <summary> <b>Logs</b> #1 </summary>\n\n```\n<b>\n```\n\n</details>"
            ),
            "\n**Logs [#1](<https://git.io/org/app/-/issues/1>)**\n\n\n\n```\n<b>\n```\n\n\n"
        );
    }

    #[test]
    fn inline_tags_translate() {
        assert_eq!(gitlab("a<br>b"), "a  \nb");
        assert_eq!(gitlab("<b>a</b> <kbd>#1</kbd>"), "**a** `#1`");
        assert_eq!(
            gitlab("x <a href=\"/uploads/f.pdf\">f #1</a>"),
            "x [f #1](<https://git.io/org/app/uploads/f.pdf>)"
        );
        assert_eq!(
            gitlab("x <img src=\"/uploads/f.png\" width=\"10\" alt='f'> y"),
            "x [f](<https://git.io/org/app/uploads/f.png>) y"
        );
        assert_eq!(gitlab("a < b <c"), "a < b <c");
    }

    #[test]
    fn a_table_stays_on_its_lines() {
        assert_eq!(gitlab("| a |\n|---|\n| b<br>c |"), "| a |\n|---|\n| b c |");
    }

    #[test]
    fn indented_html_does_not_turn_into_code() {
        assert_eq!(gitlab("<div>\n      text\n</div>"), "\ntext\n\n\n");
    }

    fn text(text: &str) -> Block {
        Block::Text(format!("{text}\n\n"))
    }

    fn task(checked: bool, body: Vec<Block>) -> Item {
        Item {
            marker: Marker::Task { checked },
            body,
        }
    }

    #[test]
    fn a_list_of_tasks_is_cut_out_of_the_text() {
        assert_eq!(
            blocks("Before\n\n- [x] a\n- [ ] b\n\nAfter", ""),
            vec![
                text("Before"),
                Block::List(vec![
                    task(true, vec![text("a")]),
                    task(false, vec![text("b")]),
                ]),
                text("After"),
            ]
        );
    }

    #[test]
    fn plain_lists_and_quotes_stay_for_the_viewer() {
        let source = "- a\n- b\n\n> - [ ] c";
        assert_eq!(blocks(source, ""), vec![text(source)]);
    }

    #[test]
    fn an_item_keeps_what_is_nested_in_it() {
        assert_eq!(
            blocks("1. first\n   - [x] sub\n\n     more\n2. [ ] second", ""),
            vec![Block::List(vec![
                Item {
                    marker: Marker::Number(1),
                    body: vec![
                        text("first"),
                        Block::List(vec![task(true, vec![text("sub\n\nmore")])]),
                    ],
                },
                task(false, vec![text("second")]),
            ])]
        );
    }

    #[test]
    fn a_reference_link_finds_its_definition_in_every_block() {
        let source = "- [ ] [x][1]\n\n[y][1]\n\n[1]: https://a.io";
        let definitions = definitions(source);
        assert_eq!(definitions, "[1]: https://a.io");
        assert_eq!(
            blocks(source, &definitions),
            vec![
                Block::List(vec![task(
                    false,
                    vec![Block::Text("[x][1]\n\n[1]: https://a.io".into())],
                )]),
                Block::Text("[y][1]\n\n[1]: https://a.io\n\n[1]: https://a.io".into()),
            ]
        );
    }

    #[test]
    fn scripts_are_dropped() {
        assert_eq!(gitlab("<script>\nalert(1)\n</script>\n\nb"), "\n\nb");
    }

    #[test]
    fn an_unusable_url_keeps_only_absolute_links() {
        let links = Links::new("", "main");
        assert_eq!(
            prepare("#1 [a](/b) [c](https://d.io)", &links),
            "#1 a [c](https://d.io)"
        );
    }
}
