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
        io::Cursor::new(archive),
        ArchiveKind::TarGz,
        "anaquel",
        &scratch.path("out"),
        limits,
    )
}

/// A zip archive of `entries`; files are deflated.
fn zip_archive(entries: &[Entry<'_>]) -> Vec<u8> {
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut writer = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
    for entry in entries {
        match entry {
            Entry::File(path, data) => {
                writer.start_file(*path, options).unwrap();
                writer.write_all(data).unwrap();
            }
            Entry::Link(path, target) => writer.add_symlink(*path, *target, options).unwrap(),
            Entry::Folder(path) => writer.add_directory(*path, options).unwrap(),
        }
    }
    writer.finish().unwrap().into_inner()
}

fn extract_zip(scratch: &Scratch, archive: &[u8], limits: Limits) -> Result<u64, ArchiveError> {
    extract_program(
        io::Cursor::new(archive),
        ArchiveKind::Zip,
        "anaquel.exe",
        &scratch.path("out"),
        limits,
    )
}

#[test]
fn the_program_is_taken_from_a_zip_and_nothing_else_is_written() {
    let scratch = Scratch::new("zip-folder");
    let archive = zip_archive(&[
        Entry::Folder("anaquel-x86_64-pc-windows-msvc/"),
        Entry::File("anaquel-x86_64-pc-windows-msvc/README.md", b"readme"),
        Entry::File("anaquel-x86_64-pc-windows-msvc/anaquel.exe", PROGRAM),
        Entry::File("anaquel-x86_64-pc-windows-msvc/NOTICE", b"notice"),
    ]);
    assert_eq!(
        extract_zip(&scratch, &archive, small()).unwrap(),
        PROGRAM.len() as u64
    );
    assert_eq!(fs::read(scratch.path("out")).unwrap(), PROGRAM);
    assert_eq!(scratch.names(), ["out"]);
}

#[test]
fn a_zip_with_the_program_at_the_top_is_read_too() {
    let scratch = Scratch::new("zip-top");
    let archive = zip_archive(&[Entry::File("anaquel.exe", PROGRAM)]);
    extract_zip(&scratch, &archive, small()).unwrap();
    assert_eq!(fs::read(scratch.path("out")).unwrap(), PROGRAM);
}

#[test]
fn a_zip_without_the_program_two_programs_or_a_special_entry_is_refused() {
    let scratch = Scratch::new("zip-refused");
    let missing = [
        vec![],
        vec![Entry::File("dir/README.md", b"x")],
        vec![Entry::File("a/b/anaquel.exe", PROGRAM)],
        vec![Entry::File("anaquel", PROGRAM)],
    ];
    for entries in missing {
        let error = extract_zip(&scratch, &zip_archive(&entries), small()).unwrap_err();
        assert!(matches!(error, ArchiveError::ProgramMissing(_)), "{error}");
        scratch.assert_empty();
    }

    let twice = zip_archive(&[
        Entry::File("anaquel.exe", PROGRAM),
        Entry::File("dir/anaquel.exe", b"another"),
    ]);
    let error = extract_zip(&scratch, &twice, small()).unwrap_err();
    assert!(matches!(error, ArchiveError::ProgramRepeated(_)), "{error}");
    scratch.assert_empty();

    for entry in [
        Entry::Link("anaquel.exe", "C:/Windows/System32/cmd.exe"),
        Entry::Folder("dir/anaquel.exe/"),
    ] {
        let error = extract_zip(&scratch, &zip_archive(&[entry]), small()).unwrap_err();
        assert!(matches!(error, ArchiveError::NotAPlainFile(_)), "{error}");
        scratch.assert_empty();
    }
}

#[test]
fn a_zip_program_larger_than_the_limit_is_cut_off_even_when_it_compresses_to_nothing() {
    let limits = Limits {
        program_bytes: 10,
        archive_bytes: 1024 * 1024,
    };
    let scratch = Scratch::new("zip-size");
    let exactly = zip_archive(&[Entry::File("anaquel.exe", &[7; 10])]);
    assert_eq!(extract_zip(&scratch, &exactly, limits).unwrap(), 10);

    let scratch = Scratch::new("zip-over");
    // 300 KiB of zeros deflate to a few hundred bytes: the cap must stop the read, not the size
    // of the archive file.
    let over = zip_archive(&[Entry::File("anaquel.exe", &vec![0; 300 * 1024])]);
    assert!(
        over.len() < 2048,
        "the test needs a highly compressed entry"
    );
    assert!(matches!(
        extract_zip(&scratch, &over, limits),
        Err(ArchiveError::BadProgramSize { limit: 10 })
    ));
    scratch.assert_empty();

    let scratch = Scratch::new("zip-empty");
    let empty = zip_archive(&[Entry::File("anaquel.exe", b"")]);
    assert!(matches!(
        extract_zip(&scratch, &empty, limits),
        Err(ArchiveError::BadProgramSize { .. })
    ));
    scratch.assert_empty();
}

#[test]
fn a_zip_whose_files_declare_more_than_the_limit_is_refused_before_anything_is_written() {
    let limits = Limits {
        program_bytes: 1024,
        archive_bytes: 16 * 1024,
    };
    let scratch = Scratch::new("zip-total");
    let archive = zip_archive(&[
        Entry::File("anaquel.exe", PROGRAM),
        Entry::File("filler", &vec![0; 64 * 1024]),
    ]);
    assert!(matches!(
        extract_zip(&scratch, &archive, limits),
        Err(ArchiveError::ArchiveTooLarge { limit: 16384 })
    ));
    scratch.assert_empty();
}

#[test]
fn the_declared_total_of_a_zip_may_equal_the_limit_and_no_more() {
    let archive = zip_archive(&[Entry::File("anaquel.exe", PROGRAM)]);
    let size = PROGRAM.len() as u64;
    let scratch = Scratch::new("zip-exact");
    let exact = Limits {
        program_bytes: 1024,
        archive_bytes: size,
    };
    assert!(extract_zip(&scratch, &archive, exact).is_ok());
    let scratch = Scratch::new("zip-one-less");
    let one_less = Limits {
        archive_bytes: size - 1,
        ..exact
    };
    assert!(matches!(
        extract_zip(&scratch, &archive, one_less),
        Err(ArchiveError::ArchiveTooLarge { .. })
    ));
}

#[test]
fn what_is_not_a_zip_and_every_truncation_of_one_is_refused() {
    let scratch = Scratch::new("zip-broken");
    for bytes in [&b""[..], b"plain text", b"PK\x03\x04"] {
        let error = extract_zip(&scratch, bytes, small()).unwrap_err();
        assert!(matches!(error, ArchiveError::Unreadable(_)), "{error}");
        scratch.assert_empty();
    }
    // A zip lists its files at the end, so a cut anywhere loses the list.
    let archive = zip_archive(&[
        Entry::File("dir/README.md", &[1; 3000]),
        Entry::File("dir/anaquel.exe", &[9; 900]),
    ]);
    for cut in (0..archive.len()).step_by(5) {
        let scratch = Scratch::new("zip-cut");
        assert!(
            extract_zip(&scratch, &archive[..cut], small()).is_err(),
            "cut at {cut}"
        );
        scratch.assert_empty();
    }
    // A damaged compressed stream is found while it is read (the stored checksum or the
    // deflate stream no longer fits), and the half-written program is removed. The file data
    // starts after the 30-byte local header and the 11-byte name `anaquel.exe`.
    let mut damaged = zip_archive(&[Entry::File("anaquel.exe", &[5; 600])]);
    damaged[30 + 11] ^= 0xff;
    let scratch = Scratch::new("zip-damaged");
    assert!(extract_zip(&scratch, &damaged, small()).is_err());
    scratch.assert_empty();
}

#[test]
fn an_existing_file_at_the_destination_is_never_replaced_by_a_zip() {
    let scratch = Scratch::new("zip-exists");
    fs::write(scratch.path("out"), b"keep me").unwrap();
    let archive = zip_archive(&[Entry::File("anaquel.exe", PROGRAM)]);
    let error = extract_zip(&scratch, &archive, small()).unwrap_err();
    assert!(matches!(error, ArchiveError::Destination { .. }), "{error}");
    assert_eq!(fs::read(scratch.path("out")).unwrap(), b"keep me");
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
    assert_eq!(
        ArchiveKind::of_file_name("anaquel-x86_64-pc-windows-msvc.zip"),
        Some(ArchiveKind::Zip)
    );
    for name in [
        "anaquel.tar.xz",
        "anaquel.tar",
        "tar.gz",
        "zip",
        "anaquel.tar.gz.sha256",
        "anaquel.zip.sha256",
        "anaquel.7z",
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
