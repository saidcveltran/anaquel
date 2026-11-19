use std::collections::{BTreeMap, BTreeSet};

use super::{FILES, PREFIX, find};
use crate::api::ApiError;
use crate::guard::Refusal;
use crate::page_store::SaveError;

const INDEX: &str = include_str!("index.html");
const SCRIPT: &str = include_str!("app.js");
const ENGLISH: &str = include_str!("texts.en.json");
const SPANISH: &str = include_str!("texts.es.json");

fn table(source: &str) -> BTreeMap<String, String> {
    serde_json::from_str(source).expect("a texts file is one flat object of strings")
}

/// The names between braces in a text, sorted: the values a translation must keep.
fn placeholders(text: &str) -> Vec<String> {
    let mut found: Vec<String> = text
        .split('{')
        .skip(1)
        .filter_map(|rest| rest.split_once('}').map(|(name, _)| name.to_owned()))
        .collect();
    found.sort();
    found
}

/// Every double-quoted string in `source` that looks like a text key (`area.name`).
fn quoted_keys(source: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for piece in source.split('"') {
        let shaped = piece.contains('.')
            && !piece.ends_with('.')
            && !piece.starts_with('.')
            && piece
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'_');
        if shaped {
            keys.insert(piece.to_owned());
        }
    }
    keys
}

/// Every value of an attribute such as `data-text="..."` in the page.
fn attribute_values(page: &str, attribute: &str) -> Vec<String> {
    let marker = format!("{attribute}=\"");
    page.split(&marker)
        .skip(1)
        .filter_map(|rest| rest.split_once('"').map(|(value, _)| value.to_owned()))
        .collect()
}

#[test]
fn both_languages_have_the_same_keys() {
    let (english, spanish) = (table(ENGLISH), table(SPANISH));
    let only_english: Vec<_> = english
        .keys()
        .filter(|k| !spanish.contains_key(*k))
        .collect();
    let only_spanish: Vec<_> = spanish
        .keys()
        .filter(|k| !english.contains_key(*k))
        .collect();
    assert!(
        only_english.is_empty(),
        "missing in Spanish: {only_english:?}"
    );
    assert!(
        only_spanish.is_empty(),
        "missing in English: {only_spanish:?}"
    );
}

#[test]
fn a_translation_keeps_the_values_the_text_expects() {
    let (english, spanish) = (table(ENGLISH), table(SPANISH));
    for (key, text) in &english {
        assert_eq!(placeholders(text), placeholders(&spanish[key]), "{key}");
    }
}

#[test]
fn no_text_is_empty_except_the_one_that_means_nothing() {
    for (language, source) in [("en", ENGLISH), ("es", SPANISH)] {
        for (key, text) in table(source) {
            assert!(
                !text.trim().is_empty() || key == "state.clean",
                "{language}: `{key}` is empty"
            );
        }
    }
}

#[test]
fn a_spanish_text_is_not_a_copy_of_the_english_one() {
    // Names and shared words may match; most of the table must not.
    let (english, spanish) = (table(ENGLISH), table(SPANISH));
    let same = english
        .iter()
        .filter(|(key, text)| !text.is_empty() && spanish[*key] == **text)
        .count();
    assert!(
        same * 5 < english.len(),
        "{same} of {} texts are not translated",
        english.len()
    );
}

#[test]
fn the_page_and_the_script_use_only_keys_that_exist() {
    let english = table(ENGLISH);
    let mut used: BTreeSet<String> = attribute_values(INDEX, "data-text").into_iter().collect();
    for value in attribute_values(INDEX, "data-attribute") {
        let (_, key) = value.split_once(':').expect("data-attribute is `name:key`");
        used.insert(key.to_owned());
    }
    used.extend(quoted_keys(SCRIPT));
    let missing: Vec<_> = used.iter().filter(|k| !english.contains_key(*k)).collect();
    assert!(missing.is_empty(), "used but not in the table: {missing:?}");
}

#[test]
fn every_text_is_used_by_the_page_or_the_script() {
    let mut used: BTreeSet<String> = attribute_values(INDEX, "data-text").into_iter().collect();
    for value in attribute_values(INDEX, "data-attribute") {
        used.insert(value.split_once(':').expect("name:key").1.to_owned());
    }
    used.extend(quoted_keys(SCRIPT));
    // `error.<code>` is looked up from the code of a refusal, so the script never spells it.
    let unused: Vec<_> = table(ENGLISH)
        .into_keys()
        .filter(|key| !key.starts_with("error.") && !used.contains(key))
        .collect();
    assert!(unused.is_empty(), "in the table but never used: {unused:?}");
}

#[test]
fn an_error_text_belongs_to_a_code_the_server_can_send() {
    let mut codes: BTreeSet<&str> = [
        Refusal::BadHost,
        Refusal::BadOrigin,
        Refusal::BadContentType,
        Refusal::BadMethod,
        Refusal::NoToken,
        Refusal::WrongToken,
        Refusal::TokenInUrl,
    ]
    .into_iter()
    .map(Refusal::code)
    .collect();
    let save_errors = [
        SaveError::NotAPage(String::new()),
        SaveError::Missing(String::new()),
        SaveError::TooLarge,
        SaveError::NotText,
        SaveError::BadRevision,
        SaveError::Stale,
        SaveError::MixedLineEndings,
        SaveError::Failed {
            message: String::new(),
            unrestored: Vec::new(),
        },
    ];
    codes.extend(save_errors.iter().map(|e| ApiError::from(e).code));
    for key in table(ENGLISH)
        .keys()
        .filter_map(|k| k.strip_prefix("error."))
    {
        assert!(
            codes.contains(key),
            "`error.{key}` is not a code the server sends"
        );
    }
}

#[test]
fn the_page_runs_no_inline_script_or_style_and_the_frame_runs_no_script() {
    // The policy of the server forbids these; a page that used them would simply break, so the
    // test says so before a person has to find out.
    assert_eq!(
        INDEX.matches("<script").count(),
        1,
        "one script, from a file"
    );
    assert!(INDEX.contains("<script src=\"/__studio/app.js\" defer></script>"));
    assert!(!INDEX.contains("<style"), "inline style element");
    assert!(!INDEX.contains(" style=\""), "inline style attribute");
    for handler in [
        " onclick=",
        " onload=",
        " onerror=",
        " oninput=",
        " onchange=",
        " onsubmit=",
    ] {
        assert!(!INDEX.contains(handler), "inline handler {handler}");
    }
    assert!(!INDEX.contains("javascript:"));
    let frame = INDEX
        .split("<iframe")
        .nth(1)
        .and_then(|rest| rest.split_once('>'))
        .map(|(tag, _)| tag)
        .expect("the page has a preview frame");
    assert!(frame.contains("sandbox=\"allow-same-origin\""), "{frame}");
    assert!(!frame.contains("allow-scripts"), "{frame}");
}

#[test]
fn the_script_never_turns_text_into_markup() {
    for forbidden in [
        "innerHTML",
        "outerHTML",
        "insertAdjacentHTML",
        "document.write",
        "eval(",
        "new Function",
        "setTimeout(\"",
    ] {
        assert!(!SCRIPT.contains(forbidden), "`{forbidden}` in app.js");
    }
}

#[test]
fn every_file_has_its_own_address_inside_the_interface_folder() {
    let mut seen = BTreeSet::new();
    for file in FILES {
        assert!(file.path.starts_with(PREFIX), "{}", file.path);
        assert!(seen.insert(file.path), "{} is served twice", file.path);
        assert!(!file.body.is_empty(), "{} is empty", file.path);
        assert!(std::ptr::eq(find(file.path).expect("found"), file));
    }
    assert!(find("/__studio/missing.js").is_none());
    assert!(find("/").is_none());
    assert!(find("/__studio/../anaquel.toml").is_none());
}

#[test]
fn the_page_names_the_files_the_server_serves() {
    for needle in ["/__studio/app.css", "/__studio/app.js"] {
        assert!(INDEX.contains(needle), "{needle}");
        assert!(find(needle).is_some(), "{needle}");
    }
    for language in ["en", "es"] {
        assert!(find(&format!("/__studio/texts.{language}.json")).is_some());
        assert!(
            INDEX.contains(&format!("value=\"{language}\"")),
            "{language}"
        );
    }
}

#[test]
fn the_script_posts_json_and_never_puts_the_key_in_an_address() {
    assert!(
        SCRIPT.contains("\"Content-Type\": \"application/json\"")
            || SCRIPT.contains("\"Content-Type\"] = \"application/json\"")
    );
    assert!(
        !SCRIPT.contains("token"),
        "the interface never handles the key"
    );
    assert!(!SCRIPT.contains("document.cookie"));
}
