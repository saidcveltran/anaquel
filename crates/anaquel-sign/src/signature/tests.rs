use anaquel_update::{TrustedKeys, UpdateError};

use super::*;

const DATA: &[u8] = b"the bytes of a release manifest";

fn key(seed_byte: u8, id_byte: u8) -> SigningKey {
    SigningKey::from_parts([seed_byte; 32], [id_byte; 8]).unwrap()
}

fn trusting(key: &SigningKey) -> TrustedKeys {
    TrustedKeys::from_base64(&[key.public_key_line().as_str()]).unwrap()
}

#[test]
fn the_signature_file_has_the_four_minisign_lines() {
    let text = key(1, 1).sign(DATA, "timestamp:1 file:m.json").unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4);
    assert!(lines[0].starts_with("untrusted comment: signature from anaquel-sign key "));
    assert_eq!(lines[2], "trusted comment: timestamp:1 file:m.json");
    assert!(text.ends_with('\n'));
}

#[test]
fn the_independent_verifier_accepts_what_this_tool_signs() {
    let signer = key(1, 1);
    let text = signer.sign(DATA, "timestamp:1 file:m.json").unwrap();
    assert_eq!(trusting(&signer).verify(DATA, &text), Ok(()));
}

#[test]
fn the_verifier_refuses_changed_data_another_key_and_a_changed_comment() {
    let signer = key(1, 1);
    let text = signer.sign(DATA, "timestamp:1 file:m.json").unwrap();

    let mut changed = DATA.to_vec();
    changed[0] ^= 1;
    assert_eq!(
        trusting(&signer).verify(&changed, &text),
        Err(UpdateError::UntrustedSignature)
    );

    // A different key, even one with the same key id, must not verify.
    let other = key(2, 1);
    assert_eq!(
        trusting(&other).verify(DATA, &text),
        Err(UpdateError::UntrustedSignature)
    );

    // The trusted comment is signed too: a changed one breaks the signature.
    let forged = text.replace("timestamp:1", "timestamp:2");
    assert_ne!(forged, text);
    assert_eq!(
        trusting(&signer).verify(DATA, &forged),
        Err(UpdateError::UntrustedSignature)
    );
}

#[test]
fn signing_is_repeatable_and_depends_on_every_input() {
    let signer = key(1, 1);
    let first = signer.sign(DATA, "c").unwrap();
    assert_eq!(first, signer.sign(DATA, "c").unwrap());
    assert_ne!(first, signer.sign(b"other", "c").unwrap());
    assert_ne!(first, signer.sign(DATA, "d").unwrap());
    assert_ne!(first, key(2, 1).sign(DATA, "c").unwrap());
}

#[test]
fn a_trusted_comment_must_be_one_plain_line() {
    let signer = key(1, 1);
    for bad in [
        "two\nlines",
        "carriage\rreturn",
        "tab\there",
        "nul\0byte",
        "\u{7f}",
        // Line and paragraph separators, and the bidirectional overrides and isolates.
        "line\u{2028}separator",
        "paragraph\u{2029}separator",
        "override\u{202E}reversed",
        "embedding\u{202A}text",
        "isolate\u{2066}text",
        "isolate end\u{2069}",
    ] {
        assert_eq!(
            signer.sign(DATA, bad),
            Err(SignError::BadTrustedComment),
            "{bad:?}"
        );
    }
}

#[test]
fn text_made_plain_is_always_accepted_as_a_comment_and_keeps_ordinary_text() {
    let signer = key(1, 1);
    let nasty = "a\nb\r\tc\u{202E}d\u{2028}e\u{2066}f\u{7f}g";
    let plain = plain_comment_text(nasty);
    assert_eq!(plain, "abcdefg");
    assert!(signer.sign(DATA, &plain).is_ok());
    assert_eq!(
        plain_comment_text("ñandú 日本 file.json"),
        "ñandú 日本 file.json"
    );
}

#[test]
fn comments_that_are_empty_or_not_ascii_are_signed_and_verified() {
    let signer = key(1, 1);
    for comment in ["", "ñandú 日本", "timestamp:1 file:e\u{301}.json"] {
        let text = signer.sign(DATA, comment).unwrap();
        assert!(text.contains(&format!("trusted comment: {comment}\n")));
        assert_eq!(trusting(&signer).verify(DATA, &text), Ok(()), "{comment:?}");
    }
}

#[test]
fn an_empty_message_can_be_signed() {
    let signer = key(1, 1);
    let text = signer.sign(b"", "c").unwrap();
    assert_eq!(trusting(&signer).verify(b"", &text), Ok(()));
}
