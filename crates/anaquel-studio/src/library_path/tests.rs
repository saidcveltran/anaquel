use std::fs;
use std::path::Path;

use super::{LibraryPath, PathError};
use crate::test_folder::{TestFolder, make_link};

fn library() -> TestFolder {
    let t = TestFolder::new("path");
    t.put("anaquel.toml", "schema = 1\n");
    t.put("volumes/guide/volume.toml", "title = \"Guide\"\n");
    t.put("volumes/guide/setup.md", "# Setup\n");
    t
}

fn refusal(t: &TestFolder, relative: &str) -> PathError {
    match LibraryPath::new(t.root(), relative) {
        Err(error) => error,
        Ok(path) => panic!("`{relative}` must be refused, but gave {path:?}"),
    }
}

#[test]
fn plain_relative_paths_are_accepted() {
    let t = library();
    for relative in [
        "volumes/guide/setup.md",
        "volumes/guide/my page.md",
        "volumes/guide/ref/api.md",
        "shelves/team/volumes/guide/index.md",
        // Only COM1 to COM9 are reserved, not COM10 or a name that merely starts with a device.
        "volumes/guide/com10.md",
        "volumes/guide/console.md",
        "volumes/guide/auxiliary.md",
        // A file that does not exist yet, under folders that do not exist yet.
        "volumes/new/deeper/page.md",
    ] {
        let path = LibraryPath::new(t.root(), relative)
            .unwrap_or_else(|error| panic!("`{relative}` must be accepted: {error}"));
        assert_eq!(path.as_str(), relative);
        assert!(path.absolute().ends_with(Path::new(relative)), "{relative}");
    }
}

#[test]
fn an_empty_path_is_refused() {
    assert_eq!(refusal(&library(), ""), PathError::Empty);
}

#[test]
fn a_path_longer_than_the_limit_is_refused() {
    let t = library();
    let long = format!("volumes/guide/{}.md", "a/".repeat(600));
    assert_eq!(refusal(&t, &long), PathError::TooLong);
}

#[test]
fn an_absolute_path_a_drive_and_a_backslash_are_refused() {
    let t = library();
    for relative in [
        "/etc/passwd",
        "/volumes/guide/setup.md",
        "C:/Windows/win.ini",
        "C:win.ini",
        "volumes\\guide\\setup.md",
        "\\\\server\\share\\a.md",
        // An alternate data stream on Windows.
        "volumes/guide/setup.md:stream",
    ] {
        assert!(
            matches!(refusal(&t, relative), PathError::NotRelative(_)),
            "{relative}"
        );
    }
}

#[test]
fn a_reference_to_the_parent_or_the_current_folder_is_refused() {
    let t = library();
    for relative in [
        "..",
        "../anaquel.toml",
        "volumes/../anaquel.toml",
        "volumes/guide/../../../outside.md",
        "./volumes/guide/setup.md",
        "volumes/./guide/setup.md",
        "volumes//guide/setup.md",
        "volumes/guide/setup.md/",
    ] {
        assert!(
            matches!(refusal(&t, relative), PathError::Segment { .. }),
            "{relative}"
        );
    }
}

#[test]
fn a_name_the_library_skips_is_refused() {
    let t = library();
    for relative in [
        ".git/config",
        "volumes/.hidden/page.md",
        "volumes/guide/.draft.md",
    ] {
        assert!(
            matches!(refusal(&t, relative), PathError::Segment { .. }),
            "{relative}"
        );
    }
}

#[test]
fn names_that_windows_changes_or_reserves_are_refused_on_every_system() {
    let t = library();
    for relative in [
        "volumes/guide/setup.md.",
        "volumes/guide/setup.md ",
        "volumes/guide/CON.md",
        "volumes/guide/con",
        "volumes/guide/Nul.txt",
        "volumes/guide/aux.tar.gz",
        "volumes/guide/PRN.md",
        "volumes/guide/COM1.md",
        "volumes/guide/com9",
        "volumes/guide/LPT1.md",
        "volumes/guide/lpt5.md",
        "volumes/guide/CONIN$",
        "volumes/guide/COM\u{b9}.md",
    ] {
        assert!(
            matches!(refusal(&t, relative), PathError::Segment { .. }),
            "{relative}"
        );
    }
}

#[test]
fn characters_a_file_name_cannot_hold_are_refused() {
    let t = library();
    for relative in [
        "volumes/guide/a<b.md",
        "volumes/guide/a>b.md",
        "volumes/guide/a\"b.md",
        "volumes/guide/a|b.md",
        "volumes/guide/a?b.md",
        "volumes/guide/a*b.md",
        // These two would end or change the address of the page.
        "volumes/guide/c#.md",
        "volumes/guide/100%.md",
        "volumes/guide/a\u{7}b.md",
        "volumes/guide/a\nb.md",
        "volumes/guide/a\0b.md",
    ] {
        assert!(
            matches!(refusal(&t, relative), PathError::Segment { .. }),
            "{relative:?}"
        );
    }
}

#[test]
fn a_name_longer_than_a_file_system_allows_is_refused() {
    let t = library();
    let longest_ok = format!("volumes/guide/{}.md", "a".repeat(252));
    assert!(LibraryPath::new(t.root(), &longest_ok).is_ok());
    let too_long = format!("volumes/guide/{}.md", "a".repeat(253));
    assert!(matches!(refusal(&t, &too_long), PathError::Segment { .. }));
}

#[test]
fn a_file_cannot_be_a_folder_of_the_path() {
    let t = library();
    assert!(matches!(
        refusal(&t, "volumes/guide/setup.md/inner.md"),
        PathError::Segment { .. }
    ));
}

#[test]
fn a_missing_library_folder_is_reported() {
    let t = library();
    let missing = t.root().join("nowhere");
    assert!(matches!(
        LibraryPath::new(&missing, "volumes/guide/setup.md"),
        Err(PathError::Unreadable(_))
    ));
}

#[test]
fn the_page_test_follows_the_layout_of_a_library() {
    let t = library();
    let is_page = |relative: &str| LibraryPath::new(t.root(), relative).unwrap().is_page();
    for page in [
        "volumes/guide/setup.md",
        "volumes/guide/_index.md",
        "volumes/guide/ref/api.md",
        "volumes/guide/SHOUT.MD",
        "shelves/team/volumes/guide/setup.md",
    ] {
        assert!(is_page(page), "{page}");
    }
    for other in [
        "anaquel.toml",
        "README.md",
        "volumes/guide/volume.toml",
        "volumes/guide/img/logo.png",
        // A file straight under `volumes` is not in a volume.
        "volumes/stray.md",
        "shelves/team/volumes/stray.md",
        "shelves/team/stray.md",
        "themes/folio/page.md",
        // Only a name, no stem.
        "volumes/guide/.md",
    ] {
        // `.md` alone starts with a dot, so it is refused before the page test.
        if let Ok(path) = LibraryPath::new(t.root(), other) {
            assert!(!path.is_page(), "{other}");
        }
    }
}

#[test]
fn a_folder_that_is_a_link_is_refused_even_when_it_points_inside() {
    let t = library();
    if !make_link(
        &t.root().join("volumes/guide"),
        &t.root().join("volumes/alias"),
    ) {
        return;
    }
    assert!(matches!(
        refusal(&t, "volumes/alias/setup.md"),
        PathError::Link(_)
    ));
}

#[test]
fn a_link_that_leaves_the_library_is_refused() {
    let t = library();
    let outside = TestFolder::new("path-outside");
    outside.put("secret.md", "# Secret\n");
    if !make_link(outside.root(), &t.root().join("volumes/leak")) {
        return;
    }
    assert!(matches!(
        refusal(&t, "volumes/leak/secret.md"),
        PathError::Link(_)
    ));
    // The file the link leads to was never touched.
    assert_eq!(outside.bytes("secret.md"), b"# Secret\n");
}

#[test]
fn a_page_that_is_a_link_is_refused() {
    let t = library();
    let outside = TestFolder::new("path-outside-file");
    outside.put("secret.md", "# Secret\n");
    if !make_link(
        &outside.root().join("secret.md"),
        &t.root().join("volumes/guide/linked.md"),
    ) {
        return;
    }
    assert!(matches!(
        refusal(&t, "volumes/guide/linked.md"),
        PathError::Link(_)
    ));
}

#[test]
fn a_library_folder_that_is_itself_reached_through_a_link_is_accepted() {
    let t = library();
    let holder = TestFolder::new("path-holder");
    let alias = holder.root().join("library-alias");
    if !make_link(t.root(), &alias) {
        return;
    }
    let path = LibraryPath::new(&alias, "volumes/guide/setup.md").unwrap();
    assert_eq!(fs::read(path.absolute()).unwrap(), b"# Setup\n");
}
