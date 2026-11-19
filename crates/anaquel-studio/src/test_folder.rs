//! A temporary library folder for the tests, removed when it goes out of scope.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A folder under the system temporary folder, unique to the process and to the call.
pub(crate) struct TestFolder(PathBuf);

impl TestFolder {
    pub(crate) fn new(label: &str) -> Self {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "anaquel-studio-{label}-{}-{serial}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("the test folder can be created");
        Self(dir)
    }

    pub(crate) fn root(&self) -> &Path {
        &self.0
    }

    /// Writes `text` to `relative`, creating the folders on the way.
    pub(crate) fn put(&self, relative: &str, text: &str) {
        self.put_bytes(relative, text.as_bytes());
    }

    pub(crate) fn put_bytes(&self, relative: &str, bytes: &[u8]) {
        let full = self.0.join(relative);
        fs::create_dir_all(full.parent().expect("a file has a parent")).expect("folders");
        fs::write(full, bytes).expect("the test file can be written");
    }

    pub(crate) fn bytes(&self, relative: &str) -> Vec<u8> {
        fs::read(self.0.join(relative)).expect("the test file can be read")
    }
}

/// Makes `link` point at `target`; false when the system or its policy does not allow it.
///
/// On Windows a symbolic link needs a privilege, so a link to a folder falls back to a junction,
/// which does not; a link to a file has no such fallback. A test that gets false makes no claim.
/// The CI runs on Linux, where links are always allowed.
pub(crate) fn make_link(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link).is_ok();
    #[cfg(windows)]
    let made = if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link).is_ok() || make_junction(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link).is_ok()
    };
    made
}

/// A junction is a folder link that Windows lets any user make.
#[cfg(windows)]
fn make_junction(target: &Path, link: &Path) -> bool {
    std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .output()
        .is_ok_and(|output| output.status.success())
}

impl Drop for TestFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
