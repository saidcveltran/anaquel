use anaquel_catalog::{Catalog, LogicalPath, PageSource, VolumeSource};
use anaquel_diagnostics::Code;
use anaquel_schema::{LibraryManifest, VolumeManifest};

use super::*;

fn compose_page(page: &str) -> Composed {
    let library =
        LibraryManifest::parse("schema = 1\nengine = \"0.1\"\ntitle = \"T\"\n", "0.1.0").unwrap();
    let pages = [
        ("_index.md", "# Home\n"),
        ("page.md", page),
        ("other.md", "# Other\n\n## Part\n"),
        ("terms.md", "# Terms\n\n### Cache\n\nStores copies.\n"),
    ];
    let volume = VolumeSource {
        slug: "v".to_owned(),
        manifest: VolumeManifest::parse("title = \"V\"\n", "volume.toml").unwrap(),
        pages: pages
            .iter()
            .map(|(p, t)| PageSource {
                path: LogicalPath::new(p).unwrap(),
                text: (*t).to_owned(),
            })
            .collect(),
        files: vec![LogicalPath::new("pic.png").unwrap()],
    };
    let (catalog, _) = Catalog::build(library, vec![volume], &headings_of);
    let entry = catalog
        .entries()
        .iter()
        .find(|e| e.rel.as_str() == "page.md")
        .unwrap();
    compose(&catalog, entry)
}

fn codes(c: &Composed) -> Vec<Code> {
    c.report.items().iter().map(|d| d.code).collect()
}

#[test]
fn renders_basic_markdown_with_heading_ids() {
    let c = compose_page("# Title\n\nSome *text* and `code`.\n\n## Second part\n");
    assert!(
        !c.html.contains("<h1"),
        "the theme prints the title: {}",
        c.html
    );
    assert!(c.html.starts_with("<p>Some"), "{}", c.html);
    assert!(c.html.contains("<h2 id=\"second-part\">"), "{}", c.html);
    assert!(c.html.contains("<em>text</em>"));
    assert_eq!(c.plain, "Title Some text and code. Second part");
    assert_eq!(c.report.items().len(), 0);
}

#[test]
fn internal_links_are_rewritten_to_site_urls() {
    let c = compose_page("[go](other.md#part) and ![p](pic.png)\n");
    assert!(c.html.contains("href=\"/v/other/#part\""), "{}", c.html);
    assert!(c.html.contains("src=\"/v/pic.png\""), "{}", c.html);
    assert_eq!(c.links_out, ["/v/other/", "/v/pic.png"]);
}

#[test]
fn broken_and_dangerous_links_are_reported_with_their_line() {
    let c = compose_page("text\n\n[a](nope.md) [b](javascript:alert(1))\n");
    assert_eq!(codes(&c), [Code::BROKEN_LINK, Code::FORBIDDEN_URL]);
    assert!(c.report.items().iter().all(|d| d.line == Some(3)));
    assert!(!c.html.contains("javascript"), "{}", c.html);
}

#[test]
fn raw_html_is_refused_and_shown_as_text() {
    let c = compose_page("<script>alert(1)</script>\n\nA <b>bold</b> word\n");
    assert!(codes(&c).iter().all(|code| *code == Code::RAW_HTML));
    assert_ne!(codes(&c).len(), 0);
    assert!(!c.html.contains("<script>"), "{}", c.html);
    assert!(!c.html.contains("<b>"), "{}", c.html);
}

#[test]
fn terms_become_links_and_unknown_terms_are_reported() {
    let c = compose_page("Use the [[cache]] or [[Cache|a store]] or [[nothing]].\n");
    assert!(
        c.html
            .contains("<a class=\"term\" href=\"/v/terms/#cache\">cache</a>")
            || c.html.contains(">Cache</a>"),
        "{}",
        c.html
    );
    assert!(c.html.contains(">a store</a>"), "{}", c.html);
    assert!(c.html.contains("[[nothing]]"));
    assert_eq!(codes(&c), [Code::UNKNOWN_TERM]);
}

#[test]
fn terms_are_left_alone_in_code_and_in_links() {
    let c = compose_page("`[[cache]]`\n\n```\n[[cache]]\n```\n\n[see [[cache]]](other.md)\n");
    assert!(!c.html.contains("class=\"term\""), "{}", c.html);
    assert_eq!(c.report.items().len(), 0, "{:?}", c.report.items());
}

#[test]
fn tables_are_wrapped_for_scrolling() {
    let c = compose_page("| a | b |\n|---|---|\n| 1 | 2 |\n");
    assert!(
        c.html.starts_with("<div class=\"table-wrap\"><table>"),
        "{}",
        c.html
    );
    assert!(
        c.html.trim_end().ends_with("</table></div>") || c.html.contains("</table>\n</div>"),
        "{}",
        c.html
    );
}

#[test]
fn only_a_leading_h1_that_repeats_the_title_is_dropped() {
    let c = compose_page("---\ntitle: Other name\n---\n# Title\n\ntext\n");
    assert!(
        c.html.contains("<h1"),
        "kept, it says something else: {}",
        c.html
    );
    let c = compose_page("text first\n\n# Title\n");
    assert!(
        c.html.contains("<h1"),
        "kept, not the first block: {}",
        c.html
    );
}
