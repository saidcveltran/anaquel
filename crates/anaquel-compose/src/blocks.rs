//! Turns directive blocks into HTML. The markup is fixed here; content only fills its slots,
//! and everything it supplies is escaped.

use anaquel_diagnostics::{Code, Diagnostic};
use anaquel_directive::{Block, Node};

use crate::markdown::{State, escape, render_markdown};

/// Renders a list of Markdown pieces and directive blocks.
pub(crate) fn render_nodes(state: &mut State<'_>, nodes: &[Node<'_>]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Markdown { start, text } => out.push_str(&render_markdown(state, text, *start)),
            Node::Block(block) => {
                state.drop_h1 = false;
                out.push_str(&render_block(state, block));
            }
        }
    }
    out
}

fn title_of(state: &mut State<'_>, block: &Block<'_>) -> String {
    let title = block.attrs.get("title").unwrap_or_default();
    state.plain.push_str(title);
    state.plain.push(' ');
    escape(title)
}

fn render_block(state: &mut State<'_>, block: &Block<'_>) -> String {
    match block.name {
        "hint" => {
            let kind = ["info", "success", "warning", "danger"]
                .into_iter()
                .find(|k| block.attrs.has_flag(k))
                .unwrap_or("info");
            let inner = render_nodes(state, &block.children);
            format!("<div class=\"hint hint-{kind}\" role=\"note\">\n{inner}</div>\n")
        }
        "details" => {
            let title = title_of(state, block);
            let open = if block.attrs.has_flag("open") {
                " open"
            } else {
                ""
            };
            let inner = render_nodes(state, &block.children);
            format!(
                "<details class=\"details\"{open}>\n<summary>{title}</summary>\n{inner}</details>\n"
            )
        }
        "tabs" => {
            let sync = block
                .attrs
                .get("sync")
                .map(|s| format!(" data-sync=\"{}\"", escape(s)))
                .unwrap_or_default();
            let inner = render_nodes(state, &block.children);
            format!("<div class=\"tabs\"{sync}>\n{inner}</div>\n")
        }
        "tab" => {
            let title = title_of(state, block);
            let inner = render_nodes(state, &block.children);
            format!(
                "<section class=\"tab\" data-title=\"{title}\">\n<div class=\"tab-title\">{title}</div>\n{inner}</section>\n"
            )
        }
        "cards" => {
            let cols = block
                .attrs
                .get("cols")
                .map(|c| format!(" cols-{}", escape(c)))
                .unwrap_or_default();
            let inner = render_nodes(state, &block.children);
            format!("<div class=\"card-grid{cols}\">\n{inner}</div>\n")
        }
        "card" => card(state, block),
        "steps" => {
            let inner = render_nodes(state, &block.children);
            format!("<ol class=\"steps\">\n{inner}</ol>\n")
        }
        "step" => {
            let title = title_of(state, block);
            let inner = render_nodes(state, &block.children);
            format!("<li class=\"step\">\n<div class=\"step-title\">{title}</div>\n{inner}</li>\n")
        }
        "columns" => {
            let inner = render_nodes(state, &block.children);
            format!("<div class=\"columns\">\n{inner}</div>\n")
        }
        "column" => {
            let width = block
                .attrs
                .get("width")
                .map(|w| format!(" col-w-{}", escape(w)))
                .unwrap_or_default();
            let inner = render_nodes(state, &block.children);
            format!("<div class=\"column{width}\">\n{inner}</div>\n")
        }
        "embed" => embed(state, block),
        _ => String::new(),
    }
}

fn card(state: &mut State<'_>, block: &Block<'_>) -> String {
    let title = title_of(state, block);
    let heading = match block.attrs.get("href") {
        Some(href) => {
            let url = state.target(href, block.start);
            format!(
                "<a class=\"card-title\" href=\"{}\">{title}</a>",
                escape(&url)
            )
        }
        None => format!("<span class=\"card-title\">{title}</span>"),
    };
    let image = match block.attrs.get("image") {
        Some(src) => {
            let url = state.target(src, block.start);
            format!(
                "<img class=\"card-image\" src=\"{}\" alt=\"\" loading=\"lazy\">\n",
                escape(&url)
            )
        }
        None => String::new(),
    };
    let inner = render_nodes(state, &block.children);
    format!(
        "<div class=\"card\">\n{image}<div class=\"card-body\">\n{heading}\n{inner}</div>\n</div>\n"
    )
}

fn embed(state: &mut State<'_>, block: &Block<'_>) -> String {
    let url = block.attrs.get("url").unwrap_or_default();
    if let Some(src) = embed_src(url) {
        if let Some(origin) = origin_of(&src) {
            state.embed_origins.push(origin);
        }
        format!(
            "<div class=\"embed\"><iframe src=\"{}\" title=\"Embedded video\" loading=\"lazy\" \
sandbox=\"allow-scripts allow-same-origin allow-presentation\" allowfullscreen \
referrerpolicy=\"strict-origin-when-cross-origin\"></iframe></div>\n",
            escape(&src)
        )
    } else {
        let d = Diagnostic::error(Code::FORBIDDEN_URL, "this address cannot be embedded")
            .with_help(
                "embeddable: YouTube (youtube.com, youtu.be, youtube-nocookie.com) and Vimeo, over https",
            );
        let located = state.located(d, block.start);
        state.report.push(located);
        String::new()
    }
}

/// `https://host` of an address that starts with `https://`.
fn origin_of(src: &str) -> Option<String> {
    let rest = src.strip_prefix("https://")?;
    let host = rest.split('/').next()?;
    Some(format!("https://{host}"))
}

fn video_id(id: &str) -> bool {
    (6..=20).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn vimeo_id(id: &str) -> bool {
    (1..=12).contains(&id.len()) && id.chars().all(|c| c.is_ascii_digit())
}

/// The address an iframe may load for an accepted video link, or `None`.
///
/// The host must be exactly one of the listed ones, over `https`, with no user name or port,
/// and the id must have the shape of that provider's ids.
pub(crate) fn embed_src(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let (host, tail) = rest.split_once('/').unwrap_or((rest, ""));
    let (path, query) = match tail.split_once('?') {
        Some((p, q)) => (p, q.split('#').next().unwrap_or("")),
        None => (tail.split('#').next().unwrap_or(""), ""),
    };
    let path = path.split('#').next().unwrap_or("").trim_end_matches('/');
    let youtube =
        |id: &str| video_id(id).then(|| format!("https://www.youtube-nocookie.com/embed/{id}"));
    let vimeo = |id: &str| vimeo_id(id).then(|| format!("https://player.vimeo.com/video/{id}"));
    match host {
        "www.youtube.com" | "youtube.com" => {
            if path == "watch" {
                let id = query.split('&').find_map(|kv| kv.strip_prefix("v="))?;
                youtube(id)
            } else {
                youtube(path.strip_prefix("embed/")?)
            }
        }
        "www.youtube-nocookie.com" => youtube(path.strip_prefix("embed/")?),
        "youtu.be" => youtube(path),
        "vimeo.com" => vimeo(path),
        "player.vimeo.com" => vimeo(path.strip_prefix("video/")?),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_listed_providers() {
        for (url, want) in [
            (
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ",
            ),
            (
                "https://youtu.be/dQw4w9WgXcQ",
                "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ",
            ),
            (
                "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ",
                "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ",
            ),
            (
                "https://vimeo.com/76979871",
                "https://player.vimeo.com/video/76979871",
            ),
            (
                "https://player.vimeo.com/video/76979871?h=abc",
                "https://player.vimeo.com/video/76979871",
            ),
        ] {
            assert_eq!(embed_src(url).as_deref(), Some(want), "{url}");
        }
    }

    #[test]
    fn refuses_everything_else() {
        for url in [
            "http://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://evil.example/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com.evil.example/embed/dQw4w9WgXcQ",
            // A user name before the host (written with an escape so no address-like text appears).
            "https://user\u{40}www.youtube.com/embed/dQw4w9WgXcQ",
            "https://www.youtube.com:444/embed/dQw4w9WgXcQ",
            "https://www.youtube.com/embed/x",
            "https://www.youtube.com/embed/a\"onload=\"x",
            "https://vimeo.com/abc",
            "javascript:alert(1)",
            "",
        ] {
            assert_eq!(embed_src(url), None, "{url}");
        }
    }
}
