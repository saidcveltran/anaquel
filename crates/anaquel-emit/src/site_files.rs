use std::fmt::Write as _;

/// `sitemap.xml` for the given site URLs.
///
/// `base_url` must be absolute and without a trailing slash.
pub fn sitemap(base_url: &str, urls: &[String]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for url in urls {
        let _ = writeln!(
            out,
            "  <url><loc>{}{}</loc></url>",
            xml_escape(base_url),
            xml_escape(url)
        );
    }
    out.push_str("</urlset>\n");
    out
}

/// One link of `llms.txt`.
#[derive(Debug, Clone)]
pub struct LlmsLink {
    /// Page title.
    pub title: String,
    /// Site URL of the page, starting with `/`.
    pub url: String,
    /// One sentence, when the page has one.
    pub description: Option<String>,
}

/// One `##` section of `llms.txt`: a volume and its pages in reading order.
#[derive(Debug, Clone)]
pub struct LlmsSection {
    /// Heading of the section.
    pub title: String,
    /// The pages of the volume.
    pub links: Vec<LlmsLink>,
}

/// `llms.txt`: a plain Markdown index of the library for programs that read it.
///
/// `base_url`, when there is one, makes every link absolute; it has no trailing slash.
pub fn llms_txt(
    title: &str,
    description: Option<&str>,
    base_url: Option<&str>,
    sections: &[LlmsSection],
) -> String {
    let mut out = format!("# {}\n", one_line(title));
    if let Some(description) = description.map(one_line).filter(|d| !d.is_empty()) {
        let _ = write!(out, "\n> {description}\n");
    }
    let base = base_url.unwrap_or("");
    for section in sections.iter().filter(|s| !s.links.is_empty()) {
        let _ = write!(out, "\n## {}\n\n", one_line(&section.title));
        for link in &section.links {
            let text = one_line(&link.title)
                .replace('[', "\\[")
                .replace(']', "\\]");
            let target = format!("{base}{}", link.url)
                .replace(' ', "%20")
                .replace(')', "%29");
            match link.description.as_deref().map(one_line) {
                Some(d) if !d.is_empty() => {
                    let _ = writeln!(out, "- [{text}]({target}): {d}");
                }
                _ => {
                    let _ = writeln!(out, "- [{text}]({target})");
                }
            }
        }
    }
    out
}

/// Text on one line, so a title cannot start a new heading or list item.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Cloudflare `_redirects`: one permanent redirect per line.
pub fn redirects_file(redirects: &[(String, String)]) -> String {
    let mut out = String::new();
    for (from, to) in redirects {
        let _ = writeln!(out, "{from} {to} 301");
    }
    out
}

/// Cloudflare `_headers`. The policy forbids inline scripts and styles, so the
/// theme can never be made to run text from a page. Frames are refused unless a page embeds a
/// video: then only the origins in `frame_origins` may be framed.
pub fn headers_file(search: bool, frame_origins: &[String]) -> String {
    let script = if search {
        "script-src 'self' 'wasm-unsafe-eval'"
    } else {
        "script-src 'self'"
    };
    let frames = if frame_origins.is_empty() {
        "'none'".to_owned()
    } else {
        frame_origins.join(" ")
    };
    format!(
        "/*\n  Content-Security-Policy: default-src 'self'; {script}; style-src 'self'; img-src 'self' https: data:; \
         font-src 'self'; connect-src 'self'; frame-src {frames}; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\n  \
         X-Content-Type-Options: nosniff\n  Referrer-Policy: strict-origin-when-cross-origin\n\
         /assets/*\n  Cache-Control: public, max-age=31536000, immutable\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sitemap_lists_absolute_urls() {
        let xml = sitemap(
            "https://docs.example",
            &["/a/".to_owned(), "/b&c/".to_owned()],
        );
        assert!(xml.contains("<loc>https://docs.example/a/</loc>"));
        assert!(xml.contains("<loc>https://docs.example/b&amp;c/</loc>"));
    }

    #[test]
    fn llms_txt_lists_pages_by_volume() {
        let sections = [
            LlmsSection {
                title: "Guide".to_owned(),
                links: vec![
                    LlmsLink {
                        title: "Setup [1]\n# not a heading".to_owned(),
                        url: "/guide/setup/".to_owned(),
                        description: Some("How to\ninstall".to_owned()),
                    },
                    LlmsLink {
                        title: "Plain".to_owned(),
                        url: "/guide/a b)/".to_owned(),
                        description: None,
                    },
                ],
            },
            LlmsSection {
                title: "Empty".to_owned(),
                links: vec![],
            },
        ];
        let text = llms_txt(
            "Lib",
            Some("About\nit"),
            Some("https://docs.example"),
            &sections,
        );
        assert_eq!(
            text,
            "# Lib\n\n> About it\n\n## Guide\n\n\
             - [Setup \\[1\\] # not a heading](https://docs.example/guide/setup/): How to install\n\
             - [Plain](https://docs.example/guide/a%20b%29/)\n"
        );
        let relative = llms_txt("Lib", None, None, &sections);
        assert!(relative.contains("](/guide/setup/)"), "{relative}");
        assert!(!relative.contains("Empty"));
    }

    #[test]
    fn redirects_are_permanent() {
        let text = redirects_file(&[("/old/".to_owned(), "/new/".to_owned())]);
        assert_eq!(text, "/old/ /new/ 301\n");
    }

    #[test]
    fn headers_forbid_inline_scripts() {
        assert!(headers_file(false, &[]).contains("script-src 'self';"));
        assert!(headers_file(true, &[]).contains("'wasm-unsafe-eval'"));
        assert!(!headers_file(true, &[]).contains("unsafe-inline"));
    }

    #[test]
    fn frames_are_refused_unless_a_page_embeds() {
        assert!(headers_file(false, &[]).contains("frame-src 'none';"));
        let origins = ["https://player.vimeo.com".to_owned()];
        let text = headers_file(false, &origins);
        assert!(
            text.contains("frame-src https://player.vimeo.com;"),
            "{text}"
        );
    }
}
