use std::collections::BTreeSet;
use std::fmt::Write as _;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use blake2::{Blake2b256, Digest};
use ed25519_compact::{KeyPair, Seed};
use scrypt::Params;
use unicode_normalization::UnicodeNormalization as _;
use zeroize::{Zeroize, Zeroizing};

use crate::error::SignError;

/// Fewest characters accepted for the passphrase of a new key.
pub const MIN_PASSPHRASE_CHARS: usize = 12;

const MAGIC: &[u8; 4] = b"AQSK";
const FORMAT_VERSION: u8 = 1;
const SALT_LEN: usize = 32;
const KEY_ID_LEN: usize = 8;
const SEED_LEN: usize = 32;
const TAG_LEN: usize = 32;
/// Everything before the key id, which the tag also covers.
const HEADER_LEN: usize = MAGIC.len() + 1 + 1 + 4 + 4 + SALT_LEN;
const BODY_LEN: usize = HEADER_LEN + KEY_ID_LEN + SEED_LEN + TAG_LEN;
/// Domain separation so the tag cannot be reused for another purpose.
const TAG_DOMAIN: &[u8] = b"anaquel-sign key file v1";
const COMMENT_LINE: &str = "untrusted comment: anaquel release signing key (passphrase protected; not a minisign secret key file)";

/// Bounds a key file may ask for, so a changed file cannot exhaust memory or time.
const MAX_LOG_N: u8 = 22;
const MIN_LOG_N: u8 = 10;
const MAX_R: u32 = 16;
const MAX_P: u32 = 4;
/// scrypt allocates 128 * r * N bytes; this caps it whatever `r` and `N` are.
const MAX_SCRYPT_BYTES: u64 = 1 << 30;
/// A passphrase of one repeated character is as weak as a short one.
const MIN_DISTINCT_CHARS: usize = 6;

/// How hard it is to guess a passphrase: the scrypt cost written into the key file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfCost {
    log_n: u8,
    r: u32,
    p: u32,
}

impl KdfCost {
    /// 256 MiB and about a second on a current machine; used for real keys.
    pub const STANDARD: Self = Self {
        log_n: 18,
        r: 8,
        p: 1,
    };
    /// The cheapest cost the program will read back. Only for tests: it makes a
    /// stolen file easy to attack.
    pub const CHEAPEST: Self = Self {
        log_n: MIN_LOG_N,
        r: 1,
        p: 1,
    };

    /// A cost with explicit scrypt parameters, kept within what a key file may ask for.
    ///
    /// # Errors
    /// Returns [`SignError::KdfCostRefused`] outside the accepted bounds.
    pub fn new(log_n: u8, r: u32, p: u32) -> Result<Self, SignError> {
        if (MIN_LOG_N..=MAX_LOG_N).contains(&log_n)
            && (1..=MAX_R).contains(&r)
            && (1..=MAX_P).contains(&p)
            && (u64::from(r) * 128) << log_n <= MAX_SCRYPT_BYTES
        {
            Ok(Self { log_n, r, p })
        } else {
            Err(SignError::KdfCostRefused)
        }
    }

    fn params(self) -> Result<Params, SignError> {
        Params::new(self.log_n, self.r, self.p).map_err(|e| SignError::Kdf(e.to_string()))
    }
}

/// An Ed25519 release signing key and the id minisign puts in its signatures.
///
/// The seed is zeroed when the value is dropped. This is best effort: the signing
/// library keeps its own working copies while a signature is made.
pub struct SigningKey {
    seed: Zeroizing<[u8; SEED_LEN]>,
    key_id: [u8; KEY_ID_LEN],
    public: [u8; 32],
}

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigningKey")
            .field("key_id", &self.key_id_hex())
            .finish_non_exhaustive()
    }
}

impl SigningKey {
    /// Makes a new key from the operating system's random source.
    ///
    /// # Errors
    /// Returns [`SignError::Random`] when no random bytes are available.
    pub fn generate() -> Result<Self, SignError> {
        let mut seed = Zeroizing::new([0u8; SEED_LEN]);
        let mut key_id = [0u8; KEY_ID_LEN];
        fill_random(&mut seed[..])?;
        fill_random(&mut key_id)?;
        Self::from_parts(*seed, key_id)
    }

    /// Builds a key from known bytes. A seed must be random and secret; this exists
    /// so tests can make the same key twice.
    ///
    /// # Errors
    /// Returns [`SignError::KeyFileFormat`] for an all-zero seed.
    pub(crate) fn from_parts(
        seed: [u8; SEED_LEN],
        key_id: [u8; KEY_ID_LEN],
    ) -> Result<Self, SignError> {
        let seed = Zeroizing::new(seed);
        // Reject a seed the signing library cannot use before it is stored anywhere.
        let pair = KeyPair::try_from_seed(Seed::new(*seed))
            .map_err(|_| SignError::KeyFileFormat("the seed is all zeros".to_owned()))?;
        Ok(Self {
            seed,
            key_id,
            public: *pair.pk,
        })
    }

    /// The key id as 16 uppercase hexadecimal digits, as minisign shows it.
    #[must_use]
    pub fn key_id_hex(&self) -> String {
        let mut text = String::with_capacity(KEY_ID_LEN * 2);
        for byte in self.key_id.iter().rev() {
            // Writing to a String cannot fail.
            let _ = write!(text, "{byte:02X}");
        }
        text
    }

    pub(crate) fn key_pair(&self) -> Result<KeyPair, SignError> {
        KeyPair::try_from_seed(Seed::new(*self.seed))
            .map_err(|_| SignError::KeyFileFormat("the seed is all zeros".to_owned()))
    }

    pub(crate) fn key_id(&self) -> [u8; KEY_ID_LEN] {
        self.key_id
    }

    /// The public key as the single base64 line a minisign `.pub` file holds.
    #[must_use]
    pub fn public_key_line(&self) -> String {
        let mut bytes = Vec::with_capacity(2 + KEY_ID_LEN + 32);
        bytes.extend_from_slice(b"Ed");
        bytes.extend_from_slice(&self.key_id);
        bytes.extend_from_slice(&self.public);
        STANDARD.encode(bytes)
    }

    /// The text of a minisign `.pub` file.
    #[must_use]
    pub fn public_key_file(&self) -> String {
        format!(
            "untrusted comment: minisign public key: {}\n{}\n",
            self.key_id_hex(),
            self.public_key_line()
        )
    }

    /// Encrypts the key under a passphrase with a fresh random salt.
    ///
    /// # Errors
    /// Returns [`SignError::WeakPassphrase`] for a short passphrase, or the error of
    /// the random source or of key stretching.
    pub fn seal(&self, passphrase: &str, cost: KdfCost) -> Result<String, SignError> {
        let mut salt = [0u8; SALT_LEN];
        fill_random(&mut salt)?;
        self.seal_with_salt(passphrase, cost, salt)
    }

    /// Like [`SigningKey::seal`] with a chosen salt, so a test can repeat a result.
    ///
    /// # Errors
    /// The same as [`SigningKey::seal`].
    pub(crate) fn seal_with_salt(
        &self,
        passphrase: &str,
        cost: KdfCost,
        salt: [u8; SALT_LEN],
    ) -> Result<String, SignError> {
        check_new_passphrase(passphrase)?;
        let derived = stretch(passphrase, &salt, cost)?;
        let (pad, mac_key) = derived.split_at(SEED_LEN);

        let mut body = Vec::with_capacity(BODY_LEN);
        body.extend_from_slice(MAGIC);
        body.push(FORMAT_VERSION);
        body.push(cost.log_n);
        body.extend_from_slice(&cost.r.to_le_bytes());
        body.extend_from_slice(&cost.p.to_le_bytes());
        body.extend_from_slice(&salt);
        body.extend_from_slice(&self.key_id);
        body.extend(self.seed.iter().zip(pad).map(|(seed, pad)| seed ^ pad));
        let tag = tag_of(mac_key, &body);
        body.extend_from_slice(&tag);

        Ok(format!("{COMMENT_LINE}\n{}\n", STANDARD.encode(body)))
    }

    /// Reads a key file made by [`SigningKey::seal`].
    ///
    /// # Errors
    /// Returns [`SignError::KeyFileFormat`] or [`SignError::UnsupportedKeyFile`] for
    /// a file that is not one, [`SignError::KdfCostRefused`] for a file asking for too
    /// much work, and [`SignError::WrongPassphraseOrDamaged`] when the checksum fails.
    pub fn open(text: &str, passphrase: &str) -> Result<Self, SignError> {
        if passphrase.is_empty() {
            return Err(SignError::WrongPassphraseOrDamaged);
        }
        let (body, cost) = parse_file(text)?;
        let salt = array_at::<SALT_LEN>(&body, 14);
        let (signed, tag) = body.split_at(BODY_LEN - TAG_LEN);

        let derived = stretch(passphrase, &salt, cost)?;
        let (pad, mac_key) = derived.split_at(SEED_LEN);
        if !constant_time_equal(&tag_of(mac_key, signed), tag) {
            return Err(SignError::WrongPassphraseOrDamaged);
        }

        let key_id = array_at::<KEY_ID_LEN>(&body, HEADER_LEN);
        let masked = &body[HEADER_LEN + KEY_ID_LEN..HEADER_LEN + KEY_ID_LEN + SEED_LEN];
        let mut seed = [0u8; SEED_LEN];
        for ((out, masked), pad) in seed.iter_mut().zip(masked).zip(pad) {
            *out = masked ^ pad;
        }
        let key = Self::from_parts(seed, key_id);
        seed.zeroize();
        key
    }

    /// Checks that `text` is a key file this version can read, without a passphrase and
    /// without any key stretching, so a wrong file is refused before a passphrase is asked.
    ///
    /// # Errors
    /// The format errors of [`SigningKey::open`].
    pub fn check_file_format(text: &str) -> Result<(), SignError> {
        parse_file(text).map(|_| ())
    }
}

/// The decoded body of a key file and the scrypt cost it asks for, after the checks
/// that need no passphrase: layout, version and cost bounds.
fn parse_file(text: &str) -> Result<(Vec<u8>, KdfCost), SignError> {
    let body = decode_body(text)?;
    let version = body[MAGIC.len()];
    if version != FORMAT_VERSION {
        return Err(SignError::UnsupportedKeyFile {
            found: version,
            supported: FORMAT_VERSION,
        });
    }
    let cost = KdfCost::new(
        body[5],
        u32::from_le_bytes([body[6], body[7], body[8], body[9]]),
        u32::from_le_bytes([body[10], body[11], body[12], body[13]]),
    )?;
    Ok((body, cost))
}

/// Checks a passphrase for a new key.
///
/// # Errors
/// Returns [`SignError::WeakPassphrase`] when it has fewer than [`MIN_PASSPHRASE_CHARS`]
/// characters or fewer than six different ones.
pub fn check_new_passphrase(passphrase: &str) -> Result<(), SignError> {
    let distinct = passphrase.chars().collect::<BTreeSet<_>>().len();
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS || distinct < MIN_DISTINCT_CHARS {
        Err(SignError::WeakPassphrase {
            minimum: MIN_PASSPHRASE_CHARS,
        })
    } else {
        Ok(())
    }
}

fn fill_random(buffer: &mut [u8]) -> Result<(), SignError> {
    getrandom::fill(buffer).map_err(|e| SignError::Random(e.to_string()))
}

/// 64 bytes from the passphrase: the first 32 hide the seed, the last 32 key the tag.
fn stretch(
    passphrase: &str,
    salt: &[u8; SALT_LEN],
    cost: KdfCost,
) -> Result<Zeroizing<[u8; 64]>, SignError> {
    // The same letters typed as one composed character or as a base letter plus an
    // accent are one passphrase; without this the key would open on one keyboard only.
    let normalized: Zeroizing<String> = Zeroizing::new(passphrase.nfc().collect());
    let mut derived = Zeroizing::new([0u8; 64]);
    scrypt::scrypt(
        normalized.as_bytes(),
        salt,
        &cost.params()?,
        &mut derived[..],
    )
    .map_err(|e| SignError::Kdf(e.to_string()))?;
    Ok(derived)
}

fn tag_of(mac_key: &[u8], signed: &[u8]) -> [u8; TAG_LEN] {
    let mut hasher = Blake2b256::new();
    hasher.update(TAG_DOMAIN);
    hasher.update(mac_key);
    hasher.update(signed);
    hasher.finalize().into()
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

fn array_at<const N: usize>(bytes: &[u8], at: usize) -> [u8; N] {
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes[at..at + N]);
    out
}

/// Reads the two lines of a key file and returns the decoded body.
fn decode_body(text: &str) -> Result<Vec<u8>, SignError> {
    let bad = |why: &str| SignError::KeyFileFormat(why.to_owned());
    let mut lines = text.lines();
    let comment = lines.next().ok_or_else(|| bad("the file is empty"))?;
    if !comment.starts_with("untrusted comment:") {
        return Err(bad("the first line is not a comment"));
    }
    let encoded = lines.next().ok_or_else(|| bad("the key line is missing"))?;
    if lines.any(|line| !line.trim().is_empty()) {
        return Err(bad("there is text after the key line"));
    }
    let body = STANDARD
        .decode(encoded.trim())
        .map_err(|_| bad("the key line is not base64"))?;
    if body.len() != BODY_LEN {
        return Err(bad("the key line has the wrong length"));
    }
    if &body[..MAGIC.len()] != MAGIC {
        return Err(bad("this is not an anaquel signing key"));
    }
    Ok(body)
}

#[cfg(test)]
mod tests;
