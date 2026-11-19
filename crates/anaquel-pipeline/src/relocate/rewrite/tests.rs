use std::fs;

use super::super::disk::RealDisk;
use super::super::tests::{Failing, snapshot};
use super::{RewriteError, rewrite_page, rewrite_page_on};
use crate::tests::Scratch;

const PAGE: &str = "volumes/guide/setup.md";

fn folder(name: &str, bytes: &[u8]) -> Scratch {
    let s = Scratch::new(name);
    let page = s.0.join(PAGE);
    fs::create_dir_all(page.parent().unwrap()).unwrap();
    fs::write(page, bytes).unwrap();
    s
}

fn on_disk(s: &Scratch) -> Vec<u8> {
    fs::read(s.0.join(PAGE)).unwrap()
}

#[test]
fn the_new_text_replaces_the_page_and_leaves_no_temporary_file() {
    let s = folder("rewrite-plain", b"# Old\n");
    rewrite_page(&s.0.join(PAGE), "# New\n\nMore.\n", "# Old\n").unwrap();
    assert_eq!(on_disk(&s), b"# New\n\nMore.\n");
    let names: Vec<String> = snapshot(&s.0).into_keys().collect();
    assert_eq!(names, ["volumes", "volumes/guide", PAGE]);
}

#[test]
fn a_byte_order_mark_and_crlf_line_ends_are_kept() {
    let s = folder("rewrite-crlf", b"\xEF\xBB\xBF# Old\r\n\r\nText.\r\n");
    rewrite_page(&s.0.join(PAGE), "# New\n\nText.\n", "# Old\n\nText.\n").unwrap();
    assert_eq!(on_disk(&s), b"\xEF\xBB\xBF# New\r\n\r\nText.\r\n");
}

#[test]
fn a_page_that_changed_since_it_was_read_is_not_written() {
    let s = folder("rewrite-stale", b"# Edited elsewhere\n");
    let before = snapshot(&s.0);
    let error = rewrite_page(&s.0.join(PAGE), "# Mine\n", "# Old\n").unwrap_err();
    assert_eq!(error, RewriteError::Changed);
    assert_eq!(snapshot(&s.0), before);
}

#[test]
fn a_page_that_is_not_text_counts_as_changed() {
    let s = folder("rewrite-binary", &[0xFF, 0xFE, 0x00]);
    let before = snapshot(&s.0);
    let error = rewrite_page(&s.0.join(PAGE), "# Mine\n", "# Old\n").unwrap_err();
    assert_eq!(error, RewriteError::Changed);
    assert_eq!(snapshot(&s.0), before);
}

#[test]
fn a_page_that_mixes_line_ends_is_refused_untouched() {
    let s = folder("rewrite-mixed", b"# Old\r\nline\nline\n");
    let before = snapshot(&s.0);
    let error = rewrite_page(&s.0.join(PAGE), "# New\n", "# Old\nline\nline\n").unwrap_err();
    assert_eq!(error, RewriteError::MixedLineEndings);
    assert_eq!(snapshot(&s.0), before);
}

#[test]
fn a_missing_page_is_an_error_and_creates_nothing() {
    let s = Scratch::new("rewrite-missing");
    let before = snapshot(&s.0);
    let error = rewrite_page(&s.0.join(PAGE), "# New\n", "").unwrap_err();
    assert!(matches!(error, RewriteError::Failed { .. }), "{error:?}");
    assert_eq!(snapshot(&s.0), before);
}

#[test]
fn the_bytes_written_are_returned() {
    let s = folder("rewrite-returned", b"\xEF\xBB\xBF# Old\r\n");
    let written = rewrite_page(&s.0.join(PAGE), "# New\n\nMore.\n", "# Old\n").unwrap();
    assert_eq!(written, b"\xEF\xBB\xBF# New\r\n\r\nMore.\r\n");
    assert_eq!(written, on_disk(&s));
}

#[test]
fn a_staged_name_that_is_taken_is_never_written_through_and_the_next_one_is_used() {
    let s = folder("rewrite-taken", b"# Old\n");
    // A file a crash left, and a file of someone else that happens to have the name.
    let taken = [
        "volumes/guide/.anaquel-staged-000000",
        "volumes/guide/.anaquel-staged-000001",
    ];
    for name in taken {
        fs::write(s.0.join(name), b"someone else's file").unwrap();
    }
    rewrite_page(&s.0.join(PAGE), "# New\n", "# Old\n").unwrap();
    assert_eq!(on_disk(&s), b"# New\n");
    for name in taken {
        assert_eq!(fs::read(s.0.join(name)).unwrap(), b"someone else's file");
    }
    // Only the two foreign files remain beside the page.
    let names: Vec<String> = snapshot(&s.0).into_keys().collect();
    assert_eq!(names.len(), 5, "{names:?}");
}

#[test]
fn when_every_staged_name_is_taken_the_page_is_left_as_it_is() {
    let s = folder("rewrite-all-taken", b"# Old\n");
    for slot in 0..16 {
        let name = format!("volumes/guide/.anaquel-staged-{slot:06}");
        fs::write(s.0.join(name), b"taken").unwrap();
    }
    let before = snapshot(&s.0);
    let error = rewrite_page(&s.0.join(PAGE), "# New\n", "# Old\n").unwrap_err();
    assert!(matches!(error, RewriteError::Failed { .. }), "{error:?}");
    assert_eq!(snapshot(&s.0), before);
}

#[test]
fn the_size_limit_counts_the_line_ends_and_the_byte_order_mark_that_are_written() {
    let limit = usize::try_from(crate::MAX_PAGE_BYTES).unwrap();
    // As `\n` the text is 3 MB; the page has `\r\n`, so it would be written as 6 MB.
    let s = folder("rewrite-limit-crlf", b"a\r\n");
    let before = snapshot(&s.0);
    let doubled = "\n".repeat(3_000_000);
    assert_eq!(
        rewrite_page(&s.0.join(PAGE), &doubled, "a\n"),
        Err(RewriteError::TooLarge)
    );
    assert_eq!(snapshot(&s.0), before);

    // The same text in a page with `\n` line ends is 3 MB and is written.
    let s = folder("rewrite-limit-lf", b"a\n");
    assert_eq!(
        rewrite_page(&s.0.join(PAGE), &doubled, "a\n")
            .unwrap()
            .len(),
        3_000_000
    );

    // The three bytes of the mark are counted: the limit minus three fits, minus two does not.
    let s = folder("rewrite-limit-bom", b"\xEF\xBB\xBFa\n");
    let fits = "a".repeat(limit - 3);
    assert_eq!(
        rewrite_page(&s.0.join(PAGE), &fits, "a\n").unwrap().len(),
        limit
    );
    let s = folder("rewrite-limit-bom-over", b"\xEF\xBB\xBFa\n");
    let over = "a".repeat(limit - 2);
    assert_eq!(
        rewrite_page(&s.0.join(PAGE), &over, "a\n"),
        Err(RewriteError::TooLarge)
    );
    assert_eq!(on_disk(&s), b"\xEF\xBB\xBFa\n");
}

#[test]
fn an_interrupted_write_leaves_the_old_page_at_every_step() {
    let mut failures = 0;
    let mut finished = false;
    for fail_at in 1..10 {
        let s = folder("rewrite-interrupted", b"# Old\n");
        let before = snapshot(&s.0);
        let mut disk = Failing {
            inner: RealDisk,
            fail_at: vec![fail_at],
            count: 0,
        };
        match rewrite_page_on(&mut disk, &s.0.join(PAGE), "# New\n", "# Old\n") {
            Ok(_) => {
                assert_eq!(on_disk(&s), b"# New\n");
                finished = true;
                break;
            }
            Err(error) => {
                failures += 1;
                assert!(matches!(error, RewriteError::Failed { .. }), "{error:?}");
                assert_eq!(snapshot(&s.0), before, "failing at step {fail_at}");
            }
        }
    }
    // Staging, copying the permissions and the rename are three steps, each of which must fail.
    assert_eq!(failures, 3);
    // The loop must reach a step past the last one, or it proved nothing about it.
    assert!(finished);
}

#[test]
fn a_failed_clean_up_names_the_file_it_could_not_remove() {
    let s = folder("rewrite-stuck", b"# Old\n");
    // The rename (third call) fails, and so does the removal of the staged file (fourth call).
    let mut disk = Failing {
        inner: RealDisk,
        fail_at: vec![3, 4],
        count: 0,
    };
    let error = rewrite_page_on(&mut disk, &s.0.join(PAGE), "# New\n", "# Old\n").unwrap_err();
    let RewriteError::Failed { unrestored, .. } = error else {
        panic!("expected a failure with leftovers");
    };
    assert_eq!(unrestored.len(), 1, "{unrestored:?}");
    assert!(unrestored[0].contains("temporary file"), "{unrestored:?}");
    // The page itself is the old one.
    assert_eq!(on_disk(&s), b"# Old\n");
}

#[cfg(unix)]
#[test]
fn the_permission_bits_of_the_page_are_kept() {
    use std::os::unix::fs::PermissionsExt;

    let s = folder("rewrite-mode", b"# Old\n");
    let path = s.0.join(PAGE);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    rewrite_page(&path, "# New\n", "# Old\n").unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
}
