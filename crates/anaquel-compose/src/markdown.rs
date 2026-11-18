use std::slice::Iter;

use anaquel_catalog::{Catalog, Entry, Heading, ResolvedLink};
use anaquel_diagnostics::{Code, Diagnostic, Report};
use anaquel_directive::{find_term_refs, parse_blocks};
use pulldown_cmark::{CodeBlockKind, CowStr, Event, HeadingLevel, Parser, Tag, TagEnd, html};

use crate::blocks::render_nodes;
use crate::heading::{Lines, options};
use crate::highlight::{highlight, language_of};

/// A page after composition.
#[derive(Debug)]
pub struct Composed {
    /// HTML of the body, without the page title.
    pub html: String,
    /// Text of the body with no markup, for search and descriptions.
    pub plain: String,
    /// Site URLs of the pages and files this page links to, without anchors.
    pub links_out: Vec<String>,
    /// Origins whose iframes this page loads (from `:::embed`), sorted, without repeats.
    pub embed_origins: Vec<String>,
    /// Problems found while composing.
    pub report: Report,
}

/// What composing one page shares between its pieces of Markdown and its directives.
pub(crate) struct State<'a> {
    pub(crate) catalog: &'a Catalog,
    pub(crate) entry: &'a Entry,
    pub(crate) lines: Lines,
    pub(crate) report: Report,
    pub(crate) links_out: Vec<String>,
    /// Headings of the page not yet reached, in order.
    pub(crate) headings: Iter<'a, Heading>,
    /// Whether a leading `# Title` that repeats the page title is still to be left out.
    pub(crate) drop_h1: bool,
    pub(crate) plain: String,
    pub(crate) embed_origins: Vec<String>,
}

impl State<'_> {
    pub(crate) fn located(&self, diagnostic: Diagnostic, offset: usize) -> Diagnostic {
        diagnostic
            .at(self.entry.source.as_str())
            .on_line(self.entry.body_line - 1 + self.lines.at(offset))
    }

    /// Resolves a link target; on failure reports it and returns `#`.
    pub(crate) fn target(&mut self, href: &str, offset: usize) -> String {
        match self.catalog.resolve_link(self.entry.id, href) {
            Ok(ResolvedLink::External(url)) => url,
            Ok(ResolvedLink::Internal(url)) => {
                if !url.starts_with('#') {
                    self.links_out
                        .push(url.split('#').next().unwrap_or(&url).to_owned());
                }
                url
            }
            Err(diagnostic) => {
                let located = self.located(diagnostic, offset);
                self.report.push(located);
                "#".to_owned()
            }
        }
    }
}

/// Composes one entry.
pub fn compose(catalog: &Catalog, entry: &Entry) -> Composed {
    // The theme prints the page title; a leading `# Title` that says the same
    // thing would show it twice, so it is left out of the body.
    let drop_h1 = entry
        .headings
        .first()
        .is_some_and(|h| h.level == 1 && h.text == entry.title);
    let mut state = State {
        catalog,
        entry,
        lines: Lines::new(&entry.body),
        report: Report::new(),
        links_out: Vec::new(),
        headings: entry.headings.iter(),
        drop_h1,
        plain: String::new(),
        embed_origins: Vec::new(),
    };
    let parsed = parse_blocks(&entry.body);
    for problem in &parsed.errors {
        let d = Diagnostic::error(Code::DIRECTIVE, problem.message.clone()).with_help(
            "directives are written `:::name attributes` ... `:::`; see the manual for the list",
        );
        let located = state.located(d, problem.offset);
        state.report.push(located);
    }
    let out = render_nodes(&mut state, &parsed.nodes);
    state.links_out.sort();
    state.links_out.dedup();
    state.embed_origins.sort();
    state.embed_origins.dedup();
    Composed {
        html: out,
        plain: state.plain.trim().to_owned(),
        links_out: state.links_out,
        embed_origins: state.embed_origins,
        report: state.report,
    }
}

const PRE_OPEN: &str = "<pre tabindex=\"0\"><code>";
const PRE_CLOSE: &str = "</code></pre>\n";

/// Renders one piece of Markdown that starts at byte `base` of the page body.
#[allow(
    clippy::too_many_lines,
    reason = "one event loop; splitting it would scatter its state"
)]
pub(crate) fn render_markdown(state: &mut State<'_>, text: &str, base: usize) -> String {
    let mut events: Vec<Event<'_>> = Vec::new();
    let mut buffer = String::new();
    let mut buffer_start = 0usize;
    let mut code_depth = 0usize;
    let mut link_depth = 0usize;
    let mut dropping = false;
    // A fenced block with a language is collected whole, then highlighted at its end.
    let mut capture: Option<(String, String)> = None;

    for (event, range) in Parser::new_ext(text, options()).into_offset_iter() {
        let at = base + range.start;
        if dropping {
            match &event {
                Event::End(TagEnd::Heading(_)) => {
                    dropping = false;
                    state.plain.push(' ');
                }
                Event::Text(t) | Event::Code(t) => state.plain.push_str(t),
                _ => {}
            }
            continue;
        }
        if state.drop_h1
            && events.is_empty()
            && buffer.is_empty()
            && matches!(
                event,
                Event::Start(Tag::Heading {
                    level: HeadingLevel::H1,
                    ..
                })
            )
        {
            state.drop_h1 = false;
            dropping = true;
            state.headings.next();
            continue;
        }
        if let (Some((_, code)), Event::Text(t)) = (capture.as_mut(), &event) {
            state.plain.push_str(t);
            code.push_str(t);
            continue;
        }
        let scannable = code_depth == 0 && link_depth == 0;
        if let Event::Text(t) = &event {
            state.plain.push_str(t);
            if scannable {
                if buffer.is_empty() {
                    buffer_start = at;
                }
                buffer.push_str(t);
                continue;
            }
        }
        flush_text(state, &mut buffer, buffer_start, &mut events);

        match event {
            Event::Text(t) => events.push(Event::Text(t)),
            Event::Code(code) => {
                state.plain.push_str(&code);
                events.push(Event::Code(code));
            }
            Event::Html(raw) | Event::InlineHtml(raw) => {
                let d = Diagnostic::error(Code::RAW_HTML, "raw HTML is not allowed in content")
                    .with_help(
                        "use Markdown; HTML in pages is refused so a page cannot run scripts",
                    );
                let located = state.located(d, at);
                state.report.push(located);
                events.push(Event::Text(raw));
            }
            Event::SoftBreak | Event::HardBreak => {
                state.plain.push(' ');
                events.push(event);
            }
            Event::Start(Tag::Heading {
                level,
                classes,
                attrs,
                ..
            }) => {
                let anchor = state.headings.next().map(|h| h.anchor.clone());
                events.push(Event::Start(Tag::Heading {
                    level,
                    id: anchor.map(CowStr::from),
                    classes,
                    attrs,
                }));
            }
            Event::End(end @ (TagEnd::Heading(_) | TagEnd::Paragraph)) => {
                state.plain.push(' ');
                events.push(Event::End(end));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                code_depth += 1;
                let language = match &kind {
                    CodeBlockKind::Fenced(info) => language_of(info).map(str::to_owned),
                    CodeBlockKind::Indented => None,
                };
                if let Some(language) = language {
                    capture = Some((language, String::new()));
                } else {
                    events.push(Event::Html(PRE_OPEN.into()));
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                code_depth = code_depth.saturating_sub(1);
                state.plain.push(' ');
                if let Some((language, code)) = capture.take() {
                    let class = escape(&language);
                    let body = highlight(&language, &code).unwrap_or_else(|| escape(&code));
                    events.push(Event::Html(
                        format!(
                            "<pre tabindex=\"0\"><code class=\"language-{class}\">{body}</code></pre>\n"
                        )
                        .into(),
                    ));
                } else {
                    events.push(Event::Html(PRE_CLOSE.into()));
                }
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                link_depth += 1;
                let dest = state.target(&dest_url, at);
                events.push(Event::Start(Tag::Link {
                    link_type,
                    dest_url: dest.into(),
                    title,
                    id,
                }));
            }
            Event::End(TagEnd::Link) => {
                link_depth = link_depth.saturating_sub(1);
                events.push(Event::End(TagEnd::Link));
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                link_depth += 1;
                let dest = state.target(&dest_url, at);
                events.push(Event::Start(Tag::Image {
                    link_type,
                    dest_url: dest.into(),
                    title,
                    id,
                }));
            }
            Event::End(TagEnd::Image) => {
                link_depth = link_depth.saturating_sub(1);
                events.push(Event::End(TagEnd::Image));
            }
            Event::Start(Tag::Table(alignments)) => {
                events.push(Event::Html("<div class=\"table-wrap\">".into()));
                events.push(Event::Start(Tag::Table(alignments)));
            }
            Event::End(TagEnd::Table) => {
                events.push(Event::End(TagEnd::Table));
                events.push(Event::Html("</div>\n".into()));
            }
            other => events.push(other),
        }
    }
    flush_text(state, &mut buffer, buffer_start, &mut events);
    state.drop_h1 = false;

    let mut out = String::with_capacity(text.len() * 2);
    html::push_html(&mut out, events.into_iter());
    out
}

/// Emits the buffered text, turning `[[term]]` references into links.
fn flush_text(
    state: &mut State<'_>,
    buffer: &mut String,
    start: usize,
    events: &mut Vec<Event<'_>>,
) {
    if buffer.is_empty() {
        return;
    }
    let text = std::mem::take(buffer);
    let refs = find_term_refs(&text);
    if refs.is_empty() {
        events.push(Event::Text(text.into()));
        return;
    }
    let mut cursor = 0;
    for reference in refs {
        if reference.start > cursor {
            events.push(Event::Text(text[cursor..reference.start].to_owned().into()));
        }
        match state.catalog.resolve_term(state.entry.id, reference.target) {
            Ok(target) => {
                state.links_out.push(
                    target
                        .url
                        .split('#')
                        .next()
                        .unwrap_or(&target.url)
                        .to_owned(),
                );
                let label = reference.label.unwrap_or(&target.text);
                events.push(Event::Html(
                    format!(
                        "<a class=\"term\" href=\"{}\">{}</a>",
                        escape(&target.url),
                        escape(label)
                    )
                    .into(),
                ));
            }
            Err(diagnostic) => {
                let located = state.located(diagnostic, start + reference.start);
                state.report.push(located);
                events.push(Event::Text(
                    text[reference.start..reference.end].to_owned().into(),
                ));
            }
        }
        cursor = reference.end;
    }
    if cursor < text.len() {
        events.push(Event::Text(text[cursor..].to_owned().into()));
    }
}

pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}
