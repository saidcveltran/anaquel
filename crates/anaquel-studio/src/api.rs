//! The JSON the interface and the server exchange, built and read apart from HTTP.
//!
//! Nothing here touches a socket or a file, so each shape can be tested alone. The core types of
//! the crate carry no serialisation: the wire format is defined in this module only, and a change
//! to it is a change to the interface.

use anaquel_diagnostics::Diagnostic;
use anaquel_pipeline::PagePreview;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::library_path::PathError;
use crate::page_store::{PageText, Revision, SaveError};
use crate::pages::{PageList, PageStatus, PageSummary};

/// A refusal in the form the interface reads: an HTTP status, a stable code and a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApiError {
    pub(crate) status: StatusCode,
    pub(crate) code: &'static str,
    pub(crate) message: String,
    /// Files that a failed write could not put back, one line each; empty for every other error.
    pub(crate) unrestored: Vec<String>,
}

impl ApiError {
    pub(crate) fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            unrestored: Vec::new(),
        }
    }

    pub(crate) fn body(&self) -> Value {
        let mut error = json!({ "code": self.code, "message": self.message });
        if !self.unrestored.is_empty() {
            error["unrestored"] = json!(self.unrestored);
        }
        json!({ "error": error })
    }
}

impl From<&PathError> for ApiError {
    fn from(error: &PathError) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_path", error.to_string())
    }
}

impl From<&SaveError> for ApiError {
    fn from(error: &SaveError) -> Self {
        let (status, code) = match error {
            SaveError::NotAPage(_) => (StatusCode::BAD_REQUEST, "not_a_page"),
            SaveError::Missing(_) => (StatusCode::NOT_FOUND, "missing"),
            SaveError::TooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "too_large"),
            SaveError::NotText => (StatusCode::UNPROCESSABLE_ENTITY, "not_text"),
            SaveError::BadRevision => (StatusCode::BAD_REQUEST, "bad_revision"),
            SaveError::Stale => (StatusCode::CONFLICT, "stale"),
            SaveError::MixedLineEndings => (StatusCode::UNPROCESSABLE_ENTITY, "mixed_line_ends"),
            SaveError::Failed { .. } => (StatusCode::INTERNAL_SERVER_ERROR, "write_failed"),
        };
        let mut api = Self::new(status, code, error.to_string());
        if let SaveError::Failed { unrestored, .. } = error {
            api.unrestored.clone_from(unrestored);
        }
        api
    }
}

/// The body of `PUT /__studio/api/page`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SaveRequest {
    pub(crate) path: String,
    pub(crate) text: String,
    pub(crate) revision: String,
}

/// The body of `POST /__studio/api/preview`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreviewRequest {
    pub(crate) path: String,
    pub(crate) text: String,
}

/// Reads a JSON body into its request type; anything else is a `bad_json` refusal.
pub(crate) fn parse_body<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|error| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "bad_json",
            format!("the request body is not the expected JSON: {error}"),
        )
    })
}

/// Reads the revision of a save request.
pub(crate) fn parse_revision(text: &str) -> Result<Revision, ApiError> {
    Revision::parse(text).map_err(|error| ApiError::from(&error))
}

/// The value of one query parameter, percent-decoded. `+` is a plus: the interface encodes with
/// `encodeURIComponent`, which never writes a space as `+`.
pub(crate) fn query_parameter(query: Option<&str>, name: &str) -> Option<String> {
    query?.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then(|| percent_decode(value))?
    })
}

/// Decodes `%XX` sequences; `None` when one is malformed or the result is not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let pair = bytes.get(index + 1..index + 3)?;
            let digits = std::str::from_utf8(pair).ok()?;
            out.push(u8::from_str_radix(digits, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// One diagnostic as the interface shows it. The code is written as `AQ0302`.
pub(crate) fn diagnostic_body(diagnostic: &Diagnostic) -> Value {
    json!({
        "code": diagnostic.code.to_string(),
        "severity": diagnostic.severity.to_string(),
        "message": diagnostic.message,
        "path": diagnostic.path,
        "line": diagnostic.line,
        "help": diagnostic.help,
    })
}

fn status_body(status: &PageStatus) -> Value {
    match status {
        PageStatus::Published => json!({ "kind": "published" }),
        PageStatus::Draft => json!({ "kind": "draft" }),
        PageStatus::Invalid(reason) => json!({ "kind": "invalid", "reason": reason }),
        PageStatus::Unavailable(reason) => json!({ "kind": "unavailable", "reason": reason }),
    }
}

fn summary_body(page: &PageSummary) -> Value {
    json!({
        "path": page.path,
        "shelf": page.shelf,
        "volume": page.volume,
        "title": page.title,
        "status": status_body(&page.status),
        "bytes": page.bytes,
        "modified": page.modified,
    })
}

/// The body of `GET /__studio/api/pages`.
pub(crate) fn pages_body(list: &PageList) -> Value {
    json!({
        "pages": list.pages.iter().map(summary_body).collect::<Vec<_>>(),
        "problems": list.problems,
    })
}

/// The body of `GET /__studio/api/page`.
pub(crate) fn page_body(path: &str, page: &PageText) -> Value {
    json!({ "path": path, "text": page.text, "revision": page.revision.as_str() })
}

/// The body of a successful `PUT /__studio/api/page`.
pub(crate) fn saved_body(path: &str, revision: &Revision) -> Value {
    json!({ "path": path, "revision": revision.as_str() })
}

/// The body of `POST /__studio/api/preview`.
pub(crate) fn preview_body(preview: &PagePreview) -> Value {
    json!({
        "page": preview.page.iter().map(diagnostic_body).collect::<Vec<_>>(),
        "elsewhere": preview.elsewhere.iter().map(diagnostic_body).collect::<Vec<_>>(),
        "html": preview.html,
        "url": preview.url,
        "draft": preview.draft,
        "frame_origins": preview.frame_origins,
    })
}

#[cfg(test)]
mod tests;
