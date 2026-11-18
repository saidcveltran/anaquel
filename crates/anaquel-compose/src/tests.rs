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

#[test]
fn a_hint_wraps_its_markdown() {
    let c = compose_page("before\n\n:::hint warning\nCareful with *this*.\n:::\n\nafter\n");
    assert_eq!(codes(&c), []);
    assert!(
        c.html
            .contains("<div class=\"hint hint-warning\" role=\"note\">\n<p>Careful with <em>this</em>.</p>\n</div>"),
        "{}",
        c.html
    );
    assert!(c.html.contains("<p>before</p>") && c.html.contains("<p>after</p>"));
    assert_eq!(c.plain, "before Careful with this. after");
}

#[test]
fn tabs_cards_steps_columns_and_details_render() {
    let page = "\
::::tabs sync=\"os\"
:::tab title=\"Linux\"
one
:::
:::tab title=\"Windows\"
two
:::
::::

::::cards cols=2
:::card title=\"Other\" href=\"other.md\"
text
:::
:::card title=\"Picture\" image=\"pic.png\"
more
:::
::::

::::steps
:::step title=\"First\"
do it
:::
::::

::::columns
:::column width=2
left
:::
:::column
right
:::
::::

:::details title=\"More\" open
hidden
:::
";
    let c = compose_page(page);
    assert_eq!(codes(&c), [], "{:?}", c.report.items());
    for want in [
        "<div class=\"tabs\" data-sync=\"os\">",
        "<section class=\"tab\" data-title=\"Linux\">",
        "<section class=\"tab\" data-title=\"Windows\">",
        "<div class=\"card-grid cols-2\">",
        "<a class=\"card-title\" href=\"/v/other/\">Other</a>",
        "<img class=\"card-image\" src=\"/v/pic.png\"",
        "<ol class=\"steps\">",
        "<li class=\"step\">",
        "<div class=\"column col-w-2\">",
        "<div class=\"column\">",
        "<details class=\"details\" open>",
        "<summary>More</summary>",
    ] {
        assert!(c.html.contains(want), "missing {want}: {}", c.html);
    }
    assert_eq!(c.links_out, ["/v/other/", "/v/pic.png"]);
}

#[test]
fn directive_problems_are_reported_with_their_line() {
    let c = compose_page("text\n\n:::nope\nx\n:::\n\n:::hint\nnever closed\n");
    assert_eq!(codes(&c), [Code::DIRECTIVE, Code::DIRECTIVE]);
    let lines: Vec<_> = c.report.items().iter().map(|d| d.line).collect();
    assert_eq!(lines, [Some(3), Some(7)]);
}

#[test]
fn titles_are_escaped_and_cannot_break_out() {
    let c = compose_page(":::details title=\"<script>alert(1)</script> & \\\"\"\nx\n:::\n");
    assert!(!c.html.contains("<script>"), "{}", c.html);
    let c = compose_page("::::tabs\n:::tab title=\"a'b&c\"\nx\n:::\n::::\n");
    assert!(
        c.html.contains("data-title=\"a&#39;b&amp;c\""),
        "{}",
        c.html
    );
}

#[test]
fn raw_html_inside_a_block_is_still_refused() {
    let c = compose_page(":::hint\n<script>alert(1)</script>\n:::\n");
    assert!(codes(&c).contains(&Code::RAW_HTML));
    assert!(!c.html.contains("<script>"), "{}", c.html);
}

#[test]
fn links_and_terms_work_inside_blocks_with_the_right_line() {
    let c = compose_page("x\n\n:::hint\n[a](nope.md)\n\n[[nothing]]\n:::\n");
    assert_eq!(codes(&c), [Code::BROKEN_LINK, Code::UNKNOWN_TERM]);
    let lines: Vec<_> = c.report.items().iter().map(|d| d.line).collect();
    assert_eq!(lines, [Some(4), Some(6)]);
}

#[test]
fn headings_inside_blocks_get_their_anchors() {
    let c = compose_page("## One\n\n:::hint\n## Two\n:::\n\n## Three\n");
    assert!(c.html.contains("<h2 id=\"one\">"), "{}", c.html);
    assert!(c.html.contains("<h2 id=\"two\">"), "{}", c.html);
    assert!(c.html.contains("<h2 id=\"three\">"), "{}", c.html);
}

#[test]
fn a_title_h1_inside_a_block_is_not_dropped() {
    let c = compose_page(":::hint\n# Title\n:::\n");
    assert!(c.html.contains("<h1"), "{}", c.html);
}

#[test]
fn directive_lines_in_code_are_text() {
    let c = compose_page("```\n:::hint\nx\n```\n");
    assert_eq!(codes(&c), []);
    assert!(c.html.contains(":::hint"), "{}", c.html);
}

#[test]
fn embed_uses_a_fixed_sandboxed_iframe_or_is_refused() {
    let c = compose_page(":::embed url=\"https://youtu.be/dQw4w9WgXcQ\"\n:::\n");
    assert_eq!(codes(&c), []);
    assert!(
        c.html
            .contains("src=\"https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ\""),
        "{}",
        c.html
    );
    assert!(c.html.contains("sandbox=\""), "{}", c.html);
    let c = compose_page(":::embed url=\"https://evil.example/x\"\n:::\n");
    assert_eq!(codes(&c), [Code::FORBIDDEN_URL]);
    assert!(!c.html.contains("iframe"), "{}", c.html);
}

#[test]
fn embeds_report_the_origins_their_frames_need() {
    let c = compose_page(
        ":::embed url=\"https://youtu.be/dQw4w9WgXcQ\"\n:::\n\n:::embed url=\"https://vimeo.com/76979871\"\n:::\n\n:::embed url=\"https://youtu.be/dQw4w9WgXcQ\"\n:::\n",
    );
    assert_eq!(
        c.embed_origins,
        [
            "https://player.vimeo.com",
            "https://www.youtube-nocookie.com"
        ]
    );
    assert_eq!(compose_page("text\n").embed_origins, Vec::<String>::new());
}

#[test]
fn fenced_code_with_a_known_language_is_highlighted_with_classes() {
    let c = compose_page("```rust\nfn main() {}\n```\n");
    assert!(
        c.html
            .starts_with("<pre tabindex=\"0\"><code class=\"language-rust\"><span class=\"s-"),
        "{}",
        c.html
    );
    assert!(!c.html.contains("style="), "{}", c.html);
    assert_eq!(c.plain, "fn main() {}");
}

#[test]
fn code_with_an_unknown_language_or_none_stays_plain_and_escaped() {
    let c = compose_page("```nonsense\n<b>&\n```\n\n```\n<i>\n```\n\n    <u>\n");
    assert!(
        c.html.contains(
            "<pre tabindex=\"0\"><code class=\"language-nonsense\">&lt;b&gt;&amp;\n</code></pre>"
        ),
        "{}",
        c.html
    );
    assert!(
        c.html
            .contains("<pre tabindex=\"0\"><code>&lt;i&gt;\n</code></pre>"),
        "{}",
        c.html
    );
    assert!(
        c.html
            .contains("<pre tabindex=\"0\"><code>&lt;u&gt;\n</code></pre>"),
        "{}",
        c.html
    );
    assert_eq!(codes(&c), []);
}

#[test]
fn every_code_block_can_take_keyboard_focus() {
    // A block that is wider than a phone screen scrolls inside itself; without a tab stop a
    // keyboard user cannot reach its hidden end. Each kind of block gets exactly one.
    let c = compose_page(
        "```rust\nfn main() {}\n```\n\n```nonsense\nx\n```\n\n```\ny\n```\n\n    z\n\n:::hint\n```\nw\n```\n:::\n",
    );
    assert_eq!(c.html.matches("<pre").count(), 5, "{}", c.html);
    assert_eq!(
        c.html.matches("<pre tabindex=\"0\">").count(),
        5,
        "{}",
        c.html
    );
    assert_eq!(c.html.matches("</pre>").count(), 5, "{}", c.html);
}

#[test]
fn highlighted_html_code_cannot_inject_markup() {
    let c = compose_page("```html\n<script>alert(1)</script><img src=x onerror=alert(1)>\n```\n");
    assert!(!c.html.contains("<script>"), "{}", c.html);
    assert!(!c.html.contains("<img"), "{}", c.html);
    assert_eq!(codes(&c), []);
}

#[test]
fn a_bad_language_name_cannot_break_out_of_the_class() {
    let c = compose_page("```a\"onmouseover=\"x\ncode\n```\n");
    assert!(!c.html.contains("onmouseover=\"x\""), "{}", c.html);
}

#[test]
fn code_blocks_work_inside_directives() {
    let c = compose_page(":::hint\n```rust\nlet x = 1;\n```\n:::\n");
    assert!(c.html.contains("language-rust"), "{}", c.html);
    assert_eq!(codes(&c), []);
}
