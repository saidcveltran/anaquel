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
fn a_staged_name_that_is_taken_is_never_written_through() {
    let s = folder("rewrite-taken", b"# Old\n");
    let taken = s.0.join("volumes/guide/.anaquel-staged-000000");
    fs::write(&taken, b"someone else's file").unwrap();
    let before = snapshot(&s.0);
    let error = rewrite_page(&s.0.join(PAGE), "# New\n", "# Old\n").unwrap_err();
    assert!(matches!(error, RewriteError::Failed { .. }), "{error:?}");
    assert_eq!(snapshot(&s.0), before);
    assert_eq!(fs::read(taken).unwrap(), b"someone else's file");
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
            Ok(()) => {
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
