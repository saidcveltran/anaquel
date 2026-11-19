use flate2::Compression;
use flate2::write::GzEncoder;

use super::*;
use crate::scratch::Scratch;

const PROGRAM: &[u8] = b"#!program-bytes";

/// What an archive entry is made of.
enum Entry<'a> {
    File(&'a str, &'a [u8]),
    Link(&'a str, &'a str),
    Folder(&'a str),
}

fn tar_gz(entries: &[Entry<'_>]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for entry in entries {
        let mut header = tar::Header::new_gnu();
        match entry {
            Entry::File(path, data) => {
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                builder.append_data(&mut header, path, *data).unwrap();
            }
            Entry::Link(path, target) => {
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                builder.append_link(&mut header, path, target).unwrap();
            }
            Entry::Folder(path) => {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
                header.set_mode(0o755);
                builder.append_data(&mut header, path, &[][..]).unwrap();
            }
        }
    }
    let tar_bytes = builder.into_inner().unwrap();
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&tar_bytes).unwrap();
    encoder.finish().unwrap()
}

fn small() -> Limits {
    Limits {
        program_bytes: 1024,
        archive_bytes: 64 * 1024,
    }
}

fn extract(scratch: &Scratch, archive: &[u8], limits: Limits) -> Result<u64, ArchiveError> {
    extract_program(
        archive,
        ArchiveKind::TarGz,
        "anaquel",
        &scratch.path("out"),
        limits,
    )
}

#[test]
fn the_program_is_taken_from_a_folder_of_the_archive_and_nothing_else_is_written() {
    let scratch = Scratch::new("folder");
    let archive = tar_gz(&[
        Entry::Folder("anaquel-x86_64-unknown-linux-gnu/"),
        Entry::File("anaquel-x86_64-unknown-linux-gnu/README.md", b"readme"),
        Entry::File("anaquel-x86_64-unknown-linux-gnu/anaquel", PROGRAM),
        Entry::File("anaquel-x86_64-unknown-linux-gnu/NOTICE", b"notice"),
    ]);
    let size = extract(&scratch, &archive, small()).unwrap();
    assert_eq!(size, PROGRAM.len() as u64);
    assert_eq!(fs::read(scratch.path("out")).unwrap(), PROGRAM);
    assert_eq!(scratch.names(), ["out"]);
}

#[test]
fn the_program_is_found_at_the_top_and_with_a_leading_dot_slash() {
    for path in ["anaquel", "./anaquel"] {
        let scratch = Scratch::new("top");
        let archive = tar_gz(&[Entry::File(path, PROGRAM)]);
        extract(&scratch, &archive, small()).unwrap();
        assert_eq!(fs::read(scratch.path("out")).unwrap(), PROGRAM, "{path}");
    }
}

#[cfg(unix)]
#[test]
fn the_program_is_executable_by_its_owner() {
    use std::os::unix::fs::PermissionsExt as _;
    let scratch = Scratch::new("mode");
    extract(
        &scratch,
        &tar_gz(&[Entry::File("anaquel", PROGRAM)]),
        small(),
    )
    .unwrap();
    let mode = fs::metadata(scratch.path("out"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o100, 0o100);
}

#[test]
fn which_entries_name_the_program() {
    for (path, expected) in [
        ("anaquel", true),
        ("dir/anaquel", true),
        ("./anaquel", true),
        ("./dir/anaquel", true),
        ("a/b/anaquel", false),
        ("anaquel/other", false),
        ("anaquel.exe", false),
        ("dir/anaquel-extra", false),
        ("../anaquel", false),
        ("dir/../anaquel", false),
        ("/anaquel", false),
        ("", false),
    ] {
        assert_eq!(
            names_the_program(Path::new(path), "anaquel"),
            expected,
            "{path:?}"
        );
    }
}

#[test]
fn an_archive_without_the_program_is_refused_and_leaves_nothing() {
    let scratch = Scratch::new("missing");
    for entries in [
        vec![],
        vec![Entry::File("dir/README.md", b"x")],
        vec![Entry::File("a/b/anaquel", PROGRAM)],
        vec![Entry::File("anaquel.exe", PROGRAM)],
    ] {
        let error = extract(&scratch, &tar_gz(&entries), small()).unwrap_err();
        assert!(matches!(error, ArchiveError::ProgramMissing(_)), "{error}");
        scratch.assert_empty();
    }
}

#[test]
fn two_programs_in_one_archive_are_refused_and_the_first_is_removed() {
    let scratch = Scratch::new("twice");
    let archive = tar_gz(&[
        Entry::File("anaquel", PROGRAM),
        Entry::File("dir/anaquel", b"another"),
    ]);
    let error = extract(&scratch, &archive, small()).unwrap_err();
    assert!(matches!(error, ArchiveError::ProgramRepeated(_)), "{error}");
    assert!(scratch.names().is_empty(), "{:?}", scratch.names());
}

#[test]
fn a_link_or_a_folder_with_the_programs_name_is_refused() {
    let scratch = Scratch::new("special");
    for entry in [
        Entry::Link("anaquel", "/bin/sh"),
        Entry::Link("dir/anaquel", "../elsewhere"),
        Entry::Folder("dir/anaquel/"),
    ] {
        let error = extract(&scratch, &tar_gz(&[entry]), small()).unwrap_err();
        assert!(matches!(error, ArchiveError::NotAPlainFile(_)), "{error}");
        scratch.assert_empty();
    }
}

#[test]
fn the_size_limit_of_the_program_is_inclusive_and_an_empty_program_is_refused() {
    let limits = Limits {
        program_bytes: 10,
        archive_bytes: 64 * 1024,
    };
    let scratch = Scratch::new("size");
    let exactly = tar_gz(&[Entry::File("anaquel", &[7; 10])]);
    assert_eq!(extract(&scratch, &exactly, limits).unwrap(), 10);

    let scratch = Scratch::new("size-over");
    let over = tar_gz(&[Entry::File("anaquel", &[7; 11])]);
    assert!(matches!(
        extract(&scratch, &over, limits),
        Err(ArchiveError::BadProgramSize { limit: 10 })
    ));
    scratch.assert_empty();

    let scratch = Scratch::new("size-empty");
    let empty = tar_gz(&[Entry::File("anaquel", b"")]);
    assert!(matches!(
        extract(&scratch, &empty, limits),
        Err(ArchiveError::BadProgramSize { .. })
    ));
    scratch.assert_empty();
}

#[test]
fn an_archive_that_expands_beyond_the_limit_is_refused_wherever_the_bulk_is() {
    let filler = vec![0_u8; 256 * 1024];
    let limits = Limits {
        program_bytes: 1024,
        archive_bytes: 16 * 1024,
    };
    // The bulk after the program, and the bulk before it.
    for entries in [
        vec![
            Entry::File("anaquel", PROGRAM),
            Entry::File("filler", &filler),
        ],
        vec![
            Entry::File("filler", &filler),
            Entry::File("anaquel", PROGRAM),
        ],
    ] {
        let scratch = Scratch::new("bomb");
        let error = extract(&scratch, &tar_gz(&entries), limits).unwrap_err();
        assert!(
            matches!(error, ArchiveError::ArchiveTooLarge { limit: 16384 }),
            "{error}"
        );
        assert!(scratch.names().is_empty(), "{:?}", scratch.names());
    }
}

#[test]
fn an_archive_exactly_at_the_limit_is_accepted() {
    // The reader consumes a header block, the data padded to a block and the first all-zero
    // block that ends the archive: 512 + 512 + 512 bytes for a small program.
    const CONSUMED: u64 = 1536;
    let archive = tar_gz(&[Entry::File("anaquel", PROGRAM)]);
    let exact = Limits {
        program_bytes: 1024,
        archive_bytes: CONSUMED,
    };
    let scratch = Scratch::new("exact");
    assert!(extract(&scratch, &archive, exact).is_ok());

    let one_less = Limits {
        archive_bytes: CONSUMED - 1,
        ..exact
    };
    let scratch = Scratch::new("one-less");
    assert!(matches!(
        extract(&scratch, &archive, one_less),
        Err(ArchiveError::ArchiveTooLarge { .. })
    ));
}

#[test]
fn what_is_not_a_gzip_archive_is_refused() {
    let scratch = Scratch::new("not-gzip");
    for bytes in [&b""[..], b"plain text", &[0x1f, 0x8b, 0, 0][..]] {
        let error = extract(&scratch, bytes, small()).unwrap_err();
        assert!(matches!(error, ArchiveError::Unreadable(_)), "{error}");
        scratch.assert_empty();
    }
}

#[test]
fn a_truncated_archive_never_leaves_a_program_behind() {
    let archive = tar_gz(&[
        Entry::File("dir/README.md", &[1; 3000]),
        Entry::File("dir/anaquel", &[9; 900]),
        Entry::File("dir/NOTICE", &[2; 3000]),
    ]);
    let mut refused = 0;
    for cut in (0..archive.len()).step_by(7) {
        let scratch = Scratch::new("truncated");
        if let Ok(size) = extract(&scratch, &archive[..cut], small()) {
            // Only a cut inside the trailing padding or checksum can still give a whole file.
            assert_eq!(size, 900);
            assert_eq!(fs::read(scratch.path("out")).unwrap(), vec![9; 900]);
        } else {
            refused += 1;
            assert_eq!(scratch.names(), Vec::<String>::new(), "cut at {cut}");
        }
    }
    assert!(refused > archive.len() / 7 / 2, "most cuts must be refused");
}

#[test]
fn an_existing_file_at_the_destination_is_never_replaced() {
    let scratch = Scratch::new("exists");
    fs::write(scratch.path("out"), b"keep me").unwrap();
    let error = extract(
        &scratch,
        &tar_gz(&[Entry::File("anaquel", PROGRAM)]),
        small(),
    )
    .unwrap_err();
    assert!(matches!(error, ArchiveError::Destination { .. }), "{error}");
    assert_eq!(fs::read(scratch.path("out")).unwrap(), b"keep me");

    // Also when a second program makes the archive fail after the destination was refused.
    let twice = tar_gz(&[
        Entry::File("anaquel", PROGRAM),
        Entry::File("dir/anaquel", PROGRAM),
    ]);
    assert!(extract(&scratch, &twice, small()).is_err());
    assert_eq!(fs::read(scratch.path("out")).unwrap(), b"keep me");
}

#[test]
fn the_kind_of_an_archive_comes_from_its_name() {
    assert_eq!(
        ArchiveKind::of_file_name("anaquel-x86_64-unknown-linux-gnu.tar.gz"),
        Some(ArchiveKind::TarGz)
    );
    for name in [
        "anaquel-x86_64-pc-windows-msvc.zip",
        "anaquel.tar.xz",
        "anaquel.tar",
        "tar.gz",
        "anaquel.tar.gz.sha256",
        "",
    ] {
        // `tar.gz` alone has no dot before `tar`, so it is not a `.tar.gz`.
        assert_eq!(ArchiveKind::of_file_name(name), None, "{name:?}");
    }
}

#[test]
fn the_release_limits_follow_the_largest_asset_the_manifest_allows() {
    let limits = Limits::for_release();
    assert_eq!(limits.program_bytes, anaquel_update::MAX_ASSET_BYTES);
    assert_eq!(limits.archive_bytes, 2 * anaquel_update::MAX_ASSET_BYTES);
}
