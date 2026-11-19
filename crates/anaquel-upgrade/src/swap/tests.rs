use super::*;
use crate::scratch::Scratch;

/// A folder with an installed program holding `installed`, and a staged one holding `staged`.
fn folder(name: &str, installed: &[u8], staged: &[u8]) -> (Scratch, PathBuf, PathBuf) {
    let scratch = Scratch::new(name);
    let program = scratch.path("anaquel");
    fs::write(&program, installed).unwrap();
    let new = staged_path(&program);
    fs::write(&new, staged).unwrap();
    (scratch, program, new)
}

fn text(path: &Path) -> String {
    String::from_utf8(fs::read(path).unwrap()).unwrap()
}

/// A rename that works the first `succeed` times and fails after, counting its calls.
fn failing_after(succeed: usize) -> impl FnMut(&Path, &Path) -> io::Result<()> {
    let mut calls = 0;
    move |from, to| {
        calls += 1;
        if calls <= succeed {
            fs::rename(from, to)
        } else {
            Err(io::Error::other("refused by the test"))
        }
    }
}

#[test]
fn the_new_program_takes_the_place_and_the_old_one_is_kept() {
    let (scratch, program, staged) = folder("install", b"old", b"new");
    install(&program, &staged).unwrap();
    assert_eq!(text(&program), "new");
    assert_eq!(text(&previous_path(&program)), "old");
    assert_eq!(scratch.names(), ["anaquel", "anaquel.previous"]);
}

#[test]
fn only_one_previous_program_is_kept() {
    let (scratch, program, staged) = folder("one-kept", b"v1", b"v2");
    install(&program, &staged).unwrap();
    fs::write(&staged, b"v3").unwrap();
    install(&program, &staged).unwrap();
    assert_eq!(text(&program), "v3");
    assert_eq!(text(&previous_path(&program)), "v2");
    assert_eq!(scratch.names(), ["anaquel", "anaquel.previous"]);
}

#[test]
fn a_missing_new_program_changes_nothing() {
    let (scratch, program, staged) = folder("no-staged", b"old", b"new");
    fs::remove_file(&staged).unwrap();
    assert!(matches!(
        install(&program, &staged),
        Err(SwapError::NothingStaged(_))
    ));
    assert_eq!(text(&program), "old");
    assert_eq!(scratch.names(), ["anaquel"]);

    // A folder is not a program.
    fs::create_dir(&staged).unwrap();
    assert!(matches!(
        install(&program, &staged),
        Err(SwapError::NothingStaged(_))
    ));
    assert_eq!(text(&program), "old");
}

#[test]
fn a_failure_while_moving_the_old_program_aside_changes_nothing() {
    let (scratch, program, staged) = folder("first-fails", b"old", b"new");
    let error = install_with(&program, &staged, &mut failing_after(0)).unwrap_err();
    assert!(
        matches!(
            error,
            SwapError::Step {
                action: "move aside",
                ..
            }
        ),
        "{error}"
    );
    assert_eq!(text(&program), "old");
    assert_eq!(text(&staged), "new");
    assert_eq!(scratch.names(), ["anaquel", "anaquel.new"]);
}

#[test]
fn a_failure_while_putting_the_new_program_in_place_puts_the_old_one_back() {
    let (scratch, program, staged) = folder("second-fails", b"old", b"new");
    // The first rename works, the second fails, the undo (the third) works again.
    let mut calls = 0;
    let mut rename = |from: &Path, to: &Path| {
        calls += 1;
        if calls == 2 {
            Err(io::Error::other("refused by the test"))
        } else {
            fs::rename(from, to)
        }
    };
    let error = install_with(&program, &staged, &mut rename).unwrap_err();
    assert!(
        matches!(
            error,
            SwapError::Step {
                action: "put in place",
                ..
            }
        ),
        "{error}"
    );
    assert_eq!(text(&program), "old", "the installed program is as it was");
    assert_eq!(text(&staged), "new");
    assert_eq!(scratch.names(), ["anaquel", "anaquel.new"]);
}

#[test]
fn when_the_undo_fails_too_the_error_says_where_the_old_program_is() {
    let (scratch, program, staged) = folder("stranded", b"old", b"new");
    let error = install_with(&program, &staged, &mut failing_after(1)).unwrap_err();
    match &error {
        SwapError::Stranded { moved_to, .. } => assert_eq!(moved_to, &previous_path(&program)),
        other => panic!("{other}"),
    }
    assert!(error.to_string().contains("anaquel.previous"), "{error}");
    assert_eq!(text(&previous_path(&program)), "old");
    assert_eq!(scratch.names(), ["anaquel.new", "anaquel.previous"]);
}

#[test]
fn a_rollback_brings_back_the_previous_program_and_keeps_the_newer_as_previous() {
    let (_scratch, program, staged) = folder("rollback", b"old", b"new");
    install(&program, &staged).unwrap();

    rollback(&program).unwrap();
    assert_eq!(text(&program), "old");
    assert_eq!(text(&previous_path(&program)), "new");

    // A second rollback goes forward again.
    rollback(&program).unwrap();
    assert_eq!(text(&program), "new");
    assert_eq!(text(&previous_path(&program)), "old");
}

#[test]
fn a_rollback_without_a_previous_program_changes_nothing() {
    let scratch = Scratch::new("no-previous");
    let program = scratch.path("anaquel");
    fs::write(&program, b"only").unwrap();
    assert!(matches!(
        rollback(&program),
        Err(SwapError::NothingToRestore(_))
    ));
    assert_eq!(text(&program), "only");
    assert_eq!(scratch.names(), ["anaquel"]);
}

#[test]
fn a_failed_rollback_leaves_the_installed_program_where_it_was() {
    let (scratch, program, staged) = folder("rollback-fails", b"old", b"new");
    install(&program, &staged).unwrap();

    // The put-aside works, the restore fails, the undo works.
    let mut calls = 0;
    let mut rename = |from: &Path, to: &Path| {
        calls += 1;
        if calls == 2 {
            Err(io::Error::other("refused by the test"))
        } else {
            fs::rename(from, to)
        }
    };
    let error = rollback_with(&program, &mut rename).unwrap_err();
    assert!(
        matches!(
            error,
            SwapError::Step {
                action: "restore",
                ..
            }
        ),
        "{error}"
    );
    assert_eq!(text(&program), "new");
    assert_eq!(text(&previous_path(&program)), "old");
    assert_eq!(scratch.names(), ["anaquel", "anaquel.previous"]);
}

#[test]
fn a_rollback_that_cannot_keep_the_newer_program_still_goes_back() {
    let (scratch, program, staged) = folder("rollback-last", b"old", b"new");
    install(&program, &staged).unwrap();

    // The first two renames work; the last one, which keeps the newer program, fails.
    let mut rename = failing_after(2);
    rollback_with(&program, &mut rename).unwrap();
    assert_eq!(text(&program), "old");
    assert_eq!(scratch.names(), ["anaquel"], "no leftover file");
}

#[test]
fn a_previous_path_that_cannot_be_cleared_stops_the_swap_before_anything_moves() {
    let (scratch, program, staged) = folder("blocked", b"old", b"new");
    // A folder where the previous program should be cannot be removed as a file.
    fs::create_dir(previous_path(&program)).unwrap();
    let error = install(&program, &staged).unwrap_err();
    assert!(
        matches!(
            error,
            SwapError::Step {
                action: "remove",
                ..
            }
        ),
        "{error}"
    );
    assert_eq!(text(&program), "old");
    assert_eq!(text(&staged), "new");
    assert_eq!(
        scratch.names(),
        ["anaquel", "anaquel.new", "anaquel.previous"]
    );
}

#[test]
fn an_aside_path_that_cannot_be_cleared_stops_a_rollback_before_anything_moves() {
    let (scratch, program, staged) = folder("blocked-aside", b"old", b"new");
    install(&program, &staged).unwrap();
    fs::create_dir(scratch.path("anaquel.swap")).unwrap();
    let error = rollback(&program).unwrap_err();
    assert!(
        matches!(
            error,
            SwapError::Step {
                action: "remove",
                ..
            }
        ),
        "{error}"
    );
    assert_eq!(text(&program), "new");
    assert_eq!(text(&previous_path(&program)), "old");
}

#[test]
fn a_leftover_aside_file_from_a_crashed_rollback_does_not_block_the_next_one() {
    let (scratch, program, staged) = folder("leftover", b"old", b"new");
    install(&program, &staged).unwrap();
    fs::write(scratch.path("anaquel.swap"), b"leftover").unwrap();
    rollback(&program).unwrap();
    assert_eq!(text(&program), "old");
    assert_eq!(scratch.names(), ["anaquel", "anaquel.previous"]);
}

#[test]
fn a_writable_folder_passes_and_leaves_nothing_behind() {
    let scratch = Scratch::new("writable");
    let program = scratch.path("anaquel");
    fs::write(&program, b"x").unwrap();
    check_writable(&program).unwrap();
    assert_eq!(scratch.names(), ["anaquel"]);
}

#[test]
fn a_folder_that_does_not_exist_is_not_writable() {
    let scratch = Scratch::new("no-folder");
    let program = scratch.path("absent").join("anaquel");
    assert!(matches!(
        check_writable(&program),
        Err(SwapError::NotWritable { .. })
    ));
}

#[cfg(unix)]
#[test]
fn a_read_only_folder_is_not_writable() {
    use std::os::unix::fs::PermissionsExt as _;
    let scratch = Scratch::new("read-only");
    let program = scratch.path("anaquel");
    fs::write(&program, b"x").unwrap();
    fs::set_permissions(scratch.root(), fs::Permissions::from_mode(0o555)).unwrap();
    let result = check_writable(&program);
    fs::set_permissions(scratch.root(), fs::Permissions::from_mode(0o755)).unwrap();
    // A superuser can write anywhere, so the check only holds for an ordinary user.
    if std::env::var_os("USER").is_none_or(|user| user != "root") {
        assert!(matches!(result, Err(SwapError::NotWritable { .. })));
    }
}

#[test]
fn the_owner_of_a_program_is_told_from_where_it_is() {
    for (path, expected) in [
        (
            r"C:\Users\x\scoop\apps\anaquel\current\anaquel.exe",
            Some(PackageManager::Scoop),
        ),
        (
            r"C:\Users\x\Scoop\Apps\anaquel\0.2.0\anaquel.exe",
            Some(PackageManager::Scoop),
        ),
        (
            r"C:\Users\x\AppData\Local\Microsoft\WinGet\Packages\Said.Anaquel_x\anaquel.exe",
            Some(PackageManager::Winget),
        ),
        (
            r"C:\Users\x\AppData\Local\Microsoft\WinGet\Links\anaquel.exe",
            Some(PackageManager::Winget),
        ),
        (r"C:\Users\x\.cargo\bin\anaquel.exe", None),
        (r"C:\tools\anaquel.exe", None),
        (r"C:\bin\anaquel.exe", None),
        (r"C:\Users\x\scoop\shims\anaquel.exe", None),
    ] {
        // On Linux a backslash is part of a name, so these Windows paths are tested there as
        // names that happen to have no separators; the folder rules only apply on Windows.
        if cfg!(windows) {
            assert_eq!(managed_by(Path::new(path)), expected, "{path}");
        }
    }
    for (path, expected) in [
        ("/usr/bin/anaquel", Some(PackageManager::System)),
        ("/bin/anaquel", Some(PackageManager::System)),
        ("/usr/local/bin/anaquel", None),
        ("/home/x/.local/bin/anaquel", None),
        (
            "/home/x/scoop/apps/anaquel/anaquel",
            Some(PackageManager::Scoop),
        ),
        ("/opt/usr/bin/anaquel", None),
        ("/usr/binary/anaquel", None),
    ] {
        assert_eq!(managed_by(Path::new(path)), expected, "{path}");
    }
}

#[test]
fn the_name_of_each_package_manager_says_how_to_update_with_it() {
    assert!(PackageManager::Scoop.to_string().contains("scoop update"));
    assert!(
        PackageManager::Winget
            .to_string()
            .contains("winget upgrade")
    );
    assert!(
        PackageManager::System
            .to_string()
            .contains("package manager")
    );
}
