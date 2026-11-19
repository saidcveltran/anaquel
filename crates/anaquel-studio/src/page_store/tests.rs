use std::fs;

use super::{MAX_PAGE_BYTES, Revision, SaveError, read_page, save_page};
use crate::library_path::LibraryPath;
use crate::test_folder::{TestFolder, make_link};

const PAGE: &str = "volumes/guide/setup.md";

fn library(page: &[u8]) -> (TestFolder, LibraryPath) {
    let t = TestFolder::new("store");
    t.put("anaquel.toml", "schema = 1\n");
    t.put_bytes(PAGE, page);
    let path = LibraryPath::new(t.root(), PAGE).unwrap();
    (t, path)
}

fn path_of(t: &TestFolder, relative: &str) -> LibraryPath {
    LibraryPath::new(t.root(), relative).unwrap()
}

#[test]
fn a_revision_is_the_sha256_of_the_bytes_in_lowercase_hexadecimal() {
    // The standard test vector for "abc".
    assert_eq!(
        Revision::of(b"abc").as_str(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        Revision::of(b"").to_string(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn only_64_lowercase_hexadecimal_digits_parse_as_a_revision() {
    let good = Revision::of(b"abc");
    assert_eq!(Revision::parse(good.as_str()).unwrap(), good);
    let uppercase = good.as_str().to_uppercase();
    let one_too_long = format!("{}0", good.as_str());
    let not_hex = format!("g{}", &good.as_str()[1..]);
    let spaced = format!(" {}", &good.as_str()[1..]);
    let bad = [
        "",
        "abc",
        uppercase.as_str(),
        one_too_long.as_str(),
        &good.as_str()[1..],
        not_hex.as_str(),
        spaced.as_str(),
    ];
    for text in bad {
        assert_eq!(
            Revision::parse(text),
            Err(SaveError::BadRevision),
            "{text:?}"
        );
    }
}

#[test]
fn a_page_is_read_as_the_library_reads_it() {
    let raw = b"\xEF\xBB\xBF# Title\r\n\r\nText.\r\n";
    let (_t, path) = library(raw);
    let page = read_page(&path).unwrap();
    assert_eq!(page.text, "# Title\n\nText.\n");
    // The revision is the one of the file, not of the normalised text.
    assert_eq!(page.revision, Revision::of(raw));
    assert_ne!(page.revision, Revision::of(page.text.as_bytes()));
}

#[test]
fn a_save_replaces_the_page_and_returns_the_revision_of_what_was_written() {
    let (t, path) = library(b"# Old\n");
    let loaded = read_page(&path).unwrap();
    let revision = save_page(&path, "# New\n\nMore.\n", &loaded.revision).unwrap();
    assert_eq!(t.bytes(PAGE), b"# New\n\nMore.\n");
    assert_eq!(revision, Revision::of(&t.bytes(PAGE)));
    assert_eq!(read_page(&path).unwrap().revision, revision);
    // No temporary file is left beside the page.
    let names: Vec<String> = fs::read_dir(t.root().join("volumes/guide"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["setup.md"]);
}

#[test]
fn the_revision_a_save_returns_is_the_one_the_next_save_needs() {
    let (t, path) = library(b"# One\n");
    let first = read_page(&path).unwrap().revision;
    let second = save_page(&path, "# Two\n", &first).unwrap();
    let third = save_page(&path, "# Three\n", &second).unwrap();
    assert_eq!(t.bytes(PAGE), b"# Three\n");
    assert_ne!(first, second);
    assert_ne!(second, third);
}

#[test]
fn an_old_revision_is_refused_and_the_page_is_left_as_it_is() {
    let (t, path) = library(b"# One\n");
    let first = read_page(&path).unwrap().revision;
    save_page(&path, "# Two\n", &first).unwrap();
    let before = t.bytes(PAGE);
    assert_eq!(save_page(&path, "# Mine\n", &first), Err(SaveError::Stale));
    assert_eq!(t.bytes(PAGE), before);
}

#[test]
fn a_page_edited_by_another_program_is_not_overwritten() {
    let (t, path) = library(b"# Old\n");
    let loaded = read_page(&path).unwrap();
    t.put(PAGE, "# Edited in another editor\n");
    assert_eq!(
        save_page(&path, "# Mine\n", &loaded.revision),
        Err(SaveError::Stale)
    );
    assert_eq!(t.bytes(PAGE), b"# Edited in another editor\n");
}

#[test]
fn a_revision_that_is_valid_but_wrong_is_stale() {
    let (t, path) = library(b"# Old\n");
    let before = t.bytes(PAGE);
    let other = Revision::of(b"something else");
    assert_eq!(save_page(&path, "# New\n", &other), Err(SaveError::Stale));
    assert_eq!(t.bytes(PAGE), before);
}

#[test]
fn a_change_of_line_ends_alone_counts_as_a_change() {
    let (t, path) = library(b"# Old\nline\n");
    let loaded = read_page(&path).unwrap();
    // Same text for the library, other bytes on disk.
    t.put_bytes(PAGE, b"# Old\r\nline\r\n");
    assert_eq!(
        save_page(&path, "# New\n", &loaded.revision),
        Err(SaveError::Stale)
    );
}

#[test]
fn a_page_keeps_its_byte_order_mark_and_its_line_ends() {
    let (t, path) = library(b"\xEF\xBB\xBF# Old\r\n\r\nText.\r\n");
    let loaded = read_page(&path).unwrap();
    // The editor may send either line end; the page keeps its own.
    save_page(&path, "# New\r\n\r\nText.\n", &loaded.revision).unwrap();
    assert_eq!(t.bytes(PAGE), b"\xEF\xBB\xBF# New\r\n\r\nText.\r\n");
}

#[test]
fn saving_the_same_text_keeps_the_bytes_and_the_revision() {
    let (t, path) = library(b"# Same\n");
    let loaded = read_page(&path).unwrap();
    let revision = save_page(&path, "# Same\n", &loaded.revision).unwrap();
    assert_eq!(revision, loaded.revision);
    assert_eq!(t.bytes(PAGE), b"# Same\n");
}

#[test]
fn a_page_that_mixes_line_ends_is_refused_untouched() {
    let raw = b"# Old\r\nline\nline\n";
    let (t, path) = library(raw);
    let loaded = read_page(&path).unwrap();
    assert_eq!(
        save_page(&path, "# New\n", &loaded.revision),
        Err(SaveError::MixedLineEndings)
    );
    assert_eq!(t.bytes(PAGE), raw);
}

#[test]
fn a_file_that_is_not_a_page_cannot_be_read_or_saved() {
    let (t, _) = library(b"# Old\n");
    t.put("volumes/stray.md", "# Stray\n");
    t.put("volumes/guide/volume.toml", "title = \"Guide\"\n");
    t.put("README.md", "# Readme\n");
    for other in [
        "anaquel.toml",
        "volumes/stray.md",
        "volumes/guide/volume.toml",
        "README.md",
    ] {
        let path = path_of(&t, other);
        let before = t.bytes(other);
        assert_eq!(read_page(&path), Err(SaveError::NotAPage(other.to_owned())));
        assert_eq!(
            save_page(&path, "# Hijacked\n", &Revision::of(&before)),
            Err(SaveError::NotAPage(other.to_owned()))
        );
        assert_eq!(t.bytes(other), before, "{other}");
    }
}

#[test]
fn a_page_that_does_not_exist_is_missing_and_a_save_does_not_create_it() {
    let (t, _) = library(b"# Old\n");
    let path = path_of(&t, "volumes/guide/new.md");
    assert!(matches!(read_page(&path), Err(SaveError::Missing(_))));
    assert!(matches!(
        save_page(&path, "# New\n", &Revision::of(b"")),
        Err(SaveError::Missing(_))
    ));
    assert!(!t.root().join("volumes/guide/new.md").exists());
}

#[test]
fn a_folder_with_the_name_of_a_page_is_not_a_page() {
    let (t, _) = library(b"# Old\n");
    fs::create_dir_all(t.root().join("volumes/guide/folder.md")).unwrap();
    let path = path_of(&t, "volumes/guide/folder.md");
    assert!(matches!(read_page(&path), Err(SaveError::NotAPage(_))));
}

#[test]
fn a_page_that_is_not_utf8_cannot_be_read_or_saved() {
    let raw = [b'#', b' ', 0xFF, 0xFE, b'\n'];
    let (t, path) = library(&raw);
    assert_eq!(read_page(&path), Err(SaveError::NotText));
    assert_eq!(
        save_page(&path, "# New\n", &Revision::of(&raw)),
        Err(SaveError::NotText)
    );
    assert_eq!(t.bytes(PAGE), raw);
}

#[test]
fn text_with_a_nul_character_is_refused() {
    let (t, path) = library(b"# Old\n");
    let loaded = read_page(&path).unwrap();
    assert_eq!(
        save_page(&path, "# New\0\n", &loaded.revision),
        Err(SaveError::NotText)
    );
    assert_eq!(t.bytes(PAGE), b"# Old\n");
}

#[test]
fn the_size_limit_is_enforced_on_both_sides() {
    let limit = usize::try_from(MAX_PAGE_BYTES).unwrap();
    // A page at the limit is read and saved; one byte more is refused.
    let at_limit = vec![b'a'; limit];
    let (t, path) = library(&at_limit);
    let loaded = read_page(&path).unwrap();
    assert_eq!(loaded.text.len(), limit);
    let too_long = "a".repeat(limit + 1);
    assert_eq!(
        save_page(&path, &too_long, &loaded.revision),
        Err(SaveError::TooLarge)
    );
    assert_eq!(t.bytes(PAGE), at_limit);

    let (_t, path) = library(&vec![b'a'; limit + 1]);
    assert_eq!(read_page(&path), Err(SaveError::TooLarge));
}

#[test]
fn a_page_that_became_a_link_after_the_path_was_checked_is_refused() {
    let (t, path) = library(b"# Old\n");
    let outside = TestFolder::new("store-outside");
    outside.put("secret.md", "# Secret\n");
    fs::remove_file(t.root().join(PAGE)).unwrap();
    if !make_link(&outside.root().join("secret.md"), &t.root().join(PAGE)) {
        return;
    }
    assert_eq!(read_page(&path), Err(SaveError::NotAPage(PAGE.to_owned())));
    assert_eq!(
        save_page(&path, "# Hijacked\n", &Revision::of(b"# Secret\n")),
        Err(SaveError::NotAPage(PAGE.to_owned()))
    );
    assert_eq!(outside.bytes("secret.md"), b"# Secret\n");
}

#[cfg(unix)]
#[test]
fn the_permission_bits_of_the_page_survive_a_save() {
    use std::os::unix::fs::PermissionsExt;

    let (t, path) = library(b"# Old\n");
    fs::set_permissions(t.root().join(PAGE), fs::Permissions::from_mode(0o640)).unwrap();
    let loaded = read_page(&path).unwrap();
    save_page(&path, "# New\n", &loaded.revision).unwrap();
    let mode = fs::metadata(t.root().join(PAGE))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o640);
}
