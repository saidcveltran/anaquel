use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(super) trait Disk {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
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
    for change in changes {
        if let Err(error) = run(disk, change, &mut undo) {
            let restored = roll_back(disk, undo);
            return Err(Failure {
                message: error.to_string(),
                restored,
            });
        }
    }
    let mut dirs: Vec<&PathBuf> = emptied.iter().collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for dir in dirs {
        let _ = disk.remove_dir(dir);
    }
    Ok(())
}

fn run(disk: &mut dyn Disk, change: &Change, undo: &mut Vec<Undo>) -> io::Result<()> {
    match change {
        Change::Move { from, to, text } => {
            make_parents(disk, to, undo)?;
            let temporary = temporary_name(to);
            if let Some(text) = text {
                let original = disk.read(from)?;
                disk.write(&temporary, &encode_like(&original, text))?;
                undo.push(Undo::RemoveFile(temporary.clone()));
                disk.remove_file(from)?;
                undo.push(Undo::Restore {
                    path: from.clone(),
                    bytes: original,
                });
                disk.rename(&temporary, to)?;
                undo.push(Undo::Rename {
                    from: to.clone(),
                    to: temporary,
                });
            } else {
                disk.rename(from, &temporary)?;
                undo.push(Undo::Rename {
                    from: temporary.clone(),
                    to: from.clone(),
                });
                disk.rename(&temporary, to)?;
                undo.push(Undo::Rename {
                    from: to.clone(),
                    to: temporary,
                });
            }
        }
        Change::Rewrite { path, text } => {
            let original = disk.read(path)?;
            let temporary = temporary_name(path);
            disk.write(&temporary, &encode_like(&original, text))?;
            if let Err(error) = disk.rename(&temporary, path) {
                let _ = disk.remove_file(&temporary);
                return Err(error);
            }
            undo.push(Undo::Restore {
                path: path.clone(),
                bytes: original,
            });
        }
    }
    Ok(())
}

fn make_parents(disk: &mut dyn Disk, file: &Path, undo: &mut Vec<Undo>) -> io::Result<()> {
    let mut missing: Vec<PathBuf> = Vec::new();
    let mut probe = file.parent();
    while let Some(dir) = probe {
        if dir.as_os_str().is_empty() || dir.exists() {
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

fn temporary_name(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    path.with_file_name(format!(".{name}.anaquel-tmp"))
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
