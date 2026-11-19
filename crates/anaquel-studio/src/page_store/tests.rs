use std::fs;

use anaquel_pipeline::RewriteError;

use super::{MAX_PAGE_BYTES, Revision, SaveError, read_page, save_page, save_with};
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

#[test]
fn a_carriage_return_that_does_not_end_a_line_is_refused_on_both_sides() {
    let (t, path) = library(b"# Old\n");
    let loaded = read_page(&path).unwrap();
    // `\r\r\n` is a lone `\r` followed by a line end; one pass of `\r\n` -> `\n` would leave a new
    // `\r\n` behind and the page would mix line ends from then on.
    for text in ["a\r\r\nb\n", "old mac\rline\r", "a\r"] {
        assert_eq!(
            save_page(&path, text, &loaded.revision),
            Err(SaveError::NotText),
            "{text:?}"
        );
    }
    assert_eq!(t.bytes(PAGE), b"# Old\n");

    // A page that already has one cannot be opened, so nobody edits a text the save would refuse.
    let (_t, path) = library(b"line\rline\r");
    assert_eq!(read_page(&path), Err(SaveError::NotText));
    let (_t, path) = library(b"a\r\r\nb\n");
    assert_eq!(read_page(&path), Err(SaveError::NotText));
}

#[test]
fn a_leading_byte_order_mark_in_the_text_is_refused_because_reading_would_drop_it() {
    let (t, path) = library(b"# Old\n");
    let loaded = read_page(&path).unwrap();
    assert_eq!(
        save_page(&path, "\u{feff}# New\n", &loaded.revision),
        Err(SaveError::NotText)
    );
    assert_eq!(t.bytes(PAGE), b"# Old\n");
    // A mark inside the text is ordinary text.
    assert!(save_page(&path, "# New \u{feff} text\n", &loaded.revision).is_ok());
}

#[test]
fn the_size_limit_applies_to_the_page_as_it_is_written() {
    // 3 MB of line ends is below the limit as `\n` and above it as `\r\n`.
    let (t, path) = library(b"a\r\n");
    let loaded = read_page(&path).unwrap();
    let doubled = "\n".repeat(3_000_000);
    assert_eq!(
        save_page(&path, &doubled, &loaded.revision),
        Err(SaveError::TooLarge)
    );
    assert_eq!(t.bytes(PAGE), b"a\r\n");
    // The same text in a page with `\n` line ends is saved, and can be read again.
    let (t, path) = library(b"a\n");
    let loaded = read_page(&path).unwrap();
    let revision = save_page(&path, &doubled, &loaded.revision).unwrap();
    assert_eq!(read_page(&path).unwrap().revision, revision);
    assert_eq!(t.bytes(PAGE).len(), 3_000_000);
}

#[test]
fn the_revision_returned_is_the_one_of_the_bytes_the_write_reported() {
    let (t, path) = library(b"# Old\n");
    let loaded = read_page(&path).unwrap();
    // The write reports what it wrote; the revision is that, not a second read of the file.
    let revision = save_with(&path, "# New\n", &loaded.revision, |_, _, _| {
        Ok(b"what the write reported".to_vec())
    })
    .unwrap();
    assert_eq!(revision, Revision::of(b"what the write reported"));
    assert_ne!(revision, Revision::of(&t.bytes(PAGE)));
}

#[test]
fn every_failure_of_the_write_maps_to_its_own_error() {
    let (_t, path) = library(b"# Old\n");
    let loaded = read_page(&path).unwrap();
    let run = |error: RewriteError| {
        save_with(&path, "# New\n", &loaded.revision, move |_, _, _| {
            Err(error)
        })
    };
    assert_eq!(run(RewriteError::Changed), Err(SaveError::Stale));
    assert_eq!(
        run(RewriteError::MixedLineEndings),
        Err(SaveError::MixedLineEndings)
    );
    assert_eq!(run(RewriteError::TooLarge), Err(SaveError::TooLarge));
    assert_eq!(
        run(RewriteError::Failed {
            message: "disk full".to_owned(),
            unrestored: vec!["`x` is a temporary file to delete".to_owned()],
        }),
        Err(SaveError::Failed {
            message: "disk full".to_owned(),
            unrestored: vec!["`x` is a temporary file to delete".to_owned()],
        })
    );
}

#[test]
fn the_write_gets_the_normalised_text_and_the_text_read_from_the_page() {
    let (_t, path) = library(b"\xEF\xBB\xBF# Old\r\nline\r\n");
    let loaded = read_page(&path).unwrap();
    let mut seen = None;
    save_with(
        &path,
        "# New\r\nline\r\n",
        &loaded.revision,
        |_, text, expected| {
            seen = Some((text.to_owned(), expected.to_owned()));
            Ok(Vec::new())
        },
    )
    .unwrap();
    assert_eq!(
        seen,
        Some(("# New\nline\n".to_owned(), "# Old\nline\n".to_owned()))
    );
}

#[test]
fn a_leftover_staged_file_does_not_stop_the_next_save() {
    let (t, path) = library(b"# Old\n");
    t.put("volumes/guide/.anaquel-staged-000000", "left by a crash");
    let loaded = read_page(&path).unwrap();
    save_page(&path, "# New\n", &loaded.revision).unwrap();
    assert_eq!(t.bytes(PAGE), b"# New\n");
    assert_eq!(
        t.bytes("volumes/guide/.anaquel-staged-000000"),
        b"left by a crash"
    );
}

#[test]
fn a_read_only_page_is_either_saved_and_still_read_only_or_left_untouched() {
    let (t, path) = library(b"# Old\n");
    let file = t.root().join(PAGE);
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions).unwrap();
    let loaded = read_page(&path).unwrap();
    let outcome = save_page(&path, "# New\n", &loaded.revision);
    let after = fs::metadata(&file).unwrap().permissions().readonly();
    // Windows refuses to replace a read-only file; Unix replaces it through the folder and the
    // new file takes the same mode. Either way no half state and no leftover remains.
    match outcome {
        Ok(_) => {
            assert_eq!(t.bytes(PAGE), b"# New\n");
            assert!(after, "the new file must keep the read-only bit");
        }
        Err(SaveError::Failed { unrestored, .. }) => {
            assert_eq!(unrestored, Vec::<String>::new());
            assert_eq!(t.bytes(PAGE), b"# Old\n");
        }
        Err(other) => panic!("unexpected error: {other:?}"),
    }
    let names: Vec<String> = fs::read_dir(t.root().join("volumes/guide"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["setup.md"]);
    // Let the folder be removed.
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(&file, permissions).unwrap();
}
