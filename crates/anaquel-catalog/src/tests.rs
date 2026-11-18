use anaquel_diagnostics::{Code, Report};
use anaquel_schema::{LibraryManifest, VolumeManifest};

use super::*;

fn library() -> LibraryManifest {
    LibraryManifest::parse("schema = 1\nengine = \"0.1\"\ntitle = \"T\"\n", "0.1.0").unwrap()
}

/// Test stand-in for the Markdown parser: one heading per line that starts with `#`.
fn headings(body: &str) -> Vec<Heading> {
    body.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let level = line.chars().take_while(|c| *c == '#').count();
            (level > 0 && line.chars().nth(level) == Some(' ')).then(|| {
                let text = line[level + 1..].trim().to_owned();
                Heading {
                    level: u8::try_from(level).unwrap(),
                    anchor: text.to_lowercase().replace(' ', "-"),
                    text,
                    line: u32::try_from(i + 1).unwrap(),
                }
            })
        })
        .collect()
}

fn volume(slug: &str, weight: i64, pages: &[(&str, &str)], files: &[&str]) -> VolumeSource {
    VolumeSource {
        slug: slug.to_owned(),
        shelf: None,
        manifest: VolumeManifest::parse(
            &format!("title = \"{slug}\"\nweight = {weight}\n"),
            "volume.toml",
        )
        .unwrap(),
        pages: pages
            .iter()
            .map(|(path, text)| PageSource {
                path: LogicalPath::new(path).unwrap(),
                text: (*text).to_owned(),
            })
            .collect(),
        files: files.iter().map(|f| LogicalPath::new(f).unwrap()).collect(),
    }
}

fn build(volumes: Vec<VolumeSource>) -> (Catalog, Report) {
    Catalog::build(library(), Vec::new(), volumes, &headings)
}

fn id_of(catalog: &Catalog, source: &str) -> EntryId {
    catalog
        .entries()
        .iter()
        .find(|e| e.source.as_str() == source)
        .unwrap()
        .id
}

fn codes(report: &Report) -> Vec<Code> {
    report.items().iter().map(|d| d.code).collect()
}

#[test]
fn urls_order_and_neighbours() {
    let (c, report) = build(vec![volume(
        "v",
        0,
        &[
            ("_index.md", "# Home\n"),
            ("a.md", "---\nweight: 2\n---\n# A\n"),
            ("b.md", "---\nweight: 1\n---\n# B\n"),
            ("sub/_index.md", "# Sub\n"),
            ("sub/c.md", "# C\n"),
        ],
        &[],
    )]);
    assert!(!report.has_errors(), "{:?}", report.items());
    let urls: Vec<_> = c.volumes()[0]
        .order
        .iter()
        .map(|id| c.entry(*id).unwrap().url.as_str())
        .collect();
    assert_eq!(urls, ["/v/", "/v/sub/", "/v/sub/c/", "/v/b/", "/v/a/"]);
    let b = c.entry(id_of(&c, "volumes/v/b.md")).unwrap();
    assert_eq!(c.entry(b.prev.unwrap()).unwrap().url, "/v/sub/c/");
    assert_eq!(c.entry(b.next.unwrap()).unwrap().url, "/v/a/");
    assert_eq!(
        c.entry(id_of(&c, "volumes/v/sub/c.md")).unwrap().parent,
        Some(id_of(&c, "volumes/v/sub/_index.md"))
    );
}

#[test]
fn title_falls_back_to_heading_then_file_name() {
    let (c, _) = build(vec![volume(
        "v",
        0,
        &[
            ("_index.md", "# Home\n"),
            ("x.md", "# From heading\n"),
            ("two-words.md", "text\n"),
            ("t.md", "---\ntitle: Front\n---\n# H\n"),
        ],
        &[],
    )]);
    let title = |p: &str| c.entry(id_of(&c, p)).unwrap().title.clone();
    assert_eq!(title("volumes/v/x.md"), "From heading");
    assert_eq!(title("volumes/v/two-words.md"), "Two words");
    assert_eq!(title("volumes/v/t.md"), "Front");
}

#[test]
fn volume_without_index_is_an_error() {
    let (_, report) = build(vec![volume("v", 0, &[("a.md", "# A\n")], &[])]);
    assert_eq!(codes(&report), [Code::MANIFEST]);
}

#[test]
fn folder_without_index_is_a_warning_and_children_attach_upwards() {
    let (c, report) = build(vec![volume(
        "v",
        0,
        &[("_index.md", "# H\n"), ("deep/x.md", "# X\n")],
        &[],
    )]);
    assert!(!report.has_errors());
    assert_eq!(report.items().len(), 1);
    assert_eq!(
        c.entry(id_of(&c, "volumes/v/deep/x.md")).unwrap().parent,
        Some(id_of(&c, "volumes/v/_index.md"))
    );
}

#[test]
fn duplicate_urls_are_reported() {
    let (_, report) = build(vec![volume(
        "v",
        0,
        &[
            ("_index.md", "# H\n"),
            ("a.md", "---\nslug: same\n---\n"),
            ("b.md", "---\nslug: same\n---\n"),
        ],
        &[],
    )]);
    assert_eq!(codes(&report), [Code::DUPLICATE_URL]);
}

#[test]
fn case_collisions_are_reported() {
    let (_, report) = build(vec![volume(
        "v",
        0,
        &[("_index.md", "# H\n"), ("A.md", "# A\n"), ("a.md", "# a\n")],
        &[],
    )]);
    assert!(codes(&report).contains(&Code::DUPLICATE_URL));
}

fn linked() -> Catalog {
    build(vec![
        volume(
            "v",
            0,
            &[
                ("_index.md", "# Home\n"),
                ("a.md", "# A\n## Part one\n"),
                ("sub/_index.md", "# Sub\n"),
                ("sub/c.md", "# C\n"),
                ("draft.md", "---\ndraft: true\n---\n# D\n"),
            ],
            &["img/logo.png"],
        ),
        volume("w", 1, &[("_index.md", "# W\n"), ("z.md", "# Z\n")], &[]),
    ])
    .0
}

#[test]
fn resolves_links() {
    let c = linked();
    let from = id_of(&c, "volumes/v/sub/c.md");
    let ok = |href: &str| c.resolve_link(from, href).unwrap();
    assert_eq!(ok("../a.md"), ResolvedLink::Internal("/v/a/".to_owned()));
    assert_eq!(
        ok("../a.md#part-one"),
        ResolvedLink::Internal("/v/a/#part-one".to_owned())
    );
    assert_eq!(
        ok("_index.md"),
        ResolvedLink::Internal("/v/sub/".to_owned())
    );
    assert_eq!(
        ok("../img/logo.png"),
        ResolvedLink::Internal("/v/img/logo.png".to_owned())
    );
    assert_eq!(
        ok("../../w/z.md"),
        ResolvedLink::Internal("/w/z/".to_owned())
    );
    assert_eq!(ok("#c"), ResolvedLink::Internal("#c".to_owned()));
    assert_eq!(
        ok("https://example.org/x"),
        ResolvedLink::External("https://example.org/x".to_owned())
    );
    assert_eq!(
        ok("mailto:a@b.c"),
        ResolvedLink::External("mailto:a@b.c".to_owned())
    );
}

#[test]
fn rejects_bad_links() {
    let c = linked();
    let from = id_of(&c, "volumes/v/sub/c.md");
    let code = |href: &str| c.resolve_link(from, href).unwrap_err().code;
    assert_eq!(code("../missing.md"), Code::BROKEN_LINK);
    assert_eq!(code("../a.md#nope"), Code::BROKEN_ANCHOR);
    assert_eq!(code("#nope"), Code::BROKEN_ANCHOR);
    assert_eq!(code("../draft.md"), Code::BROKEN_LINK);
    assert_eq!(code("javascript:alert(1)"), Code::FORBIDDEN_URL);
    assert_eq!(code("data:text/html,x"), Code::FORBIDDEN_URL);
    assert_eq!(code("//evil.example/x"), Code::FORBIDDEN_URL);
    assert_eq!(code("/v/a/"), Code::BROKEN_LINK);
    assert_eq!(code("../../../../x.md"), Code::BROKEN_LINK);
    assert_eq!(code("../a.md?x=1"), Code::BROKEN_LINK);
    assert_eq!(code(""), Code::BROKEN_LINK);
}

#[test]
fn terms_resolve_inside_their_volume_only() {
    let (c, report) = build(vec![
        volume(
            "v",
            0,
            &[
                ("_index.md", "# H\n"),
                ("glossary.md", "# G\n### Cache\n### Disk cache\n"),
                ("notes.md", "# Notes\n### Not a term\n"),
            ],
            &[],
        ),
        volume("w", 1, &[("_index.md", "# H\n")], &[]),
    ]);
    assert!(!report.has_errors());
    let from = id_of(&c, "volumes/v/_index.md");
    let target = c.resolve_term(from, "cache").unwrap();
    assert_eq!(target.url, "/v/glossary/#cache");
    assert_eq!(target.text, "Cache");
    assert_eq!(
        c.resolve_term(from, "not a term").unwrap_err().code,
        Code::UNKNOWN_TERM,
        "only glossary pages define terms"
    );
    assert_eq!(
        c.resolve_term(from, "missing").unwrap_err().code,
        Code::UNKNOWN_TERM
    );
    assert_eq!(
        c.resolve_term(id_of(&c, "volumes/w/_index.md"), "cache")
            .unwrap_err()
            .code,
        Code::UNKNOWN_TERM
    );
}

#[test]
fn duplicate_terms_are_reported() {
    let (_, report) = build(vec![volume(
        "v",
        0,
        &[
            ("_index.md", "# H\n"),
            ("terms.md", "### Cache\n"),
            ("glossary.md", "### cache\n"),
        ],
        &[],
    )]);
    assert_eq!(codes(&report), [Code::DUPLICATE_TERM]);
}

#[test]
fn aliases_become_redirects_and_may_not_collide() {
    let (c, report) = build(vec![volume(
        "v",
        0,
        &[
            ("_index.md", "# H\n"),
            ("a.md", "---\naliases:\n  - /old/a\n---\n"),
            ("b.md", "---\naliases:\n  - /v/a/\n  - /old/a/\n---\n"),
        ],
        &[],
    )]);
    assert_eq!(c.redirects(), [("/old/a/".to_owned(), "/v/a/".to_owned())]);
    assert_eq!(codes(&report), [Code::DUPLICATE_URL, Code::DUPLICATE_URL]);
}

#[test]
fn drafts_stay_out_of_navigation() {
    let c = linked();
    let draft = c.entry(id_of(&c, "volumes/v/draft.md")).unwrap();
    assert!(draft.draft && draft.prev.is_none() && draft.next.is_none());
    assert!(!c.volumes()[0].order.contains(&draft.id));
}

fn shelf(slug: &str, weight: i64) -> ShelfSource {
    ShelfSource {
        slug: slug.to_owned(),
        manifest: anaquel_schema::ShelfManifest::parse(
            &format!("title = \"{slug}\"\nweight = {weight}\n"),
            "shelf.toml",
        )
        .unwrap(),
    }
}

fn on_shelf(shelf: &str, mut source: VolumeSource) -> VolumeSource {
    source.shelf = Some(shelf.to_owned());
    source
}

fn index_only(slug: &str, weight: i64) -> VolumeSource {
    volume(
        slug,
        weight,
        &[("_index.md", "# Home\n"), ("a.md", "# A\n")],
        &["pic.png"],
    )
}

#[test]
fn a_volume_on_a_shelf_is_served_under_the_shelf() {
    let (c, report) = Catalog::build(
        library(),
        vec![shelf("tools", 0)],
        vec![on_shelf("tools", index_only("cli", 0))],
        &headings,
    );
    assert!(!report.has_errors(), "{:?}", report.items());
    assert_eq!(c.volumes()[0].address(), "tools/cli");
    assert_eq!(c.volumes()[0].folder(), "shelves/tools/volumes/cli");
    let urls: Vec<_> = c.entries().iter().map(|e| e.url.as_str()).collect();
    assert_eq!(urls, ["/tools/cli/", "/tools/cli/a/"]);
    let sources: Vec<_> = c.entries().iter().map(|e| e.source.as_str()).collect();
    assert_eq!(
        sources,
        [
            "shelves/tools/volumes/cli/_index.md",
            "shelves/tools/volumes/cli/a.md"
        ]
    );
    assert!(
        c.static_files()
            .contains(&("shelves/tools/volumes/cli/pic.png", "/tools/cli/pic.png"))
    );
}

#[test]
fn volumes_with_no_shelf_come_first_then_shelves_by_weight() {
    let (c, report) = Catalog::build(
        library(),
        vec![shelf("late", 5), shelf("early", 1)],
        vec![
            on_shelf("late", index_only("z", 0)),
            on_shelf("early", index_only("y", 0)),
            index_only("loose", 9),
        ],
        &headings,
    );
    assert!(!report.has_errors(), "{:?}", report.items());
    let order: Vec<_> = c.volumes().iter().map(Volume::address).collect();
    assert_eq!(order, ["loose", "early/y", "late/z"]);
    let shelves: Vec<_> = c.shelves().iter().map(|s| s.slug.as_str()).collect();
    assert_eq!(shelves, ["early", "late"]);
}

#[test]
fn the_same_volume_name_may_appear_on_two_shelves() {
    let (c, report) = Catalog::build(
        library(),
        vec![shelf("one", 0), shelf("two", 1)],
        vec![
            on_shelf("one", index_only("guide", 0)),
            on_shelf("two", index_only("guide", 0)),
        ],
        &headings,
    );
    assert!(!report.has_errors(), "{:?}", report.items());
    let order: Vec<_> = c.volumes().iter().map(Volume::address).collect();
    assert_eq!(order, ["one/guide", "two/guide"]);
}

#[test]
fn a_volume_that_names_a_missing_shelf_is_an_error() {
    let (_, report) = Catalog::build(
        library(),
        Vec::new(),
        vec![on_shelf("ghost", index_only("cli", 0))],
        &headings,
    );
    assert!(report.has_errors());
    assert_eq!(codes(&report), [Code::MANIFEST]);
}

#[test]
fn a_shelf_and_a_loose_volume_with_one_name_clash() {
    let (_, report) = Catalog::build(
        library(),
        vec![shelf("tools", 0)],
        vec![
            index_only("tools", 0),
            on_shelf("tools", index_only("cli", 0)),
        ],
        &headings,
    );
    assert!(
        codes(&report).contains(&Code::DUPLICATE_URL),
        "{:?}",
        report.items()
    );
}

#[test]
fn a_relative_link_inside_a_shelved_volume_resolves() {
    let (c, report) = Catalog::build(
        library(),
        vec![shelf("tools", 0)],
        vec![on_shelf(
            "tools",
            volume(
                "cli",
                0,
                &[
                    ("_index.md", "# Home\n"),
                    ("a.md", "# A\n\n[b](b.md) ![p](pic.png)\n"),
                    ("b.md", "# B\n"),
                ],
                &["pic.png"],
            ),
        )],
        &headings,
    );
    assert!(!report.has_errors(), "{:?}", report.items());
    let a = id_of(&c, "shelves/tools/volumes/cli/a.md");
    assert_eq!(
        c.resolve_link(a, "b.md").unwrap(),
        ResolvedLink::Internal("/tools/cli/b/".to_owned())
    );
    assert_eq!(
        c.resolve_link(a, "pic.png").unwrap(),
        ResolvedLink::Internal("/tools/cli/pic.png".to_owned())
    );
}
