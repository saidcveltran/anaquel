//! What the fuzz targets run, as plain functions.
//!
//! Each function takes the bytes a fuzzer made, turns them into text (invalid UTF-8 becomes
//! U+FFFD, so every input reaches the parser) and checks the promises of the code under test. A
//! failed `assert!` is a bug that the fuzzer reports. The same functions run in `tests/smoke.rs`
//! over the seed files and thousands of mutations of them, so the checks run on every system, not
//! only where `cargo fuzz` works. The targets themselves are in the `fuzz` folder of the
//! repository, which is not part of this workspace.

use anaquel_catalog::{
    Catalog, Entry, EntryId, Heading, LogicalPath, PageSource, ResolvedLink, VolumeSource,
};
use anaquel_compose::{compose, headings_of};
use anaquel_directive::{MAX_BLOCKS, MAX_DEPTH, Node, parse_blocks};
use anaquel_schema::{LibraryManifest, VolumeManifest, parse_page, url_segment_problem};

/// The directive splitter: never panics, keeps its limits, and points inside the text.
///
/// # Panics
/// When a promise of the code under test is broken; that is how a fuzzer sees a bug.
pub fn directive_grammar(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let parsed = parse_blocks(&text);
    let mut blocks = 0;
    walk(&parsed.nodes, 1, text.len(), &mut blocks);
    assert!(blocks <= MAX_BLOCKS, "{blocks} blocks");
    for error in &parsed.errors {
        assert!(error.offset <= text.len(), "error offset {}", error.offset);
    }
}

fn walk(nodes: &[Node<'_>], depth: usize, length: usize, blocks: &mut usize) {
    assert!(depth <= MAX_DEPTH, "depth {depth}");
    for node in nodes {
        match node {
            Node::Markdown { start, text } => {
                assert!(start + text.len() <= length, "markdown past the end");
            }
            Node::Block(block) => {
                *blocks += 1;
                assert!(block.start <= length, "block past the end");
                assert!(block.colons >= 3, "{} colons", block.colons);
                walk(&block.children, depth + 1, length, blocks);
            }
        }
    }
}

/// The front matter reader: never panics, and what it accepts is safe to use in an address.
///
/// # Panics
/// When a promise of the code under test is broken; that is how a fuzzer sees a bug.
pub fn front_matter(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let Ok(parsed) = parse_page(&text, "volumes/v/page.md") else {
        return;
    };
    assert!(parsed.body_line >= 1);
    assert!(parsed.body.len() <= text.len());
    if let Some(slug) = &parsed.front.slug {
        assert_eq!(url_segment_problem(slug), None, "slug {slug:?}");
    }
}

fn library() -> LibraryManifest {
    match LibraryManifest::parse("schema = 1\nengine = \"0.1\"\ntitle = \"T\"\n", "0.1.0") {
        Ok(library) => library,
        Err(d) => unreachable!("fixture library: {}", d.message),
    }
}

fn logical(path: &str) -> LogicalPath {
    match LogicalPath::new(path) {
        Ok(path) => path,
        Err(why) => unreachable!("fixture path {path}: {why}"),
    }
}

fn volume(pages: &[(&str, &str)]) -> VolumeSource {
    let manifest = match VolumeManifest::parse("title = \"guide\"\n", "volume.toml") {
        Ok(manifest) => manifest,
        Err(d) => unreachable!("fixture volume: {}", d.message),
    };
    VolumeSource {
        slug: "guide".to_owned(),
        manifest,
        pages: pages
            .iter()
            .map(|(path, text)| PageSource {
                path: logical(path),
                text: (*text).to_owned(),
            })
            .collect(),
        files: vec![logical("img/logo.png")],
    }
}

fn catalog(pages: &[(&str, &str)]) -> Catalog {
    let headings: &(dyn Fn(&str) -> Vec<Heading> + Sync) = &headings_of;
    Catalog::build(library(), vec![volume(pages)], headings).0
}

const FIXED: [(&str, &str); 3] = [
    ("_index.md", "# Guide\n"),
    ("setup.md", "# Setup\n\n## Install\n"),
    ("deep/page.md", "# Page\n"),
];

fn entry_of<'a>(catalog: &'a Catalog, source: &str) -> Option<&'a Entry> {
    catalog
        .entries()
        .iter()
        .find(|e| e.source.as_str() == source)
}

/// The whole page path: a page made of the input is composed, and what comes out cannot run
/// script, whatever the input said.
///
/// # Panics
/// When a promise of the code under test is broken; that is how a fuzzer sees a bug.
pub fn url_policy(data: &[u8]) {
    let text = String::from_utf8_lossy(data).into_owned();
    let mut pages = FIXED.to_vec();
    pages.push(("fuzzed.md", &text));
    let catalog = catalog(&pages);
    let Some(entry) = entry_of(&catalog, "volumes/guide/fuzzed.md") else {
        return;
    };
    let composed = compose(&catalog, entry);
    let html = composed.html.to_lowercase();
    for needle in [
        "<script", "<style", "<object", "<embed", "<base", "<form", "<svg",
    ] {
        assert!(
            !html.contains(needle),
            "`{needle}` in the output of {text:?}"
        );
    }
    if composed.embed_origins.is_empty() {
        assert!(
            !html.contains("<iframe"),
            "iframe without an embed in {text:?}"
        );
    }
    for attribute in ["href", "src", "action", "formaction", "poster"] {
        for quote in ['"', '\''] {
            for scheme in ["javascript:", "vbscript:", "data:", "file:"] {
                let bad = format!("{attribute}={quote}{scheme}");
                assert!(!html.contains(&bad), "`{bad}` in the output of {text:?}");
            }
        }
    }
    for origin in &composed.embed_origins {
        assert!(
            origin == "https://www.youtube-nocookie.com" || origin == "https://player.vimeo.com",
            "origin {origin}"
        );
    }
}

/// The link policy and resolver: never panics, and only http, https, mailto or a site URL come
/// out.
///
/// # Panics
/// When a promise of the code under test is broken; that is how a fuzzer sees a bug.
pub fn link_resolver(data: &[u8]) {
    let href = String::from_utf8_lossy(data);
    let catalog = catalog(&FIXED);
    let Some(from) = entry_of(&catalog, "volumes/guide/setup.md").map(|e| e.id) else {
        unreachable!("the fixture page is missing");
    };
    check_link(&catalog, from, &href);
    check_link(&catalog, from, &format!("{href}#install"));
}

fn check_link(catalog: &Catalog, from: EntryId, href: &str) {
    match catalog.resolve_link(from, href) {
        Ok(ResolvedLink::External(url)) => {
            let lower = url.to_lowercase();
            assert!(
                ["http:", "https:", "mailto:"]
                    .iter()
                    .any(|scheme| lower.starts_with(scheme)),
                "external {url:?} from {href:?}"
            );
        }
        Ok(ResolvedLink::Internal(url)) => {
            assert!(
                (url.starts_with('/') && !url.starts_with("//")) || url.starts_with('#'),
                "internal {url:?} from {href:?}"
            );
        }
        Err(_) => {}
    }
}
