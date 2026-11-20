use anaquel_diagnostics::{Code, Diagnostic};
use anaquel_pipeline::PagePreview;
use axum::http::StatusCode;
use serde_json::json;

use super::{
    ApiError, PreviewRequest, SaveRequest, diagnostic_body, page_body, pages_body, parse_body,
    parse_revision, percent_decode, preview_body, query_parameter, saved_body,
};
use crate::library_path::PathError;
use crate::page_store::{PageText, Revision, SaveError};
use crate::pages::{PageList, PageStatus, PageSummary};

const REVISION: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn summary(path: &str, status: PageStatus) -> PageSummary {
    PageSummary {
        path: path.to_owned(),
        shelf: None,
        volume: "guide".to_owned(),
        title: Some("Setup".to_owned()),
        status,
        bytes: 12,
        modified: Some(7),
    }
}

#[test]
fn every_save_error_has_its_own_status_and_code() {
    let cases = [
        (SaveError::NotAPage("a".into()), 400, "not_a_page"),
        (SaveError::Missing("a".into()), 404, "missing"),
        (SaveError::TooLarge, 413, "too_large"),
        (SaveError::NotText, 422, "not_text"),
        (SaveError::BadRevision, 400, "bad_revision"),
        (SaveError::Stale, 409, "stale"),
        (SaveError::MixedLineEndings, 422, "mixed_line_ends"),
        (
            SaveError::Failed {
                message: "disk".into(),
                unrestored: Vec::new(),
            },
            500,
            "write_failed",
        ),
    ];
    let mut codes = Vec::new();
    for (error, status, code) in cases {
        let api = ApiError::from(&error);
        assert_eq!(api.status.as_u16(), status, "{error:?}");
        assert_eq!(api.code, code);
        assert_eq!(api.message, error.to_string());
        codes.push(api.code);
    }
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), 8, "two errors share a code");
}

#[test]
fn a_failed_write_names_the_files_it_could_not_put_back() {
    let error = SaveError::Failed {
        message: "disk".into(),
        unrestored: vec!["volumes/a/.anaquel-staged-000001".into()],
    };
    let body = ApiError::from(&error).body();
    assert_eq!(
        body["error"]["unrestored"],
        json!(["volumes/a/.anaquel-staged-000001"])
    );
    let clean = ApiError::from(&SaveError::Stale).body();
    assert!(clean["error"].get("unrestored").is_none(), "{clean}");
}

#[test]
fn a_refused_path_is_a_bad_request_with_the_reason() {
    let api = ApiError::from(&PathError::Empty);
    assert_eq!(api.status, StatusCode::BAD_REQUEST);
    assert_eq!(api.code, "bad_path");
    assert_eq!(api.body()["error"]["message"], "the path is empty");
}

#[test]
fn a_save_request_must_have_exactly_its_three_fields() {
    let good = json!({ "path": "volumes/a/b.md", "text": "t", "revision": REVISION }).to_string();
    let request: SaveRequest = parse_body(good.as_bytes()).unwrap();
    assert_eq!(request.path, "volumes/a/b.md");
    assert_eq!(request.text, "t");
    assert_eq!(request.revision, REVISION);

    let extra = json!({ "path": "p", "text": "t", "revision": REVISION, "force": true });
    let missing = json!({ "path": "p", "text": "t" });
    let wrong_type = json!({ "path": "p", "text": 3, "revision": REVISION });
    for body in [extra, missing, wrong_type] {
        let error = parse_body::<SaveRequest>(body.to_string().as_bytes()).unwrap_err();
        assert_eq!(error.code, "bad_json", "{body}");
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
    }
    assert_eq!(
        parse_body::<SaveRequest>(b"not json").unwrap_err().code,
        "bad_json"
    );
    assert_eq!(parse_body::<SaveRequest>(b"").unwrap_err().code, "bad_json");
}

#[test]
fn a_preview_request_refuses_a_revision_it_does_not_use() {
    let good = json!({ "path": "p", "text": "t" }).to_string();
    assert!(parse_body::<PreviewRequest>(good.as_bytes()).is_ok());
    let extra = json!({ "path": "p", "text": "t", "revision": REVISION }).to_string();
    assert!(parse_body::<PreviewRequest>(extra.as_bytes()).is_err());
}

#[test]
fn a_revision_is_sixty_four_lowercase_hexadecimal_digits() {
    assert_eq!(parse_revision(REVISION).unwrap().as_str(), REVISION);
    for bad in ["", "abc", &REVISION.to_uppercase(), &REVISION[1..]] {
        let error = parse_revision(bad).unwrap_err();
        assert_eq!(error.code, "bad_revision", "{bad}");
    }
}

#[test]
fn a_query_parameter_is_found_and_decoded() {
    assert_eq!(
        query_parameter(Some("path=volumes%2Fa%2Fb.md"), "path").as_deref(),
        Some("volumes/a/b.md")
    );
    assert_eq!(
        query_parameter(Some("x=1&path=caf%C3%A9%20au+lait"), "path").as_deref(),
        Some("café au+lait")
    );
    assert_eq!(query_parameter(Some("a=1"), "path"), None);
    assert_eq!(query_parameter(None, "path"), None);
    assert_eq!(query_parameter(Some("path"), "path"), None);
    assert_eq!(query_parameter(Some("path="), "path").as_deref(), Some(""));
}

#[test]
fn a_malformed_percent_sequence_is_not_a_value() {
    for bad in ["%", "%4", "%zz", "%C3", "%FF%FE"] {
        assert_eq!(percent_decode(bad), None, "{bad}");
    }
    assert_eq!(query_parameter(Some("path=%zz"), "path"), None);
}

#[test]
fn a_diagnostic_is_written_with_its_code_as_aq_number() {
    let diagnostic = Diagnostic::error(Code::BROKEN_LINK, "no such page")
        .at("volumes/a/b.md")
        .on_line(4)
        .with_help("check the name");
    let body = diagnostic_body(&diagnostic);
    assert_eq!(body["code"], "AQ0302");
    assert_eq!(body["severity"], "error");
    assert_eq!(body["message"], "no such page");
    assert_eq!(body["path"], "volumes/a/b.md");
    assert_eq!(body["line"], 4);
    assert_eq!(body["help"], "check the name");
    let bare = diagnostic_body(&Diagnostic::warning(Code::IO, "m"));
    assert!(bare["path"].is_null() && bare["line"].is_null() && bare["help"].is_null());
}

#[test]
fn the_list_carries_every_status() {
    let list = PageList {
        pages: vec![
            summary("volumes/guide/a.md", PageStatus::Published),
            summary("volumes/guide/b.md", PageStatus::Draft),
            summary("volumes/guide/c.md", PageStatus::Invalid("bad".into())),
            summary("volumes/guide/d.md", PageStatus::Unavailable("big".into())),
        ],
        problems: vec!["a problem".into()],
    };
    let body = pages_body(&list);
    let kinds: Vec<_> = body["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|page| page["status"]["kind"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(kinds, ["published", "draft", "invalid", "unavailable"]);
    assert_eq!(body["pages"][2]["status"]["reason"], "bad");
    assert_eq!(body["pages"][0]["title"], "Setup");
    assert_eq!(body["pages"][0]["bytes"], 12);
    assert_eq!(body["pages"][0]["modified"], 7);
    assert!(body["pages"][0]["shelf"].is_null());
    assert_eq!(body["problems"], json!(["a problem"]));
}

#[test]
fn a_page_and_a_save_carry_the_revision_as_text() {
    let page = PageText {
        text: "# T\n".to_owned(),
        revision: Revision::parse(REVISION).unwrap(),
    };
    let body = page_body("volumes/a/b.md", &page);
    assert_eq!(
        body,
        json!({ "path": "volumes/a/b.md", "text": "# T\n", "revision": REVISION })
    );
    let saved = saved_body("volumes/a/b.md", &page.revision);
    assert_eq!(
        saved,
        json!({ "path": "volumes/a/b.md", "revision": REVISION })
    );
}

#[test]
fn a_preview_with_an_error_has_no_html_and_says_so() {
    let preview = PagePreview {
        page: vec![Diagnostic::error(Code::BROKEN_LINK, "no such page")],
        elsewhere: vec![Diagnostic::warning(Code::IO, "elsewhere")],
        html: None,
        url: None,
        draft: false,
        frame_origins: Vec::new(),
    };
    let body = preview_body(&preview);
    assert!(body["html"].is_null());
    assert!(body["url"].is_null());
    assert_eq!(body["page"][0]["code"], "AQ0302");
    assert_eq!(body["elsewhere"][0]["message"], "elsewhere");
    assert_eq!(body["draft"], false);
    assert_eq!(body["frame_origins"], json!([]));
}

#[test]
fn a_preview_with_html_carries_it_unchanged() {
    let preview = PagePreview {
        page: Vec::new(),
        elsewhere: Vec::new(),
        html: Some("<p>\"x\" & </p>".to_owned()),
        url: Some("/guide/setup/".to_owned()),
        draft: true,
        frame_origins: vec!["https://www.youtube.com".to_owned()],
    };
    let body = preview_body(&preview);
    assert_eq!(body["html"], "<p>\"x\" & </p>");
    assert_eq!(body["url"], "/guide/setup/");
    assert_eq!(body["draft"], true);
    assert_eq!(body["frame_origins"], json!(["https://www.youtube.com"]));
}

#[test]
fn a_preview_leaves_out_the_scripts_of_its_html_and_nothing_else() {
    let preview = PagePreview {
        page: Vec::new(),
        elsewhere: Vec::new(),
        html: Some(
            "<head><script src=\"/assets/a.js\" defer></script></head><body><p>x</p></body>"
                .to_owned(),
        ),
        url: Some("/guide/setup/".to_owned()),
        draft: false,
        frame_origins: Vec::new(),
    };
    let body = preview_body(&preview);
    assert_eq!(body["html"], "<head></head><body><p>x</p></body>");
    // The value the pipeline holds is untouched: only the body of the request changes.
    assert!(preview.html.as_deref().unwrap().contains("<script"));
}
