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
/// theme can never be made to run text from a page.
pub fn headers_file(search: bool) -> String {
    let script = if search {
        "script-src 'self' 'wasm-unsafe-eval'"
    } else {
        "script-src 'self'"
    };
    format!(
        "/*\n  Content-Security-Policy: default-src 'self'; {script}; style-src 'self'; img-src 'self' https: data:; \
         font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\n  \
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
    fn redirects_are_permanent() {
        let text = redirects_file(&[("/old/".to_owned(), "/new/".to_owned())]);
        assert_eq!(text, "/old/ /new/ 301\n");
    }

    #[test]
    fn headers_forbid_inline_scripts() {
        assert!(headers_file(false).contains("script-src 'self';"));
        assert!(headers_file(true).contains("'wasm-unsafe-eval'"));
        assert!(!headers_file(true).contains("unsafe-inline"));
    }
}
