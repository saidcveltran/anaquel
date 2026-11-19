//! Release archives made in memory, for the tests of this crate: the same layouts `dist` writes,
//! and the hostile ones the reader must refuse.

use std::io::{self, Write};

use flate2::Compression;
use flate2::write::GzEncoder;

/// What an archive entry is made of.
pub enum Entry<'a> {
    /// A plain file: its path inside the archive and its bytes.
    File(&'a str, &'a [u8]),
    /// A symbolic link: its path and what it points at.
    Link(&'a str, &'a str),
    /// A folder.
    Folder(&'a str),
}

/// A `.tar.gz` archive of `entries`.
pub fn tar_gz(entries: &[Entry<'_>]) -> Vec<u8> {
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

/// A `.zip` archive of `entries`; files are deflated.
pub fn zip_archive(entries: &[Entry<'_>]) -> Vec<u8> {
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
