use anaquel_catalog::{Catalog, Entry, ResolvedLink};
use anaquel_diagnostics::{Code, Diagnostic, Report};
use anaquel_directive::find_term_refs;
use pulldown_cmark::{CowStr, Event, Parser, Tag, TagEnd, html};

use crate::heading::{Lines, options};

/// A page after composition.
#[derive(Debug)]
pub struct Composed {
    /// HTML of the body, without the page title.
    pub html: String,
    /// Text of the body with no markup, for search and descriptions.
    pub plain: String,
    /// Site URLs of the pages and files this page links to, without anchors.
    pub links_out: Vec<String>,
    /// Problems found while composing.
    pub report: Report,
}

struct State<'a> {
    catalog: &'a Catalog,
    entry: &'a Entry,
    lines: Lines,
    report: Report,
    links_out: Vec<String>,
}

impl State<'_> {
    fn located(&self, diagnostic: Diagnostic, offset: usize) -> Diagnostic {
        diagnostic
            .at(self.entry.source.as_str())
            .on_line(self.entry.body_line - 1 + self.lines.at(offset))
    }

    /// Resolves a link target; on failure reports it and returns `#`.
    fn target(&mut self, href: &str, offset: usize) -> String {
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
#[allow(
    clippy::too_many_lines,
    reason = "one event loop; splitting it would scatter its state"
)]
pub fn compose(catalog: &Catalog, entry: &Entry) -> Composed {
    let mut state = State {
        catalog,
        entry,
        lines: Lines::new(&entry.body),
        report: Report::new(),
        links_out: Vec::new(),
    };
    let mut headings = entry.headings.iter();
    let mut events: Vec<Event<'_>> = Vec::new();
    let mut plain = String::new();
    let mut buffer = String::new();
    let mut buffer_start = 0usize;
    let mut code_depth = 0usize;
    let mut link_depth = 0usize;

    for (event, range) in Parser::new_ext(&entry.body, options()).into_offset_iter() {
        let scannable = code_depth == 0 && link_depth == 0;
        if let Event::Text(text) = &event {
            plain.push_str(text);
            if scannable {
                if buffer.is_empty() {
                    buffer_start = range.start;
                }
                buffer.push_str(text);
                continue;
            }
        }
        flush_text(&mut state, &mut buffer, buffer_start, &mut events);

        match event {
            Event::Text(text) => events.push(Event::Text(text)),
            Event::Code(code) => {
                plain.push_str(&code);
                events.push(Event::Code(code));
            }
            Event::Html(raw) | Event::InlineHtml(raw) => {
                let d = Diagnostic::error(Code::RAW_HTML, "raw HTML is not allowed in content")
                    .with_help(
                        "use Markdown; HTML in pages is refused so a page cannot run scripts",
                    );
                let located = state.located(d, range.start);
                state.report.push(located);
                events.push(Event::Text(raw));
            }
            Event::SoftBreak | Event::HardBreak => {
                plain.push(' ');
                events.push(event);
            }
            Event::Start(Tag::Heading {
                level,
                classes,
                attrs,
                ..
            }) => {
                let anchor = headings.next().map(|h| h.anchor.clone());
                events.push(Event::Start(Tag::Heading {
                    level,
                    id: anchor.map(CowStr::from),
                    classes,
                    attrs,
                }));
            }
            Event::End(end @ (TagEnd::Heading(_) | TagEnd::Paragraph)) => {
                plain.push(' ');
                events.push(Event::End(end));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                code_depth += 1;
                events.push(Event::Start(Tag::CodeBlock(kind)));
            }
            Event::End(TagEnd::CodeBlock) => {
                code_depth = code_depth.saturating_sub(1);
                plain.push(' ');
                events.push(Event::End(TagEnd::CodeBlock));
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                link_depth += 1;
                let dest = state.target(&dest_url, range.start);
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
                let dest = state.target(&dest_url, range.start);
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
    flush_text(&mut state, &mut buffer, buffer_start, &mut events);

    let mut out = String::with_capacity(entry.body.len() * 2);
    html::push_html(&mut out, events.into_iter());
    state.links_out.sort();
    state.links_out.dedup();
    Composed {
        html: out,
        plain: plain.trim().to_owned(),
        links_out: state.links_out,
        report: state.report,
    }
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

fn escape(text: &str) -> String {
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
