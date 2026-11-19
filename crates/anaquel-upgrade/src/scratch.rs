//! A scratch folder for the tests of this crate, removed when the test ends.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A new empty folder under the system's temporary folder.
pub struct Scratch(PathBuf);

impl Scratch {
    /// Makes the folder; `name` says which test it is for.
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "anaquel-upgrade-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// The folder itself.
    pub fn root(&self) -> &Path {
        &self.0
    }

    /// A path inside the folder.
    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Fails the test, listing what is there, unless the folder holds nothing.
    pub fn assert_empty(&self) {
        assert_eq!(
            self.names(),
            Vec::<String>::new(),
            "the folder must be empty"
        );
    }

    /// The names of the files in the folder, sorted.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
