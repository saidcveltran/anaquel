use super::{PageList, PageStatus, PageSummary, list_pages};
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
    t.put("shelves/team/shelf.toml", "title = \"Team\"\n");
    t.put("shelves/team/volumes/ops/volume.toml", "title = \"Ops\"\n");
    t.put("shelves/team/volumes/ops/runbook.md", "# Runbook\n");
    t
}

fn find<'a>(pages: &'a [PageSummary], path: &str) -> &'a PageSummary {
    pages
        .iter()
        .find(|page| page.path == path)
        .unwrap_or_else(|| panic!("`{path}` is not in the list: {pages:?}"))
}

fn paths(list: &PageList) -> Vec<&str> {
    list.pages.iter().map(|page| page.path.as_str()).collect()
}

#[test]
fn the_pages_of_every_volume_and_shelf_are_listed_in_order() {
    let t = library();
    let list = list_pages(t.root());
    assert_eq!(list.problems, Vec::<String>::new());
    // Pages of the plain volumes come first, then those of the shelves.
    assert_eq!(
        paths(&list),
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
    assert_eq!(list.problems, Vec::<String>::new());
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
    // The editor would change or refuse these when saving them back.
    t.put_bytes("volumes/guide/old-mac.md", b"line\rline\r");
    t.put_bytes("volumes/guide/nul.md", b"# A\0B\n");
    t.put_bytes(
        "volumes/guide/two-marks.md",
        b"\xEF\xBB\xBF\xEF\xBB\xBF# Two\n",
    );
    let list = list_pages(t.root());
    for path in [
        "volumes/guide/binary.md",
        "volumes/guide/c#.md",
        "volumes/guide/100%.md",
        "volumes/guide/huge.md",
        "volumes/guide/old-mac.md",
        "volumes/guide/nul.md",
        "volumes/guide/two-marks.md",
    ] {
        assert!(
            matches!(find(&list.pages, path).status, PageStatus::Unavailable(_)),
            "{path}"
        );
    }
}

#[test]
fn a_link_to_a_file_or_to_a_folder_inside_a_volume_is_not_followed() {
    let t = library();
    let outside = TestFolder::new("pages-outside");
    outside.put("page.md", "# Outside\n");
    outside.put("folder/page.md", "# Outside folder\n");
    let linked_file = make_link(
        &outside.root().join("page.md"),
        &t.root().join("volumes/guide/linked.md"),
    );
    let linked_folder = make_link(
        &outside.root().join("folder"),
        &t.root().join("volumes/guide/linked-folder"),
    );
    if !(linked_file && linked_folder) {
        return;
    }
    let list = list_pages(t.root());
    assert_eq!(list.pages.len(), 4, "{:?}", list.pages);
    assert!(
        list.pages.iter().all(|page| !page.path.contains("linked")),
        "{:?}",
        list.pages
    );
}

#[test]
fn a_volume_that_is_a_link_is_read_by_the_build_so_it_is_listed_but_cannot_be_opened() {
    let t = library();
    let outside = TestFolder::new("pages-linked-volume");
    outside.put("volume.toml", "title = \"Linked\"\n");
    outside.put("secret.md", "---\ntitle: Do not show\n---\n# Secret\n");
    if !make_link(outside.root(), &t.root().join("volumes/linked")) {
        return;
    }
    let list = list_pages(t.root());
    let page = find(&list.pages, "volumes/linked/secret.md");
    assert!(
        matches!(page.status, PageStatus::Unavailable(_)),
        "{page:?}"
    );
    // The file behind the link was not opened, so nothing of it is shown.
    assert_eq!(page.title, None);
}

#[test]
fn a_folder_that_the_build_does_not_take_for_a_volume_is_reported_and_left_out() {
    let t = library();
    for name in ["my volume", "v1.0", "a b", "caf\u{e9}!"] {
        t.put(&format!("volumes/{name}/volume.toml"), "title = \"X\"\n");
        t.put(&format!("volumes/{name}/page.md"), "# X\n");
    }
    t.put("shelves/bad shelf/shelf.toml", "title = \"X\"\n");
    t.put("shelves/bad shelf/volumes/v/volume.toml", "title = \"X\"\n");
    t.put("shelves/bad shelf/volumes/v/page.md", "# X\n");
    let list = list_pages(t.root());
    assert_eq!(list.pages.len(), 4, "{:?}", list.pages);
    assert_eq!(list.problems.len(), 5, "{:?}", list.problems);
    assert!(
        list.problems
            .iter()
            .all(|line| line.contains("cannot be a URL segment")),
        "{:?}",
        list.problems
    );
    // Letters other than ASCII, digits, `-` and `_` are valid segments.
    t.put("volumes/guía-2/volume.toml", "title = \"G\"\n");
    t.put("volumes/guía-2/page.md", "# G\n");
    assert!(paths(&list_pages(t.root())).contains(&"volumes/guía-2/page.md"));
}

#[test]
fn a_volume_or_a_shelf_without_a_valid_manifest_is_reported_and_left_out() {
    let t = library();
    t.put("volumes/no-manifest/page.md", "# X\n");
    t.put("volumes/bad-manifest/volume.toml", "title = \n");
    t.put("volumes/bad-manifest/page.md", "# X\n");
    t.put("volumes/unknown-key/volume.toml", "colour = \"red\"\n");
    t.put("volumes/unknown-key/page.md", "# X\n");
    t.put(
        "shelves/no-manifest/volumes/v/volume.toml",
        "title = \"V\"\n",
    );
    t.put("shelves/no-manifest/volumes/v/page.md", "# X\n");
    let list = list_pages(t.root());
    assert_eq!(list.pages.len(), 4, "{:?}", list.pages);
    assert_eq!(list.problems.len(), 4, "{:?}", list.problems);
    for place in [
        "volumes/no-manifest",
        "volumes/bad-manifest",
        "volumes/unknown-key",
        "shelves/no-manifest",
    ] {
        assert!(
            list.problems
                .iter()
                .any(|line| line.contains(place) && line.contains("the build skips")),
            "{place}: {:?}",
            list.problems
        );
    }
}

#[test]
fn a_temporary_file_left_by_an_interrupted_save_is_reported_not_listed() {
    let t = library();
    t.put("volumes/guide/.anaquel-staged-000000", "half a page");
    t.put(
        "volumes/guide/ref/.anaquel-held-000003",
        "a page that was moving",
    );
    let list = list_pages(t.root());
    assert_eq!(list.pages.len(), 4, "{:?}", list.pages);
    assert_eq!(list.problems.len(), 2, "{:?}", list.problems);
    assert!(
        list.problems
            .iter()
            .all(|line| line.contains("temporary file")),
        "{:?}",
        list.problems
    );
    assert!(list.problems[0].starts_with("volumes/guide/.anaquel-staged-000000"));
}

#[test]
fn a_library_without_volumes_has_no_pages_and_no_problems() {
    let t = TestFolder::new("pages-empty");
    t.put("anaquel.toml", "schema = 1\n");
    assert_eq!(list_pages(t.root()), PageList::default());
}

#[test]
fn a_volumes_entry_that_is_a_file_is_ignored_as_the_build_ignores_it() {
    let t = TestFolder::new("pages-file");
    t.put("volumes", "not a folder");
    t.put("shelves", "not a folder either");
    assert_eq!(list_pages(t.root()), PageList::default());
}

#[cfg(unix)]
#[test]
fn a_folder_that_cannot_be_read_is_reported_instead_of_hidden() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let t = library();
    let locked = t.root().join("volumes");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let readable_anyway = fs::read_dir(&locked).is_ok();
    let list = list_pages(t.root());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    // A process that ignores permissions (root) reads it; there is nothing to report then.
    if readable_anyway {
        return;
    }
    assert_eq!(list.problems.len(), 1, "{:?}", list.problems);
    assert!(
        list.problems[0].starts_with("volumes"),
        "{:?}",
        list.problems
    );
}
