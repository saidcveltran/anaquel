//! The checks every request passes before a route runs.
//!
//! A server on `127.0.0.1` is not private: any web page the user visits can make the browser send
//! a request to it, and a name that points at `127.0.0.1` (a rebinding trick) makes that request
//! look same-origin to the page. The loopback address alone protects nothing, so three more things
//! are required, each against a different attack:
//!
//! - `Host` must be the address the program printed (against rebinding: the attacker's name is in
//!   `Host`);
//! - the token, which exists only in the URL the program printed, must come back in a cookie that
//!   scripts cannot read and that the browser does not send from another site (against a page that
//!   guesses the port);
//! - a request that changes anything must also say it comes from this server's own origin, and
//!   carry JSON, which a cross-site form cannot send without a preflight the server never answers
//!   (against a forged write from a page that somehow holds the cookie).
//!
//! The token is accepted in the query string at one entry route only, where it is exchanged for the
//! cookie and dropped from the address; anywhere else a token in the URL is refused, so it does not
//! end up in a bookmark, a log or a `Referer`.

use axum::http::{HeaderMap, Method, StatusCode, Uri, header};

/// Name of the cookie that carries the token.
pub(crate) const COOKIE_NAME: &str = "anaquel_studio";
/// Name of the query parameter of the entry route.
pub(crate) const TOKEN_PARAMETER: &str = "token";

/// A secret that only the person who started the program has seen.
#[derive(Clone)]
pub(crate) struct Token(String);

impl Token {
    /// 256 random bits, in lowercase hexadecimal.
    pub(crate) fn generate() -> Result<Self, getrandom::Error> {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes)?;
        let mut text = String::with_capacity(64);
        for byte in bytes {
            text.push(char::from(DIGITS[usize::from(byte >> 4)]));
            text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
        }
        Ok(Self(text))
    }

    #[cfg(test)]
    pub(crate) fn from_text(text: &str) -> Self {
        Self(text.to_owned())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Compares without stopping at the first different byte.
    fn matches(&self, candidate: &str) -> bool {
        let (a, b) = (self.0.as_bytes(), candidate.as_bytes());
        a.len() == b.len() && a.iter().zip(b).fold(0_u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }
}

/// What a route asks of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    /// The one route that exchanges the token in the URL for the cookie.
    Enter,
    /// A route that only reads.
    Read,
    /// A route that changes a file or runs work for the editor.
    Write,
}

/// Why a request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// `Host` is missing, repeated, or not the address of this server.
    BadHost,
    /// `Origin` or `Sec-Fetch-Site` says the request comes from somewhere else, or a write has no
    /// `Origin`.
    BadOrigin,
    /// A write that does not carry `application/json`.
    BadContentType,
    /// The entry route was used with a method other than `GET`.
    BadMethod,
    /// There is no token: no cookie, or no parameter on the entry route.
    NoToken,
    /// The token is not the one this server made.
    WrongToken,
    /// A token in the query string of a route that is not the entry route.
    TokenInUrl,
}

impl Refusal {
    /// The HTTP status of the refusal.
    pub(crate) fn status(self) -> StatusCode {
        match self {
            Self::BadHost | Self::BadContentType | Self::TokenInUrl => StatusCode::BAD_REQUEST,
            Self::BadOrigin | Self::WrongToken => StatusCode::FORBIDDEN,
            Self::NoToken => StatusCode::UNAUTHORIZED,
            Self::BadMethod => StatusCode::METHOD_NOT_ALLOWED,
        }
    }

    /// A stable machine-readable name, for the interface and the tests.
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::BadHost => "bad_host",
            Self::BadOrigin => "bad_origin",
            Self::BadContentType => "bad_content_type",
            Self::BadMethod => "bad_method",
            Self::NoToken => "no_token",
            Self::WrongToken => "wrong_token",
            Self::TokenInUrl => "token_in_url",
        }
    }

    /// What to tell the person, who is the only one who can fix it.
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::BadHost => "This request was not addressed to the studio's own address.",
            Self::BadOrigin => "This request does not come from the studio's own page.",
            Self::BadContentType => "A change must be sent as application/json.",
            Self::BadMethod => "This address only answers GET.",
            Self::NoToken | Self::WrongToken => {
                "Open the address that `anaquel studio` printed; it carries the key this page needs."
            }
            Self::TokenInUrl => "The key must not be put in the address of a request.",
        }
    }
}

/// The values a request is checked against.
pub(crate) struct Guard {
    token: Token,
    hosts: [String; 2],
    origins: [String; 2],
}

impl Guard {
    /// A guard for a server on `127.0.0.1:port`.
    pub(crate) fn new(token: Token, port: u16) -> Self {
        Self {
            token,
            hosts: [format!("127.0.0.1:{port}"), format!("localhost:{port}")],
            origins: [
                format!("http://127.0.0.1:{port}"),
                format!("http://localhost:{port}"),
            ],
        }
    }

    pub(crate) fn token(&self) -> &Token {
        &self.token
    }

    /// Checks a request, in the order in which a refusal is the most informative.
    pub(crate) fn check(
        &self,
        access: Access,
        method: &Method,
        uri: &Uri,
        headers: &HeaderMap,
    ) -> Result<(), Refusal> {
        self.check_host(headers)?;
        let query_token = query_value(uri.query(), TOKEN_PARAMETER);
        if access == Access::Enter {
            if *method != Method::GET {
                return Err(Refusal::BadMethod);
            }
            return match query_token {
                None => Err(Refusal::NoToken),
                Some(candidate) if self.token.matches(candidate) => Ok(()),
                Some(_) => Err(Refusal::WrongToken),
            };
        }
        if query_token.is_some() {
            return Err(Refusal::TokenInUrl);
        }
        self.check_origin(access, headers)?;
        match cookie_value(headers, COOKIE_NAME) {
            None => Err(Refusal::NoToken),
            Some(candidate) if self.token.matches(candidate) => Ok(()),
            Some(_) => Err(Refusal::WrongToken),
        }?;
        if access == Access::Write && !is_json(headers) {
            return Err(Refusal::BadContentType);
        }
        Ok(())
    }

    fn check_host(&self, headers: &HeaderMap) -> Result<(), Refusal> {
        let mut values = headers.get_all(header::HOST).iter();
        match (values.next(), values.next()) {
            (Some(host), None) => {
                let host = host.to_str().map_err(|_| Refusal::BadHost)?;
                if self.hosts.iter().any(|h| h.eq_ignore_ascii_case(host)) {
                    Ok(())
                } else {
                    Err(Refusal::BadHost)
                }
            }
            _ => Err(Refusal::BadHost),
        }
    }

    /// `Sec-Fetch-Site` and `Origin` come from the browser and a page cannot set them. A write
    /// must have an `Origin` that is this server; a read may have none, but never a foreign one.
    fn check_origin(&self, access: Access, headers: &HeaderMap) -> Result<(), Refusal> {
        if let Some(site) = headers.get("sec-fetch-site") {
            let from_here = site.to_str().is_ok_and(|site| {
                site == "same-origin" || (access == Access::Read && site == "none")
            });
            if !from_here {
                return Err(Refusal::BadOrigin);
            }
        }
        match (headers.get(header::ORIGIN), access) {
            (Some(origin), _) => {
                let origin = origin.to_str().map_err(|_| Refusal::BadOrigin)?;
                if self.origins.iter().any(|o| o.eq_ignore_ascii_case(origin)) {
                    Ok(())
                } else {
                    Err(Refusal::BadOrigin)
                }
            }
            (None, Access::Write) => Err(Refusal::BadOrigin),
            (None, _) => Ok(()),
        }
    }
}

/// The value of the query parameter `name`, as written (no decoding).
fn query_value<'a>(query: Option<&'a str>, name: &str) -> Option<&'a str> {
    query?
        .split('&')
        .find_map(|pair| match pair.split_once('=') {
            Some((key, value)) if key == name => Some(value),
            None if pair == name => Some(""),
            _ => None,
        })
}

/// The value of the cookie `name`, from any `Cookie` header.
fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|line| line.split(';'))
        .find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == name).then_some(value)
        })
}

/// Whether the body is declared as JSON: the media type, in any case, with parameters allowed.
fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"))
}

#[cfg(test)]
mod tests;
