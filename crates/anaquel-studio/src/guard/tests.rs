use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header};

use super::{Access, Guard, Refusal, Token};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PORT: u16 = 41_234;

fn guard() -> Guard {
    Guard::new(Token::from_text(TOKEN), PORT)
}

fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.append(
            HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
    }
    map
}

fn cookie() -> String {
    format!("{}={TOKEN}", super::COOKIE_NAME)
}

fn host() -> String {
    format!("127.0.0.1:{PORT}")
}

fn origin() -> String {
    format!("http://127.0.0.1:{PORT}")
}

/// A read with every right header; each test changes one thing.
fn read_headers() -> Vec<(String, String)> {
    vec![("host".to_owned(), host()), ("cookie".to_owned(), cookie())]
}

fn write_headers() -> Vec<(String, String)> {
    vec![
        ("host".to_owned(), host()),
        ("cookie".to_owned(), cookie()),
        ("origin".to_owned(), origin()),
        ("content-type".to_owned(), "application/json".to_owned()),
    ]
}

fn run(
    access: Access,
    method: &Method,
    uri: &str,
    pairs: &[(String, String)],
) -> Result<(), Refusal> {
    let borrowed: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    guard().check(
        access,
        method,
        &uri.parse::<Uri>().unwrap(),
        &headers(&borrowed),
    )
}

fn without(pairs: Vec<(String, String)>, name: &str) -> Vec<(String, String)> {
    pairs.into_iter().filter(|(n, _)| n != name).collect()
}

fn with(mut pairs: Vec<(String, String)>, name: &str, value: &str) -> Vec<(String, String)> {
    pairs.retain(|(n, _)| n != name);
    pairs.push((name.to_owned(), value.to_owned()));
    pairs
}

#[test]
fn a_read_with_the_right_host_and_cookie_passes() {
    assert_eq!(
        run(Access::Read, &Method::GET, "/pages", &read_headers()),
        Ok(())
    );
    // The other name of the loopback address works too, in any case.
    for host in [
        format!("localhost:{PORT}"),
        format!("LocalHost:{PORT}"),
        format!("127.0.0.1:{PORT}"),
    ] {
        let pairs = with(read_headers(), "host", &host);
        assert_eq!(
            run(Access::Read, &Method::GET, "/pages", &pairs),
            Ok(()),
            "{host}"
        );
    }
}

#[test]
fn a_write_with_host_origin_cookie_and_json_passes() {
    assert_eq!(
        run(Access::Write, &Method::PUT, "/page", &write_headers()),
        Ok(())
    );
    for content_type in [
        "application/json; charset=utf-8",
        "APPLICATION/JSON",
        "application/json ;x=y",
    ] {
        let pairs = with(write_headers(), "content-type", content_type);
        assert_eq!(
            run(Access::Write, &Method::POST, "/preview", &pairs),
            Ok(()),
            "{content_type}"
        );
    }
    let pairs = with(
        write_headers(),
        "origin",
        &format!("http://localhost:{PORT}"),
    );
    assert_eq!(run(Access::Write, &Method::PUT, "/page", &pairs), Ok(()));
}

#[test]
fn a_host_that_is_not_the_address_of_the_server_is_refused() {
    let hosts = [
        "evil.example".to_owned(),
        // A name that rebinds to 127.0.0.1 still carries its own name.
        format!("rebind.evil.example:{PORT}"),
        "127.0.0.1".to_owned(),
        format!("127.0.0.1:{}", PORT + 1),
        format!("127.0.0.1:{PORT}.evil.example"),
        format!("127.0.0.1:{PORT} "),
        format!("[::1]:{PORT}"),
        format!("0.0.0.0:{PORT}"),
        format!("localhost.evil.example:{PORT}"),
        String::new(),
    ];
    for host in hosts {
        let pairs = with(read_headers(), "host", &host);
        assert_eq!(
            run(Access::Read, &Method::GET, "/pages", &pairs),
            Err(Refusal::BadHost),
            "{host:?}"
        );
    }
}

#[test]
fn a_missing_or_repeated_host_is_refused() {
    let pairs = without(read_headers(), "host");
    assert_eq!(
        run(Access::Read, &Method::GET, "/pages", &pairs),
        Err(Refusal::BadHost)
    );
    // Two `Host` headers: one right, one foreign. A proxy and the server could read different ones.
    let mut pairs = read_headers();
    pairs.push(("host".to_owned(), "evil.example".to_owned()));
    assert_eq!(
        run(Access::Read, &Method::GET, "/pages", &pairs),
        Err(Refusal::BadHost)
    );
    let mut pairs = read_headers();
    pairs.push(("host".to_owned(), host()));
    assert_eq!(
        run(Access::Read, &Method::GET, "/pages", &pairs),
        Err(Refusal::BadHost)
    );
}

#[test]
fn a_host_that_is_not_text_is_refused() {
    let mut map = HeaderMap::new();
    map.insert(
        header::HOST,
        HeaderValue::from_bytes(b"127.0.0.1:\xFF").unwrap(),
    );
    map.insert(header::COOKIE, HeaderValue::from_str(&cookie()).unwrap());
    let result = guard().check(Access::Read, &Method::GET, &"/pages".parse().unwrap(), &map);
    assert_eq!(result, Err(Refusal::BadHost));
}

#[test]
fn a_request_without_the_cookie_or_with_a_wrong_one_is_refused() {
    let pairs = without(read_headers(), "cookie");
    assert_eq!(
        run(Access::Read, &Method::GET, "/pages", &pairs),
        Err(Refusal::NoToken)
    );
    let name = super::COOKIE_NAME;
    let wrong = [
        format!("{name}={}", TOKEN.to_uppercase()),
        format!("{name}={TOKEN}0"),
        format!("{name}={}", &TOKEN[1..]),
        format!("{name}="),
        // Same value under another name, or the name as the tail of a longer one.
        format!("other={TOKEN}"),
        format!("x{name}={TOKEN}"),
        format!("{name}x={TOKEN}"),
        TOKEN.to_owned(),
    ];
    for cookie in wrong {
        let pairs = with(read_headers(), "cookie", &cookie);
        let result = run(Access::Read, &Method::GET, "/pages", &pairs);
        assert!(
            matches!(result, Err(Refusal::WrongToken | Refusal::NoToken)),
            "{cookie}: {result:?}"
        );
    }
}

#[test]
fn a_token_that_differs_in_any_one_character_is_refused() {
    for position in 0..TOKEN.len() {
        let mut bytes = TOKEN.as_bytes().to_vec();
        bytes[position] = if bytes[position] == b'f' { b'0' } else { b'f' };
        let altered = String::from_utf8(bytes).unwrap();
        let cookie = format!("{}={altered}", super::COOKIE_NAME);
        let pairs = with(read_headers(), "cookie", &cookie);
        assert_eq!(
            run(Access::Read, &Method::GET, "/pages", &pairs),
            Err(Refusal::WrongToken),
            "position {position}"
        );
    }
}

#[test]
fn the_cookie_is_found_among_others_and_in_any_header_line() {
    let amid = format!("theme=dark; {}; lang=es", cookie());
    let pairs = with(read_headers(), "cookie", &amid);
    assert_eq!(run(Access::Read, &Method::GET, "/pages", &pairs), Ok(()));
    let mut pairs = without(read_headers(), "cookie");
    pairs.push(("cookie".to_owned(), "theme=dark".to_owned()));
    pairs.push(("cookie".to_owned(), cookie()));
    assert_eq!(run(Access::Read, &Method::GET, "/pages", &pairs), Ok(()));
}

#[test]
fn a_token_in_the_url_of_a_route_that_is_not_the_entry_is_refused_even_with_the_cookie() {
    for uri in [
        format!("/pages?token={TOKEN}"),
        format!("/pages?a=1&token={TOKEN}"),
        "/pages?token=".to_owned(),
        "/pages?token".to_owned(),
        "/pages?token=wrong".to_owned(),
    ] {
        assert_eq!(
            run(Access::Read, &Method::GET, &uri, &read_headers()),
            Err(Refusal::TokenInUrl),
            "{uri}"
        );
        assert_eq!(
            run(Access::Write, &Method::PUT, &uri, &write_headers()),
            Err(Refusal::TokenInUrl),
            "{uri}"
        );
    }
    // A parameter that merely starts with the name is another parameter.
    assert_eq!(
        run(
            Access::Read,
            &Method::GET,
            "/pages?tokens=1&mytoken=2",
            &read_headers()
        ),
        Ok(())
    );
}

#[test]
fn the_entry_route_takes_the_token_from_the_url_and_needs_no_cookie() {
    let host_only = vec![("host".to_owned(), host())];
    let uri = format!("/enter?token={TOKEN}");
    assert_eq!(run(Access::Enter, &Method::GET, &uri, &host_only), Ok(()));
    assert_eq!(
        run(Access::Enter, &Method::GET, "/enter", &host_only),
        Err(Refusal::NoToken)
    );
    assert_eq!(
        run(
            Access::Enter,
            &Method::GET,
            "/enter?token=wrong",
            &host_only
        ),
        Err(Refusal::WrongToken)
    );
    assert_eq!(
        run(
            Access::Enter,
            &Method::GET,
            &format!("/enter?token={}", &TOKEN[1..]),
            &host_only
        ),
        Err(Refusal::WrongToken)
    );
    // The cookie is no substitute for the parameter on this route.
    assert_eq!(
        run(Access::Enter, &Method::GET, "/enter", &read_headers()),
        Err(Refusal::NoToken)
    );
}

#[test]
fn the_entry_route_still_checks_the_host_and_the_method() {
    let uri = format!("/enter?token={TOKEN}");
    let foreign = vec![("host".to_owned(), "evil.example".to_owned())];
    assert_eq!(
        run(Access::Enter, &Method::GET, &uri, &foreign),
        Err(Refusal::BadHost)
    );
    let host_only = vec![("host".to_owned(), host())];
    for method in [Method::POST, Method::PUT, Method::DELETE, Method::HEAD] {
        assert_eq!(
            run(Access::Enter, &method, &uri, &host_only),
            Err(Refusal::BadMethod),
            "{method}"
        );
    }
}

#[test]
fn a_write_without_an_origin_or_with_a_foreign_one_is_refused() {
    let pairs = without(write_headers(), "origin");
    assert_eq!(
        run(Access::Write, &Method::PUT, "/page", &pairs),
        Err(Refusal::BadOrigin)
    );
    let foreign = [
        "https://evil.example".to_owned(),
        "http://evil.example".to_owned(),
        "null".to_owned(),
        String::new(),
        format!("http://127.0.0.1:{}", PORT + 1),
        // The scheme matters: another origin on the same host and port.
        format!("https://127.0.0.1:{PORT}"),
        format!("http://127.0.0.1:{PORT}.evil.example"),
        format!("http://127.0.0.1:{PORT}/"),
        format!("http://[::1]:{PORT}"),
    ];
    for origin in foreign {
        let pairs = with(write_headers(), "origin", &origin);
        assert_eq!(
            run(Access::Write, &Method::PUT, "/page", &pairs),
            Err(Refusal::BadOrigin),
            "{origin:?}"
        );
    }
}

#[test]
fn a_read_may_have_no_origin_but_never_a_foreign_one() {
    assert_eq!(
        run(Access::Read, &Method::GET, "/pages", &read_headers()),
        Ok(())
    );
    let pairs = with(read_headers(), "origin", "https://evil.example");
    assert_eq!(
        run(Access::Read, &Method::GET, "/pages", &pairs),
        Err(Refusal::BadOrigin)
    );
    let pairs = with(read_headers(), "origin", &origin());
    assert_eq!(run(Access::Read, &Method::GET, "/pages", &pairs), Ok(()));
}

#[test]
fn the_browsers_own_fetch_metadata_is_honoured() {
    for site in ["cross-site", "same-site", "", "SAME-ORIGIN"] {
        let pairs = with(read_headers(), "sec-fetch-site", site);
        assert_eq!(
            run(Access::Read, &Method::GET, "/pages", &pairs),
            Err(Refusal::BadOrigin),
            "read: {site:?}"
        );
        let pairs = with(write_headers(), "sec-fetch-site", site);
        assert_eq!(
            run(Access::Write, &Method::PUT, "/page", &pairs),
            Err(Refusal::BadOrigin),
            "write: {site:?}"
        );
    }
    // A page of this server fetching its own routes.
    let pairs = with(write_headers(), "sec-fetch-site", "same-origin");
    assert_eq!(run(Access::Write, &Method::PUT, "/page", &pairs), Ok(()));
    // Typing the address is `none`, which is fine for reading and never for writing.
    let pairs = with(read_headers(), "sec-fetch-site", "none");
    assert_eq!(run(Access::Read, &Method::GET, "/", &pairs), Ok(()));
    let pairs = with(write_headers(), "sec-fetch-site", "none");
    assert_eq!(
        run(Access::Write, &Method::PUT, "/page", &pairs),
        Err(Refusal::BadOrigin)
    );
}

#[test]
fn a_write_that_is_not_declared_json_is_refused() {
    let pairs = without(write_headers(), "content-type");
    assert_eq!(
        run(Access::Write, &Method::PUT, "/page", &pairs),
        Err(Refusal::BadContentType)
    );
    // The three types a cross-site form can send without asking the server first, and near misses.
    for content_type in [
        "text/plain",
        "application/x-www-form-urlencoded",
        "multipart/form-data; boundary=x",
        "application/jsonx",
        "application/json-patch+json",
        "text/json",
        "",
    ] {
        let pairs = with(write_headers(), "content-type", content_type);
        assert_eq!(
            run(Access::Write, &Method::PUT, "/page", &pairs),
            Err(Refusal::BadContentType),
            "{content_type:?}"
        );
    }
}

#[test]
fn a_read_does_not_need_a_content_type() {
    let pairs = with(read_headers(), "content-type", "text/plain");
    assert_eq!(run(Access::Read, &Method::GET, "/pages", &pairs), Ok(()));
}

#[test]
fn the_first_refusal_is_the_most_informative_one() {
    // Nothing right: the host is what is reported, since the rest says nothing about the sender.
    let pairs = vec![("host".to_owned(), "evil.example".to_owned())];
    assert_eq!(
        run(Access::Write, &Method::PUT, "/page", &pairs),
        Err(Refusal::BadHost)
    );
    // A foreign origin is reported before a missing cookie.
    let pairs = vec![
        ("host".to_owned(), host()),
        ("origin".to_owned(), "https://evil.example".to_owned()),
    ];
    assert_eq!(
        run(Access::Write, &Method::PUT, "/page", &pairs),
        Err(Refusal::BadOrigin)
    );
}

#[test]
fn every_refusal_has_its_own_code_status_and_message() {
    let all = [
        Refusal::BadHost,
        Refusal::BadOrigin,
        Refusal::BadContentType,
        Refusal::BadMethod,
        Refusal::NoToken,
        Refusal::WrongToken,
        Refusal::TokenInUrl,
    ];
    let mut codes: Vec<&str> = all.iter().map(|r| r.code()).collect();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), all.len());
    for refusal in all {
        assert!(!refusal.message().is_empty(), "{refusal:?}");
        assert!(refusal.status().is_client_error(), "{refusal:?}");
    }
    assert_eq!(Refusal::NoToken.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(Refusal::WrongToken.status(), StatusCode::FORBIDDEN);
    assert_eq!(Refusal::BadMethod.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[test]
fn a_generated_token_is_256_bits_of_lowercase_hexadecimal_and_never_repeats() {
    let first = Token::generate().unwrap();
    let second = Token::generate().unwrap();
    for token in [&first, &second] {
        assert_eq!(token.as_str().len(), 64);
        assert!(
            token
                .as_str()
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        );
    }
    assert_ne!(first.as_str(), second.as_str());
}
