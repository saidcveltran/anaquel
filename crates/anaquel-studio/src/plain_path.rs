//! A path as a person writes it.
//!
//! On Windows `canonicalize` returns the extended form `\\?\C:\folder`, which is correct for the
//! system and strange to read in a message. The messages of the studio show the path as it is
//! typed; the program keeps using the canonical one.

use std::path::Path;

/// The text of `path`, without the extended-length prefix of Windows when the path has an
/// ordinary form: `\\?\C:\a` is shown as `C:\a` and `\\?\UNC\host\share` as `\\host\share`. Any
/// other path, including a volume written by its identifier (`\\?\Volume{...}`), is unchanged.
#[must_use]
pub fn plain_path(path: &Path) -> String {
    let text = path.display().to_string();
    let Some(rest) = text.strip_prefix(r"\\?\") else {
        return text;
    };
    if let Some(share) = rest.strip_prefix(r"UNC\") {
        return format!(r"\\{share}");
    }
    let mut chars = rest.chars();
    let drive = matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(letter), Some(':'), Some('\\')) if letter.is_ascii_alphabetic()
    );
    if drive { rest.to_owned() } else { text }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::plain_path;

    #[test]
    fn a_drive_path_loses_the_extended_prefix() {
        assert_eq!(
            plain_path(Path::new(r"\\?\C:\AIS\library")),
            r"C:\AIS\library"
        );
        assert_eq!(plain_path(Path::new(r"\\?\d:\x")), r"d:\x");
    }

    #[test]
    fn a_network_path_becomes_the_usual_two_slashes() {
        assert_eq!(
            plain_path(Path::new(r"\\?\UNC\host\share\docs")),
            r"\\host\share\docs"
        );
    }

    #[test]
    fn a_path_that_is_already_plain_is_unchanged() {
        for path in [
            r"C:\AIS\library",
            r"\\host\share",
            "/home/ana/library",
            "relative/dir",
            "",
        ] {
            assert_eq!(plain_path(Path::new(path)), path);
        }
    }

    #[test]
    fn a_form_that_is_not_a_drive_or_a_share_is_left_alone() {
        for path in [
            r"\\?\Volume{0123-4567}\folder",
            r"\\?\C:",
            r"\\?\1:\x",
            r"\\?\\x",
            r"\\?\",
        ] {
            assert_eq!(plain_path(Path::new(path)), path, "{path}");
        }
    }
}
