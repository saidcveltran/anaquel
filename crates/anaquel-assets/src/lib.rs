//! Content fingerprints for assets.
//!
//! A fingerprinted file name changes whenever its bytes change, so browsers
//! and the edge can cache it forever and a new build never serves stale CSS.

/// Number of hexadecimal characters of the hash kept in a file name.
const FINGERPRINT_LEN: usize = 10;

/// Short content hash of `bytes`.
pub fn fingerprint(bytes: &[u8]) -> String {
    blake3::hash(bytes)
        .to_hex()
        .as_str()
        .chars()
        .take(FINGERPRINT_LEN)
        .collect()
}

/// `styles/main.css` with bytes `b` becomes `styles/main.<hash>.css`.
/// A name without an extension gets the hash appended after a dot.
pub fn fingerprinted_name(path: &str, bytes: &[u8]) -> String {
    let hash = fingerprint(bytes);
    let (dir, file) = path.rsplit_once('/').map_or(("", path), |(d, f)| (d, f));
    let renamed = match file.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}.{hash}.{ext}"),
        None => format!("{file}.{hash}"),
    };
    if dir.is_empty() {
        renamed
    } else {
        format!("{dir}/{renamed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_bytes_same_name_different_bytes_different_name() {
        let a = fingerprinted_name("styles/main.css", b"a{}");
        assert_eq!(a, fingerprinted_name("styles/main.css", b"a{}"));
        assert_ne!(a, fingerprinted_name("styles/main.css", b"b{}"));
    }

    #[test]
    fn keeps_directory_and_extension() {
        let name = fingerprinted_name("styles/main.css", b"x");
        assert!(
            name.starts_with("styles/main.")
                && std::path::Path::new(&name)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("css")),
            "{name}"
        );
        let bare = fingerprinted_name("LICENSE", b"x");
        assert!(bare.starts_with("LICENSE."), "{bare}");
        assert_eq!(fingerprint(b"x").len(), FINGERPRINT_LEN);
    }
}
