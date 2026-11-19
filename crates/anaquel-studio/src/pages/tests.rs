use super::{PageList, PageStatus, list_pages};
use crate::test_folder::{TestFolder, make_link};

fn library() -> TestFolder {
    let t = TestFolder::new("pages");
    t.put("anaquel.toml", "schema = 1\n");
    t.put("volumes/guide/volume.toml", "title = \"Guide\"\n");
    t.put(
        "volumes/guide/_index.md",
        "---\ntitle: The guide\n---\n# Guide\n",
    );
    t.put("volumes/guide/setup.md", "# Setup\n");
    t.put("volumes/guide/ref/api.md", "# API\n");
    t.put("volumes/guide/img/logo.png", "not a page");
    t.put("shelves/team/volumes/ops/runbook.md", "# Runbook\n");
    t
}

fn find<'a>(pages: &'a [super::PageSummary], path: &str) -> &'a super::PageSummary {
    pages
        .iter()
        .find(|page| page.path == path)
        .unwrap_or_else(|| panic!("`{path}` is not in the list: {pages:?}"))
}

#[test]
fn the_pages_of_every_volume_and_shelf_are_listed_in_order() {
    let t = library();
    let list = list_pages(t.root());
    assert!(list.problems.is_empty(), "{:?}", list.problems);
    let paths: Vec<&str> = list.pages.iter().map(|page| page.path.as_str()).collect();
    // Pages of the plain volumes come first, then those of the shelves.
    assert_eq!(
        paths,
        [
            "volumes/guide/_index.md",
            "volumes/guide/ref/api.md",
            "volumes/guide/setup.md",
            "shelves/team/volumes/ops/runbook.md",
        ]
    );
}

#[test]
fn a_page_carries_its_shelf_volume_title_size_and_time() {
    let t = library();
    let list = list_pages(t.root());
    let index = find(&list.pages, "volumes/guide/_index.md");
    assert_eq!(index.shelf, None);
    assert_eq!(index.volume, "guide");
    assert_eq!(index.title.as_deref(), Some("The guide"));
    assert_eq!(index.status, PageStatus::Published);
    assert_eq!(index.bytes, 33);
    assert!(
        index
            .modified
            .is_some_and(|seconds| seconds > 1_000_000_000)
    );

    let runbook = find(&list.pages, "shelves/team/volumes/ops/runbook.md");
    assert_eq!(runbook.shelf.as_deref(), Some("team"));
    assert_eq!(runbook.volume, "ops");
    assert_eq!(runbook.title, None);
}

#[test]
fn files_that_are_not_pages_and_names_the_library_skips_are_not_listed() {
    let t = library();
    t.put("volumes/guide/.hidden/secret.md", "# Secret\n");
    t.put("volumes/guide/.draft.md", "# Draft\n");
    t.put(".git/notes.md", "# Git\n");
    t.put("volumes/.cache/guide/x.md", "# Cache\n");
    // Straight under `volumes`, so in no volume.
    t.put("volumes/stray.md", "# Stray\n");
    t.put("README.md", "# Readme\n");
    t.put("themes/folio/page.md", "# Theme\n");
    let list = list_pages(t.root());
    assert_eq!(list.pages.len(), 4, "{:?}", list.pages);
    assert!(list.problems.is_empty(), "{:?}", list.problems);
}

#[test]
fn the_extension_is_matched_in_any_case() {
    let t = library();
    t.put("volumes/guide/LOUD.MD", "# Loud\n");
    let list = list_pages(t.root());
    assert_eq!(
        find(&list.pages, "volumes/guide/LOUD.MD").status,
        PageStatus::Published
    );
}

#[test]
fn a_draft_is_listed_as_a_draft() {
    let t = library();
    t.put("volumes/guide/later.md", "---\ndraft: true\n---\n# Later\n");
    let list = list_pages(t.root());
    assert_eq!(
        find(&list.pages, "volumes/guide/later.md").status,
        PageStatus::Draft
    );
}

#[test]
fn a_page_with_broken_front_matter_is_listed_as_invalid_with_the_reason() {
    let t = library();
    t.put(
        "volumes/guide/broken.md",
        "---\ntitle: never closed\n# Broken\n",
    );
    t.put(
        "volumes/guide/unknown.md",
        "---\nflavour: sweet\n---\n# Unknown\n",
    );
    let list = list_pages(t.root());
    for path in ["volumes/guide/broken.md", "volumes/guide/unknown.md"] {
        let PageStatus::Invalid(reason) = &find(&list.pages, path).status else {
            panic!("`{path}` must be invalid");
        };
        assert!(!reason.is_empty(), "{path}");
    }
}

#[test]
fn crlf_and_a_byte_order_mark_do_not_hide_the_front_matter() {
    let t = library();
    t.put_bytes(
        "volumes/guide/windows.md",
        b"\xEF\xBB\xBF---\r\ntitle: From Windows\r\ndraft: true\r\n---\r\n# Body\r\n",
    );
    let list = list_pages(t.root());
    let page = find(&list.pages, "volumes/guide/windows.md");
    assert_eq!(page.title.as_deref(), Some("From Windows"));
    assert_eq!(page.status, PageStatus::Draft);
}

#[test]
fn a_page_the_editor_cannot_open_is_listed_with_the_reason() {
    let t = library();
    t.put_bytes("volumes/guide/binary.md", &[0xFF, 0xFE, 0x00]);
    t.put("volumes/guide/c#.md", "# Sharp\n");
    t.put("volumes/guide/100%.md", "# Percent\n");
    t.put_bytes("volumes/guide/huge.md", &vec![b'a'; 5 * 1024 * 1024 + 1]);
    let list = list_pages(t.root());
    for path in [
        "volumes/guide/binary.md",
        "volumes/guide/c#.md",
        "volumes/guide/100%.md",
        "volumes/guide/huge.md",
    ] {
        assert!(
            matches!(find(&list.pages, path).status, PageStatus::Unavailable(_)),
            "{path}"
        );
    }
}

#[test]
fn links_are_not_followed() {
    let t = library();
    let outside = TestFolder::new("pages-outside");
    outside.put("page.md", "# Outside\n");
    outside.put("volume/page.md", "# Outside volume\n");
    let linked_file = make_link(
        &outside.root().join("page.md"),
        &t.root().join("volumes/guide/linked.md"),
    );
    let linked_folder = make_link(
        &outside.root().join("volume"),
        &t.root().join("volumes/guide/linked-folder"),
    );
    let linked_volume = make_link(
        &outside.root().join("volume"),
        &t.root().join("volumes/leak"),
    );
    if !(linked_file && linked_folder && linked_volume) {
        return;
    }
    let list = list_pages(t.root());
    assert_eq!(list.pages.len(), 4, "{:?}", list.pages);
    assert!(
        list.pages
            .iter()
            .all(|page| !page.path.contains("link") && !page.path.contains("leak"))
    );
}

#[test]
fn a_library_without_volumes_has_no_pages_and_no_problems() {
    let t = TestFolder::new("pages-empty");
    t.put("anaquel.toml", "schema = 1\n");
    let list = list_pages(t.root());
    assert_eq!(list, PageList::default());
}

#[test]
fn a_folder_that_cannot_be_read_is_reported_instead_of_hidden() {
    let t = TestFolder::new("pages-problem");
    // `volumes` is a file, so reading it as a folder fails with something other than "not found".
    t.put("volumes", "not a folder");
    let list = list_pages(t.root());
    assert_eq!(list.pages, []);
    assert_eq!(list.problems.len(), 1, "{:?}", list.problems);
}
