use super::*;

const PASSPHRASE: &str = "correct horse battery staple";
const SEED: [u8; 32] = [0xAB; 32];
const KEY_ID: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const SALT: [u8; 32] = [9; 32];

fn key() -> SigningKey {
    SigningKey::from_parts(SEED, KEY_ID).unwrap()
}

fn sealed() -> String {
    key()
        .seal_with_salt(PASSPHRASE, KdfCost::CHEAPEST, SALT)
        .unwrap()
}

/// The decoded body of a sealed key file.
fn body_of(text: &str) -> Vec<u8> {
    STANDARD.decode(text.lines().nth(1).unwrap()).unwrap()
}

fn with_body(body: &[u8]) -> String {
    format!("{COMMENT_LINE}\n{}\n", STANDARD.encode(body))
}

#[test]
fn a_sealed_key_opens_with_its_passphrase_and_is_the_same_key() {
    let opened = SigningKey::open(&sealed(), PASSPHRASE).unwrap();
    assert_eq!(opened.key_id_hex(), key().key_id_hex());
    assert_eq!(opened.public_key_line(), key().public_key_line());
    // The same key signs the same bytes the same way.
    assert_eq!(
        opened.sign(b"data", "c").unwrap(),
        key().sign(b"data", "c").unwrap()
    );
}

#[test]
fn a_wrong_or_empty_passphrase_does_not_open_the_key() {
    for wrong in [
        "",
        "correct horse battery stapl",
        "Correct horse battery staple",
        " ",
    ] {
        assert_eq!(
            SigningKey::open(&sealed(), wrong).unwrap_err(),
            SignError::WrongPassphraseOrDamaged,
            "{wrong:?}"
        );
    }
}

#[test]
fn a_short_passphrase_is_refused_when_sealing_but_not_when_opening() {
    let short = "eleven char";
    assert_eq!(short.chars().count(), 11);
    assert_eq!(
        key()
            .seal_with_salt(short, KdfCost::CHEAPEST, SALT)
            .unwrap_err(),
        SignError::WeakPassphrase {
            minimum: MIN_PASSPHRASE_CHARS
        }
    );
    let twelve = "twelve chars";
    assert!(
        key()
            .seal_with_salt(twelve, KdfCost::CHEAPEST, SALT)
            .is_ok()
    );
    // Characters are counted, not bytes: twelve multi-byte characters are enough.
    assert!(
        key()
            .seal_with_salt("ñññññññññññð", KdfCost::CHEAPEST, SALT)
            .is_ok()
    );
}

#[test]
fn changing_any_single_byte_of_the_file_makes_it_unusable() {
    let original = body_of(&sealed());
    assert_eq!(original.len(), BODY_LEN);
    for index in 0..original.len() {
        let mut changed = original.clone();
        changed[index] ^= 0x01;
        assert!(
            SigningKey::open(&with_body(&changed), PASSPHRASE).is_err(),
            "byte {index} was changed and the key still opened"
        );
    }
}

#[test]
fn the_error_names_what_changed_where_it_can() {
    let original = body_of(&sealed());

    let mut magic = original.clone();
    magic[0] ^= 0x01;
    assert!(matches!(
        SigningKey::open(&with_body(&magic), PASSPHRASE),
        Err(SignError::KeyFileFormat(_))
    ));

    let mut version = original.clone();
    version[4] = 2;
    assert_eq!(
        SigningKey::open(&with_body(&version), PASSPHRASE).unwrap_err(),
        SignError::UnsupportedKeyFile {
            found: 2,
            supported: 1
        }
    );

    // A flipped byte in the part the checksum covers is "wrong passphrase or damaged".
    let mut salt = original.clone();
    salt[20] ^= 0x01;
    assert_eq!(
        SigningKey::open(&with_body(&salt), PASSPHRASE).unwrap_err(),
        SignError::WrongPassphraseOrDamaged
    );
}

#[test]
fn text_that_is_not_a_key_file_is_refused_with_a_reason() {
    let good = sealed();
    let comment = good.lines().next().unwrap();
    let line = good.lines().nth(1).unwrap();
    let cases = [
        String::new(),
        "just some text\n".to_owned(),
        format!("{comment}\n"),
        format!("{comment}\n!!!not base64!!!\n"),
        format!("{comment}\n{}\n", &line[..line.len() - 8]),
        format!("{comment}\n{line}\nextra line\n"),
        format!("not a comment\n{line}\n"),
        with_body(&[0u8; BODY_LEN]),
        with_body(&[]),
    ];
    for text in cases {
        assert!(
            matches!(
                SigningKey::open(&text, PASSPHRASE),
                Err(SignError::KeyFileFormat(_))
            ),
            "{text:?}"
        );
    }
    // Trailing blank lines are harmless.
    assert!(SigningKey::open(&format!("{good}\n\n"), PASSPHRASE).is_ok());
}

#[test]
fn a_file_cannot_demand_unbounded_work() {
    let original = body_of(&sealed());
    let ask = |log_n: u8, r: u32, p: u32| {
        let mut body = original.clone();
        body[5] = log_n;
        body[6..10].copy_from_slice(&r.to_le_bytes());
        body[10..14].copy_from_slice(&p.to_le_bytes());
        SigningKey::open(&with_body(&body), PASSPHRASE).unwrap_err()
    };
    for (log_n, r, p) in [
        (MAX_LOG_N + 1, 8, 1),
        (63, 8, 1),
        (MIN_LOG_N - 1, 8, 1),
        (18, 0, 1),
        (18, MAX_R + 1, 1),
        (18, u32::MAX, 1),
        (18, 8, 0),
        (18, 8, MAX_P + 1),
        (18, 8, u32::MAX),
    ] {
        assert_eq!(
            ask(log_n, r, p),
            SignError::KdfCostRefused,
            "{log_n} {r} {p}"
        );
    }
}

#[test]
fn the_cost_constructor_keeps_to_the_same_bounds() {
    assert!(KdfCost::new(MIN_LOG_N, 1, 1).is_ok());
    assert!(KdfCost::new(MAX_LOG_N, MAX_R, MAX_P).is_ok());
    assert_eq!(
        KdfCost::new(MAX_LOG_N + 1, 1, 1),
        Err(SignError::KdfCostRefused)
    );
    assert_eq!(
        KdfCost::new(MIN_LOG_N - 1, 1, 1),
        Err(SignError::KdfCostRefused)
    );
    assert_eq!(KdfCost::new(12, 0, 1), Err(SignError::KdfCostRefused));
    assert_eq!(KdfCost::new(12, 1, 0), Err(SignError::KdfCostRefused));
    assert!(KdfCost::STANDARD.params().is_ok());
}

#[test]
fn an_all_zero_seed_is_refused() {
    assert!(matches!(
        SigningKey::from_parts([0; 32], KEY_ID),
        Err(SignError::KeyFileFormat(_))
    ));
}

#[test]
fn the_salt_changes_the_file_and_a_fixed_salt_repeats_it() {
    let random_one = key().seal(PASSPHRASE, KdfCost::CHEAPEST).unwrap();
    let random_two = key().seal(PASSPHRASE, KdfCost::CHEAPEST).unwrap();
    assert_ne!(random_one, random_two);
    assert_eq!(sealed(), sealed());
    // Neither the seed nor the passphrase appears in the file.
    let text = sealed();
    assert!(!text.contains(PASSPHRASE));
    assert!(!body_of(&text).windows(8).any(|w| w == &SEED[..8]));
}

#[test]
fn two_generated_keys_differ_in_id_and_public_key() {
    let (a, b) = (
        SigningKey::generate().unwrap(),
        SigningKey::generate().unwrap(),
    );
    assert_ne!(a.key_id_hex(), b.key_id_hex());
    assert_ne!(a.public_key_line(), b.public_key_line());
}

#[test]
fn the_public_key_has_the_minisign_layout() {
    let k = key();
    let bytes = STANDARD.decode(k.public_key_line()).unwrap();
    assert_eq!(bytes.len(), 42);
    assert_eq!(&bytes[..2], b"Ed");
    assert_eq!(&bytes[2..10], &KEY_ID);
    assert_eq!(k.key_id_hex(), "0807060504030201");
    let file = k.public_key_file();
    assert!(file.starts_with("untrusted comment: minisign public key: 0807060504030201\n"));
    assert!(file.ends_with(&format!("{}\n", k.public_key_line())));
}

#[test]
fn debug_output_never_shows_the_seed() {
    let shown = format!("{:?}", key());
    assert!(shown.contains("0807060504030201"));
    assert!(!shown.to_lowercase().contains("abab"));
    assert!(!shown.contains("171"));
}
