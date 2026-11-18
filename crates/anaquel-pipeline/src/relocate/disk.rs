use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(super) trait Disk {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn exists(&self, path: &Path) -> bool;
    fn write(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&mut self, path: &Path) -> io::Result<()>;
    fn create_dir(&mut self, path: &Path) -> io::Result<()>;
    fn remove_dir(&mut self, path: &Path) -> io::Result<()>;
}

pub(super) struct RealDisk;

impl Disk for RealDisk {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn exists(&self, path: &Path) -> bool {
        fs::symlink_metadata(path).is_ok()
    }

    fn write(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        fs::write(path, bytes)
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_file(&mut self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn create_dir(&mut self, path: &Path) -> io::Result<()> {
        fs::create_dir(path)
    }

    fn remove_dir(&mut self, path: &Path) -> io::Result<()> {
        fs::remove_dir(path)
    }
}

pub(super) enum Change {
    Move {
        from: PathBuf,
        to: PathBuf,
        text: Option<String>,
    },
    Rewrite {
        path: PathBuf,
        text: String,
    },
}

enum Undo {
    RemoveFile(PathBuf),
    RemoveDir(PathBuf),
    Rename { from: PathBuf, to: PathBuf },
    Restore { path: PathBuf, bytes: Vec<u8> },
}

pub(super) struct Failure {
    pub(super) message: String,
    pub(super) restored: bool,
}

pub(super) fn apply(
    disk: &mut dyn Disk,
    changes: &[Change],
    emptied: &[PathBuf],
) -> Result<(), Failure> {
    let mut undo: Vec<Undo> = Vec::new();
    if let Err(error) = steps(disk, changes, &mut undo) {
        let restored = roll_back(disk, undo);
        return Err(Failure {
            message: error.to_string(),
            restored,
        });
    }
    let mut dirs: Vec<&PathBuf> = emptied.iter().collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for dir in dirs {
        let _ = disk.remove_dir(dir);
    }
    Ok(())
}

fn steps(disk: &mut dyn Disk, changes: &[Change], undo: &mut Vec<Undo>) -> io::Result<()> {
    let mut originals: Vec<Option<Vec<u8>>> = Vec::with_capacity(changes.len());
    for change in changes {
        let Change::Move { from, to, text } = change else {
            originals.push(None);
            continue;
        };
        make_parents(disk, to, undo)?;
        if let Some(text) = text {
            let original = disk.read(from)?;
            write_temporary(disk, &temporary_name(to), &encode_like(&original, text))?;
            undo.push(Undo::RemoveFile(temporary_name(to)));
            originals.push(Some(original));
        } else {
            let held = temporary_name(from);
            disk.rename(from, &held)?;
            undo.push(Undo::Rename {
                from: held,
                to: from.clone(),
            });
            originals.push(None);
        }
    }
    for change in changes {
        let Change::Move { from, to, text } = change else {
            continue;
        };
        let held = if text.is_some() {
            temporary_name(to)
        } else {
            temporary_name(from)
        };
        if disk.exists(to) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists", to.display()),
            ));
        }
        disk.rename(&held, to)?;
        undo.push(Undo::Rename {
            from: to.clone(),
            to: held,
        });
    }
    for (change, original) in changes.iter().zip(originals) {
        if let (Change::Move { from, .. }, Some(bytes)) = (change, original) {
            disk.remove_file(from)?;
            undo.push(Undo::Restore {
                path: from.clone(),
                bytes,
            });
        }
    }
    for change in changes {
        let Change::Rewrite { path, text } = change else {
            continue;
        };
        let original = disk.read(path)?;
        let temporary = temporary_name(path);
        write_temporary(disk, &temporary, &encode_like(&original, text))?;
        if let Err(error) = disk.rename(&temporary, path) {
            let _ = disk.remove_file(&temporary);
            return Err(error);
        }
        undo.push(Undo::Restore {
            path: path.clone(),
            bytes: original,
        });
    }
    Ok(())
}

fn write_temporary(disk: &mut dyn Disk, path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Err(error) = disk.write(path, bytes) {
        let _ = disk.remove_file(path);
        return Err(error);
    }
    Ok(())
}

fn make_parents(disk: &mut dyn Disk, file: &Path, undo: &mut Vec<Undo>) -> io::Result<()> {
    let mut missing: Vec<PathBuf> = Vec::new();
    let mut probe = file.parent();
    while let Some(dir) = probe {
        if dir.as_os_str().is_empty() || disk.exists(dir) {
            break;
        }
        missing.push(dir.to_path_buf());
        probe = dir.parent();
    }
    for dir in missing.into_iter().rev() {
        disk.create_dir(&dir)?;
        undo.push(Undo::RemoveDir(dir));
    }
    Ok(())
}

pub(super) const TEMPORARY_EXTRA: usize = ".".len() + ".anaquel-tmp".len();

fn temporary_name(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    path.with_file_name(format!(".{name}.anaquel-tmp"))
}

pub(super) fn mixed_line_endings(bytes: &[u8]) -> bool {
    let crlf = bytes.windows(2).filter(|pair| pair == b"\r\n").count();
    let lf = bytes.split(|&b| b == b'\n').count() - 1;
    crlf > 0 && lf > crlf
}

fn encode_like(original: &[u8], text: &str) -> Vec<u8> {
    let crlf = original.windows(2).any(|pair| pair == b"\r\n");
    let bom = original.starts_with(&[0xEF, 0xBB, 0xBF]);
    let mut bytes = Vec::with_capacity(text.len() + 3);
    if bom {
        bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    if crlf {
        bytes.extend_from_slice(text.replace('\n', "\r\n").as_bytes());
    } else {
        bytes.extend_from_slice(text.as_bytes());
    }
    bytes
}

fn roll_back(disk: &mut dyn Disk, undo: Vec<Undo>) -> bool {
    let mut clean = true;
    for step in undo.into_iter().rev() {
        let result = match step {
            Undo::RemoveFile(path) => disk.remove_file(&path),
            Undo::RemoveDir(path) => disk.remove_dir(&path),
            Undo::Rename { from, to } => disk.rename(&from, &to),
            Undo::Restore { path, bytes } => disk.write(&path, &bytes),
        };
        if result.is_err() {
            clean = false;
        }
    }
    clean
}
