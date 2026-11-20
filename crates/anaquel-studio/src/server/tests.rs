//! The server is tested through real sockets: each test starts it on port 0 and talks to it with
//! a blocking client that writes the request by hand, so a test can send what a browser never
//! would (a second `Host`, a foreign `Origin`, a token where it does not belong).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use anaquel_diagnostics::Code;
use serde_json::{Value, json};

use super::{Config, MAX_BODY_BYTES, Studio};
use crate::page_store::Revision;
use crate::test_folder::TestFolder;

/// The headers of one request, as name and value.
type Headers<'a> = Vec<(&'a str, &'a str)>;

const PAGE: &str = "volumes/guide/setup.md";
const ORIGIN_OF: fn(u16) -> String = |port| format!("http://127.0.0.1:{port}");

// ----- a client that writes the request by hand ------------------------------------------------

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.body))
    }

    fn error_code(&self) -> String {
        self.json()["error"]["code"]
            .as_str()
            .unwrap_or_else(|| panic!("no error code in {}", self.body))
            .to_owned()
    }
}

/// Sends one request on a new connection. `Host` is added unless the caller names it, so a test
/// can send none, a foreign one or two.
fn send(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> Reply {
    let mut lines = vec![format!("{method} {path} HTTP/1.1")];
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("host"))
    {
        lines.push(format!("Host: 127.0.0.1:{port}"));
    }
    lines.extend(
        headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}")),
    );
    lines.push("Connection: close".to_owned());
    if let Some(body) = body {
        lines.push(format!("Content-Length: {}", body.len()));
    }
    let request = lines.join("\r\n") + "\r\n\r\n";
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("the server accepts");
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    if let Some(body) = body {
        stream.write_all(body.as_bytes()).unwrap();
    }
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("the server answers");
    parse(&raw)
}

fn parse(raw: &[u8]) -> Reply {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("no end of headers in {:?}", String::from_utf8_lossy(raw)));
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|code| code.parse().ok())
        .expect("a status line");
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
        .collect();
    Reply {
        status,
        headers,
        body: String::from_utf8_lossy(&raw[split + 4..]).into_owned(),
    }
}

// ----- a library and a running studio --------------------------------------------------------------

fn library() -> TestFolder {
    let t = TestFolder::new("server");
    t.put(
        "anaquel.toml",
        "schema = 1\nengine = \"0.2\"\ntitle = \"Demo\"\n",
    );
    t.put("volumes/guide/volume.toml", "title = \"Guide\"\n");
    t.put(
        "volumes/guide/_index.md",
        "# Guide\n\nStart with [setup](setup.md).\n",
    );
    t.put(PAGE, "# Setup\n\nInstall it.\n");
    t.put("volumes/guide/terms.md", "---\ndraft: true\n---\n# Terms\n");
    t
}

struct Running {
    studio: Studio,
    folder: TestFolder,
    cookie: String,
}

fn running() -> Running {
    let folder = library();
    let studio = Studio::start(Config {
        root: folder.root().to_path_buf(),
        port: 0,
    })
    .expect("the studio starts");
    let reply = send(
        studio.port(),
        "GET",
        &format!("/__studio/enter?token={}", studio.token()),
        &[],
        None,
    );
    assert_eq!(reply.status, 303, "{}", reply.body);
    let cookie = reply
        .header("set-cookie")
        .and_then(|line| line.split(';').next())
        .expect("a cookie")
        .to_owned();
    Running {
        studio,
        folder,
        cookie,
    }
}

impl Running {
    fn port(&self) -> u16 {
        self.studio.port()
    }

    /// A read as the page makes it: with the cookie.
    fn get(&self, path: &str) -> Reply {
        send(self.port(), "GET", path, &[("Cookie", &self.cookie)], None)
    }

    /// A write as the page makes it: cookie, own origin, JSON.
    fn write(&self, method: &str, path: &str, body: &Value) -> Reply {
        let origin = ORIGIN_OF(self.port());
        send(
            self.port(),
            method,
            path,
            &[
                ("Cookie", &self.cookie),
                ("Origin", &origin),
                ("Content-Type", "application/json"),
                ("Sec-Fetch-Site", "same-origin"),
            ],
            Some(&body.to_string()),
        )
    }

    fn page(&self, path: &str) -> Value {
        let reply = self.get(&format!("/__studio/api/page?path={path}"));
        assert_eq!(reply.status, 200, "{}", reply.body);
        reply.json()
    }

    fn save(&self, text: &str, revision: &str) -> Reply {
        self.write(
            "PUT",
            "/__studio/api/page",
            &json!({ "path": PAGE, "text": text, "revision": revision }),
        )
    }

    fn preview(&self, path: &str, text: &str) -> Reply {
        self.write(
            "POST",
            "/__studio/api/preview",
            &json!({ "path": path, "text": text }),
        )
    }

    fn on_disk(&self) -> Vec<u8> {
        self.folder.bytes(PAGE)
    }
}

const ROUTES: [(&str, &str); 7] = [
    ("GET", "/"),
    ("GET", "/__studio/"),
    ("GET", "/__studio/app.js"),
    ("GET", "/__studio/api/pages"),
    ("GET", "/__studio/api/page?path=volumes/guide/setup.md"),
    ("PUT", "/__studio/api/page"),
    ("POST", "/__studio/api/preview"),
];

// ----- starting and stopping -----------------------------------------------------------------------

#[test]
fn a_folder_that_is_not_a_library_is_refused_before_anything_listens() {
    let t = TestFolder::new("server-empty");
    let error = Studio::start(Config {
        root: t.root().to_path_buf(),
        port: 0,
    })
    .err()
    .expect("refused");
    assert_eq!(error.code, Code::MANIFEST);
    assert!(error.help.is_some());
}

#[test]
fn a_port_that_is_taken_is_a_diagnostic_with_a_hint() {
    let blocker = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = blocker.local_addr().unwrap().port();
    let t = library();
    let error = Studio::start(Config {
        root: t.root().to_path_buf(),
        port,
    })
    .err()
    .expect("the port is in use");
    assert_eq!(error.code, Code::IO);
    assert!(
        error.message.contains(&port.to_string()),
        "{}",
        error.message
    );
    assert!(error.help.as_deref().is_some_and(|h| h.contains("--port")));
}

#[test]
fn port_zero_picks_a_port_and_the_address_names_it() {
    let r = running();
    assert_ne!(r.port(), 0);
    let url = r.studio.entry_url();
    assert!(url.starts_with(&format!(
        "http://127.0.0.1:{}/__studio/enter?token=",
        r.port()
    )));
    assert!(url.ends_with(r.studio.token()));
    assert_eq!(r.studio.token().len(), 64);
}

#[test]
fn two_studios_have_two_keys() {
    let (a, b) = (running(), running());
    assert_ne!(a.studio.token(), b.studio.token());
    // The key of one does not open the other, even on its own port.
    let reply = send(
        b.port(),
        "GET",
        &format!("/__studio/enter?token={}", a.studio.token()),
        &[],
        None,
    );
    assert_eq!(reply.status, 403);
    assert_eq!(reply.error_code(), "wrong_token");
}

#[test]
fn a_stopped_studio_no_longer_listens() {
    let r = running();
    let port = r.port();
    r.studio.stop();
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
}

// ----- the entry route -------------------------------------------------------------------------------

#[test]
fn the_right_key_gives_a_cookie_and_sends_the_browser_on_without_the_key() {
    let r = running();
    let reply = send(
        r.port(),
        "GET",
        &format!("/__studio/enter?token={}", r.studio.token()),
        &[],
        None,
    );
    assert_eq!(reply.status, 303);
    assert_eq!(reply.header("location"), Some("/__studio/"));
    let cookie = reply.header("set-cookie").expect("a cookie");
    assert_eq!(
        cookie,
        format!(
            "anaquel_studio={}; HttpOnly; SameSite=Strict; Path=/",
            r.studio.token()
        )
    );
    assert!(!reply.header("location").unwrap().contains(r.studio.token()));
}

#[test]
fn the_entry_route_refuses_a_missing_key_a_wrong_key_and_any_method_but_get() {
    let r = running();
    let wrong = "0".repeat(64);
    let cases = [
        ("GET", "/__studio/enter".to_owned(), 401, "no_token"),
        (
            "GET",
            "/__studio/enter?token=".to_owned(),
            403,
            "wrong_token",
        ),
        (
            "GET",
            format!("/__studio/enter?token={wrong}"),
            403,
            "wrong_token",
        ),
        (
            "GET",
            format!("/__studio/enter?token={}x", r.studio.token()),
            403,
            "wrong_token",
        ),
        (
            "POST",
            format!("/__studio/enter?token={}", r.studio.token()),
            405,
            "bad_method",
        ),
    ];
    for (method, path, status, code) in cases {
        let reply = send(r.port(), method, &path, &[], None);
        assert_eq!(reply.status, status, "{method} {path}");
        assert_eq!(reply.error_code(), code, "{method} {path}");
        assert!(
            reply.header("set-cookie").is_none(),
            "{method} {path} gave a cookie"
        );
    }
}

// ----- every route needs the right Host, cookie and no key in the address ---------------------------------

#[test]
fn every_route_refuses_a_request_without_the_cookie() {
    let r = running();
    // Origin and content type are right, so that the missing cookie is the only fault.
    let origin = ORIGIN_OF(r.port());
    let headers = [
        ("Origin", origin.as_str()),
        ("Content-Type", "application/json"),
    ];
    for (method, path) in ROUTES {
        let reply = send(r.port(), method, path, &headers, None);
        assert_eq!(reply.status, 401, "{method} {path}: {}", reply.body);
        assert_eq!(reply.error_code(), "no_token", "{method} {path}");
    }
    // A read needs no origin at all, and still needs the cookie.
    assert_eq!(
        send(r.port(), "GET", "/__studio/api/pages", &[], None).status,
        401
    );
}

#[test]
fn every_route_refuses_a_wrong_cookie() {
    let r = running();
    let wrong = format!("anaquel_studio={}", "f".repeat(64));
    let origin = ORIGIN_OF(r.port());
    for (method, path) in ROUTES {
        let reply = send(
            r.port(),
            method,
            path,
            &[
                ("Cookie", &wrong),
                ("Origin", &origin),
                ("Content-Type", "application/json"),
            ],
            None,
        );
        assert_eq!(reply.status, 403, "{method} {path}: {}", reply.body);
        assert_eq!(reply.error_code(), "wrong_token", "{method} {path}");
    }
}

#[test]
fn every_route_refuses_a_foreign_or_repeated_host_even_with_the_cookie() {
    let r = running();
    let origin = ORIGIN_OF(r.port());
    let own = format!("127.0.0.1:{}", r.port());
    let foreign_hosts: [Vec<(&str, &str)>; 5] = [
        vec![("Host", "attacker.example")],
        vec![("Host", "attacker.example:80")],
        vec![("Host", "127.0.0.1:1")],
        vec![("Host", "127.0.0.1")],
        vec![("Host", &own), ("Host", "attacker.example")],
    ];
    for hosts in &foreign_hosts {
        for (method, path) in ROUTES {
            let mut headers = vec![
                ("Cookie", r.cookie.as_str()),
                ("Origin", origin.as_str()),
                ("Content-Type", "application/json"),
            ];
            headers.extend(hosts.iter().copied());
            let reply = send(r.port(), method, path, &headers, None);
            assert_eq!(
                reply.status, 400,
                "{hosts:?} {method} {path}: {}",
                reply.body
            );
            assert_eq!(reply.error_code(), "bad_host", "{hosts:?} {method} {path}");
        }
    }
    // The entry route is checked the same way.
    let enter = format!("/__studio/enter?token={}", r.studio.token());
    let reply = send(
        r.port(),
        "GET",
        &enter,
        &[("Host", "attacker.example")],
        None,
    );
    assert_eq!(reply.status, 400);
    assert!(reply.header("set-cookie").is_none());
}

#[test]
fn the_name_localhost_with_the_port_is_an_accepted_host() {
    let r = running();
    let host = format!("localhost:{}", r.port());
    let reply = send(
        r.port(),
        "GET",
        "/__studio/api/pages",
        &[("Host", &host), ("Cookie", &r.cookie)],
        None,
    );
    assert_eq!(reply.status, 200, "{}", reply.body);
}

#[test]
fn a_key_in_the_address_of_any_other_route_is_refused() {
    let r = running();
    let token = r.studio.token();
    for path in [
        format!("/__studio/?token={token}"),
        format!("/__studio/api/pages?token={token}"),
        format!("/__studio/api/page?path={PAGE}&token={token}"),
        format!("/assets/x.css?token={token}"),
        format!("/?token={token}"),
    ] {
        let reply = r.get(&path);
        assert_eq!(reply.status, 400, "{path}");
        assert_eq!(reply.error_code(), "token_in_url", "{path}");
    }
}

#[test]
fn a_read_from_a_foreign_origin_is_refused_even_with_the_cookie() {
    let r = running();
    for headers in [
        vec![("Origin", "http://attacker.example")],
        vec![("Origin", "null")],
        vec![("Sec-Fetch-Site", "cross-site")],
        vec![("Sec-Fetch-Site", "same-site")],
    ] {
        let mut all = vec![("Cookie", r.cookie.as_str())];
        all.extend(headers.iter().copied());
        let reply = send(r.port(), "GET", "/__studio/api/pages", &all, None);
        assert_eq!(reply.status, 403, "{headers:?}");
        assert_eq!(reply.error_code(), "bad_origin", "{headers:?}");
    }
}

// ----- writes need more than a read ---------------------------------------------------------------------

#[test]
fn a_write_without_its_own_origin_json_and_cookie_changes_nothing() {
    let r = running();
    let before = r.on_disk();
    let revision = r.page(PAGE)["revision"].as_str().unwrap().to_owned();
    let body = json!({ "path": PAGE, "text": "# Hijacked\n", "revision": revision }).to_string();
    let origin = ORIGIN_OF(r.port());
    let json_type = ("Content-Type", "application/json");
    let cookie = ("Cookie", r.cookie.as_str());
    let attempts: Vec<(&str, Headers, u16, &str)> = vec![
        ("no origin", vec![cookie, json_type], 403, "bad_origin"),
        (
            "foreign origin",
            vec![cookie, json_type, ("Origin", "http://attacker.example")],
            403,
            "bad_origin",
        ),
        (
            "null origin",
            vec![cookie, json_type, ("Origin", "null")],
            403,
            "bad_origin",
        ),
        (
            "cross-site fetch metadata",
            vec![
                cookie,
                json_type,
                ("Origin", &origin),
                ("Sec-Fetch-Site", "cross-site"),
            ],
            403,
            "bad_origin",
        ),
        (
            "a form content type",
            vec![
                cookie,
                ("Content-Type", "application/x-www-form-urlencoded"),
                ("Origin", &origin),
            ],
            400,
            "bad_content_type",
        ),
        (
            "plain text content type",
            vec![cookie, ("Content-Type", "text/plain"), ("Origin", &origin)],
            400,
            "bad_content_type",
        ),
        (
            "no content type",
            vec![cookie, ("Origin", &origin)],
            400,
            "bad_content_type",
        ),
        (
            "no cookie",
            vec![json_type, ("Origin", &origin)],
            401,
            "no_token",
        ),
    ];
    for (name, headers, status, code) in attempts {
        let reply = send(r.port(), "PUT", "/__studio/api/page", &headers, Some(&body));
        assert_eq!(reply.status, status, "{name}: {}", reply.body);
        assert_eq!(reply.error_code(), code, "{name}");
        assert_eq!(r.on_disk(), before, "{name} changed the page");
    }
    // And for the preview, which runs work and reads the library.
    let preview = json!({ "path": PAGE, "text": "x" }).to_string();
    let reply = send(
        r.port(),
        "POST",
        "/__studio/api/preview",
        &[cookie, json_type],
        Some(&preview),
    );
    assert_eq!(
        (reply.status, reply.error_code().as_str()),
        (403, "bad_origin")
    );
    // The same request with everything right is accepted.
    assert_eq!(r.preview(PAGE, "# x\n").status, 200);
}

#[test]
fn json_with_a_charset_is_accepted_as_json() {
    let r = running();
    let revision = r.page(PAGE)["revision"].as_str().unwrap().to_owned();
    let origin = ORIGIN_OF(r.port());
    let body = json!({ "path": PAGE, "text": "# Setup\n", "revision": revision }).to_string();
    let reply = send(
        r.port(),
        "PUT",
        "/__studio/api/page",
        &[
            ("Cookie", &r.cookie),
            ("Origin", &origin),
            ("Content-Type", "Application/JSON; charset=utf-8"),
        ],
        Some(&body),
    );
    assert_eq!(reply.status, 200, "{}", reply.body);
}

// ----- the interface and the headers of every answer ----------------------------------------------------------

#[test]
fn the_interface_is_served_with_its_files() {
    let r = running();
    let page = r.get("/__studio/");
    assert_eq!(page.status, 200);
    assert!(
        page.header("content-type")
            .unwrap()
            .starts_with("text/html")
    );
    assert!(page.body.contains("/__studio/app.js"));
    let script = r.get("/__studio/app.js");
    assert!(
        script
            .header("content-type")
            .unwrap()
            .starts_with("text/javascript")
    );
    assert!(script.body.contains("anaquel"));
    for language in ["en", "es"] {
        let texts = r.get(&format!("/__studio/texts.{language}.json"));
        assert_eq!(texts.status, 200);
        assert!(
            texts
                .json()
                .as_object()
                .is_some_and(|table| !table.is_empty())
        );
    }
    let home = r.get("/");
    assert_eq!(
        (home.status, home.header("location")),
        (303, Some("/__studio/"))
    );
}

#[test]
fn every_answer_carries_the_policy_and_the_other_security_headers() {
    let r = running();
    let policy = |reply: &Reply, what: &str| {
        let csp = reply
            .header("content-security-policy")
            .unwrap_or_else(|| panic!("{what}: no policy"));
        for part in [
            "default-src 'none'",
            "script-src 'self'",
            "style-src 'self'",
            "frame-ancestors 'none'",
            "base-uri 'none'",
            "form-action 'none'",
            "object-src 'none'",
        ] {
            assert!(csp.contains(part), "{what}: `{part}` missing from {csp}");
        }
        for forbidden in [
            "unsafe-inline",
            "unsafe-eval",
            "script-src *",
            "default-src *",
        ] {
            assert!(!csp.contains(forbidden), "{what}: `{forbidden}` in {csp}");
        }
        assert_eq!(
            reply.header("x-content-type-options"),
            Some("nosniff"),
            "{what}"
        );
        assert_eq!(
            reply.header("referrer-policy"),
            Some("no-referrer"),
            "{what}"
        );
        assert_eq!(reply.header("cache-control"), Some("no-store"), "{what}");
        assert_eq!(reply.header("x-frame-options"), Some("DENY"), "{what}");
        assert_eq!(
            reply.header("cross-origin-resource-policy"),
            Some("same-origin"),
            "{what}"
        );
    };
    policy(&r.get("/__studio/"), "the interface");
    policy(&r.get("/__studio/api/pages"), "the list");
    policy(&r.get("/nothing"), "a missing address");
    policy(
        &send(r.port(), "GET", "/__studio/api/pages", &[], None),
        "a refusal",
    );
    policy(
        &send(r.port(), "GET", "/", &[("Host", "attacker.example")], None),
        "a refusal for the host",
    );
    policy(
        &send(
            r.port(),
            "GET",
            &format!("/__studio/enter?token={}", r.studio.token()),
            &[],
            None,
        ),
        "the entry",
    );
}

#[test]
fn a_browser_sent_here_by_hand_gets_a_page_that_says_what_to_do() {
    let r = running();
    let reply = send(
        r.port(),
        "GET",
        "/__studio/",
        &[("Accept", "text/html,application/xhtml+xml")],
        None,
    );
    assert_eq!(reply.status, 401);
    assert!(
        reply
            .header("content-type")
            .unwrap()
            .starts_with("text/html")
    );
    assert!(reply.body.contains("anaquel studio"), "{}", reply.body);
    assert!(!reply.body.contains(r.studio.token()));
}

#[test]
fn the_key_is_never_sent_back_except_in_the_cookie_of_the_entry() {
    let r = running();
    let token = r.studio.token().to_owned();
    for (method, path) in ROUTES {
        let reply = send(r.port(), method, path, &[("Cookie", &r.cookie)], None);
        assert!(!reply.body.contains(&token), "{method} {path}");
        assert!(
            reply
                .headers
                .iter()
                .all(|(_, value)| !value.contains(&token)),
            "{method} {path}"
        );
    }
    let wrong = send(
        r.port(),
        "GET",
        &format!("/__studio/enter?token={}", "1".repeat(64)),
        &[],
        None,
    );
    assert!(!wrong.body.contains(&"1".repeat(64)));
}

// ----- reading ---------------------------------------------------------------------------------------------

#[test]
fn the_list_has_every_page_with_its_status() {
    let r = running();
    let list = r.get("/__studio/api/pages").json();
    let pages = list["pages"].as_array().unwrap();
    let status_of = |path: &str| {
        pages
            .iter()
            .find(|page| page["path"] == path)
            .unwrap_or_else(|| panic!("{path} missing from {list}"))["status"]["kind"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(status_of(PAGE), "published");
    assert_eq!(status_of("volumes/guide/terms.md"), "draft");
    assert_eq!(pages.len(), 3);
    assert_eq!(list["problems"], json!([]));
}

#[test]
fn a_page_is_read_with_its_text_and_its_revision() {
    let r = running();
    let page = r.page(PAGE);
    assert_eq!(page["path"], PAGE);
    assert_eq!(page["text"], "# Setup\n\nInstall it.\n");
    assert_eq!(page["revision"], Revision::of(&r.on_disk()).as_str());
}

#[test]
fn a_page_that_cannot_be_read_says_why_and_a_path_cannot_leave_the_library() {
    let r = running();
    let cases = [
        ("/__studio/api/page", 400, "missing_path"),
        ("/__studio/api/page?path=%zz", 400, "missing_path"),
        ("/__studio/api/page?path=", 400, "bad_path"),
        ("/__studio/api/page?path=..%2Fanaquel.toml", 400, "bad_path"),
        (
            "/__studio/api/page?path=volumes%2F..%2Fanaquel.toml",
            400,
            "bad_path",
        ),
        ("/__studio/api/page?path=%2Fetc%2Fpasswd", 400, "bad_path"),
        (
            "/__studio/api/page?path=C%3A%5CWindows%5Cwin.ini",
            400,
            "bad_path",
        ),
        (
            "/__studio/api/page?path=volumes%5Cguide%5Csetup.md",
            400,
            "bad_path",
        ),
        ("/__studio/api/page?path=anaquel.toml", 400, "not_a_page"),
        (
            "/__studio/api/page?path=volumes%2Fguide%2Fvolume.toml",
            400,
            "not_a_page",
        ),
        (
            "/__studio/api/page?path=volumes%2Fguide%2Fnone.md",
            404,
            "missing",
        ),
    ];
    for (path, status, code) in cases {
        let reply = r.get(path);
        assert_eq!(reply.status, status, "{path}: {}", reply.body);
        assert_eq!(reply.error_code(), code, "{path}");
    }
}

// ----- saving ---------------------------------------------------------------------------------------------------

#[test]
fn a_save_replaces_the_page_and_returns_the_revision_of_what_was_written() {
    let r = running();
    let first = r.page(PAGE)["revision"].as_str().unwrap().to_owned();
    let reply = r.save("# Setup\n\nChanged.\n", &first);
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(r.on_disk(), b"# Setup\n\nChanged.\n");
    let saved = reply.json();
    assert_eq!(saved["path"], PAGE);
    assert_eq!(saved["revision"], Revision::of(&r.on_disk()).as_str());
    assert_ne!(saved["revision"], first.as_str());
}

#[test]
fn a_save_from_a_stale_revision_is_refused_and_leaves_the_page_alone() {
    let r = running();
    let first = r.page(PAGE)["revision"].as_str().unwrap().to_owned();
    assert_eq!(r.save("# One\n", &first).status, 200);
    let reply = r.save("# Two\n", &first);
    assert_eq!(reply.status, 409);
    assert_eq!(reply.error_code(), "stale");
    assert_eq!(r.on_disk(), b"# One\n");
}

#[test]
fn a_page_changed_by_another_program_is_not_overwritten() {
    let r = running();
    let loaded = r.page(PAGE)["revision"].as_str().unwrap().to_owned();
    r.folder.put(PAGE, "# Edited elsewhere\n");
    let reply = r.save("# Mine\n", &loaded);
    assert_eq!((reply.status, reply.error_code().as_str()), (409, "stale"));
    assert_eq!(r.on_disk(), b"# Edited elsewhere\n");
}

#[test]
fn a_save_that_cannot_be_accepted_says_why_and_changes_nothing() {
    let r = running();
    let before = r.on_disk();
    let revision = r.page(PAGE)["revision"].as_str().unwrap().to_owned();
    let big = "a".repeat(5 * 1024 * 1024 + 1);
    let cases: Vec<(String, Value, u16, &str)> = vec![
        (
            "a NUL".into(),
            json!({ "path": PAGE, "text": "a\u{0}b", "revision": revision }),
            422,
            "not_text",
        ),
        (
            "a lone CR".into(),
            json!({ "path": PAGE, "text": "a\rb", "revision": revision }),
            422,
            "not_text",
        ),
        (
            "too large".into(),
            json!({ "path": PAGE, "text": big, "revision": revision }),
            413,
            "too_large",
        ),
        (
            "bad revision".into(),
            json!({ "path": PAGE, "text": "x", "revision": "abc" }),
            400,
            "bad_revision",
        ),
        (
            "upper revision".into(),
            json!({ "path": PAGE, "text": "x", "revision": revision.to_uppercase() }),
            400,
            "bad_revision",
        ),
        (
            "another file".into(),
            json!({ "path": "anaquel.toml", "text": "x", "revision": revision }),
            400,
            "not_a_page",
        ),
        (
            "leaving the library".into(),
            json!({ "path": "../x.md", "text": "x", "revision": revision }),
            400,
            "bad_path",
        ),
        (
            "a page that is not there".into(),
            json!({ "path": "volumes/guide/new.md", "text": "x", "revision": revision }),
            404,
            "missing",
        ),
        (
            "an extra field".into(),
            json!({ "path": PAGE, "text": "x", "revision": revision, "force": true }),
            400,
            "bad_json",
        ),
        (
            "a missing field".into(),
            json!({ "path": PAGE, "text": "x" }),
            400,
            "bad_json",
        ),
    ];
    for (name, body, status, code) in cases {
        let reply = r.write("PUT", "/__studio/api/page", &body);
        assert_eq!(reply.status, status, "{name}: {}", reply.body);
        assert_eq!(reply.error_code(), code, "{name}");
        assert_eq!(r.on_disk(), before, "{name} changed the page");
    }
    assert!(!r.folder.root().join("volumes/guide/new.md").exists());
}

#[test]
fn a_body_that_is_not_json_is_refused() {
    let r = running();
    let origin = ORIGIN_OF(r.port());
    let headers = [
        ("Cookie", r.cookie.as_str()),
        ("Origin", origin.as_str()),
        ("Content-Type", "application/json"),
    ];
    for body in ["", "not json", "[]", "null", "{\"path\":"] {
        let reply = send(r.port(), "PUT", "/__studio/api/page", &headers, Some(body));
        assert_eq!(reply.status, 400, "{body:?}");
        assert_eq!(reply.error_code(), "bad_json", "{body:?}");
    }
}

#[test]
fn two_saves_from_the_same_revision_let_exactly_one_win() {
    let r = running();
    let revision = r.page(PAGE)["revision"].as_str().unwrap().to_owned();
    let (port, cookie) = (r.port(), r.cookie.clone());
    let origin = ORIGIN_OF(port);
    let handles: Vec<_> = ["# From A\n", "# From B\n"]
        .into_iter()
        .map(|text| {
            let (cookie, origin, revision) = (cookie.clone(), origin.clone(), revision.clone());
            std::thread::spawn(move || {
                let body = json!({ "path": PAGE, "text": text, "revision": revision }).to_string();
                send(
                    port,
                    "PUT",
                    "/__studio/api/page",
                    &[
                        ("Cookie", &cookie),
                        ("Origin", &origin),
                        ("Content-Type", "application/json"),
                    ],
                    Some(&body),
                )
                .status
            })
        })
        .collect();
    let mut statuses: Vec<u16> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    statuses.sort_unstable();
    assert_eq!(statuses, [200, 409]);
    let on_disk = String::from_utf8(r.on_disk()).unwrap();
    assert!(
        on_disk == "# From A\n" || on_disk == "# From B\n",
        "{on_disk:?}"
    );
}

// ----- the preview -----------------------------------------------------------------------------------------------------

#[test]
fn a_preview_is_the_page_the_pipeline_renders_and_writes_nothing() {
    let r = running();
    let before = r.on_disk();
    let text = "# Setup\n\nA new paragraph with `code`.\n";
    let reply = r.preview(PAGE, text);
    assert_eq!(reply.status, 200, "{}", reply.body);
    let body = reply.json();
    assert_eq!(body["page"], json!([]));
    let expected = anaquel_pipeline::preview_page(r.folder.root(), PAGE, text);
    // The page of the pipeline holds the scripts of the theme; the editor leaves them out because
    // the frame would only report each one as blocked. Everything else is the same page.
    let built = expected.html.clone().unwrap();
    assert!(built.contains("<script"), "{built}");
    assert_eq!(
        body["html"],
        crate::api::inert_preview::without_scripts(&built)
    );
    assert!(!body["html"].as_str().unwrap().contains("<script"));
    assert_eq!(body["url"], expected.url.clone().unwrap());
    assert_eq!(body["draft"], false);
    assert!(body["html"].as_str().unwrap().contains("A new paragraph"));
    assert_eq!(r.on_disk(), before);
}

#[test]
fn a_preview_with_an_error_gives_the_diagnostic_and_no_html() {
    let r = running();
    let reply = r.preview(PAGE, "# Setup\n\nSee [nothing](missing.md).\n");
    assert_eq!(reply.status, 200);
    let body = reply.json();
    assert!(body["html"].is_null(), "{body}");
    let first = &body["page"][0];
    assert_eq!(first["code"], "AQ0302");
    assert_eq!(first["severity"], "error");
    assert!(first["message"].as_str().unwrap().contains("missing.md"));
}

#[test]
fn a_preview_of_a_path_that_is_not_a_page_is_a_diagnostic_not_a_read() {
    let r = running();
    for path in [
        "anaquel.toml",
        "../anaquel.toml",
        "/etc/hosts",
        "volumes/guide/volume.toml",
    ] {
        let reply = r.preview(path, "# x\n");
        assert_eq!(reply.status, 200, "{path}");
        let body = reply.json();
        assert!(body["html"].is_null(), "{path}: {body}");
        assert!(
            !body["page"].as_array().unwrap().is_empty(),
            "{path}: {body}"
        );
    }
}

#[test]
fn a_preview_names_a_draft_and_a_page_that_is_not_saved_yet() {
    let r = running();
    let draft = r
        .preview("volumes/guide/terms.md", "---\ndraft: true\n---\n# Terms\n")
        .json();
    assert_eq!(draft["draft"], true);
    assert!(draft["html"].is_string());
    let new = r
        .preview("volumes/guide/brand-new.md", "# Brand new\n")
        .json();
    assert_eq!(new["page"], json!([]));
    assert!(new["html"].is_string());
    assert!(!r.folder.root().join("volumes/guide/brand-new.md").exists());
}

// ----- the files of the theme ---------------------------------------------------------------------------------------------

/// Every address under `/assets/` that the page writes between double quotes.
fn assets_of(html: &str) -> Vec<String> {
    html.match_indices("/assets/")
        .filter_map(|(start, _)| {
            let end = html[start..].find('"')?;
            Some(html[start..start + end].to_owned())
        })
        .collect()
}

/// The address of the first stylesheet the page links.
fn stylesheet_of(html: &str) -> String {
    assets_of(html)
        .into_iter()
        .find(|address| {
            std::path::Path::new(address)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("css"))
        })
        .expect("the page links a stylesheet")
}

#[test]
fn the_files_the_preview_links_are_served_and_only_to_the_page() {
    let r = running();
    let html = r.preview(PAGE, "# Setup\n").json()["html"]
        .as_str()
        .unwrap()
        .to_owned();
    let asset = stylesheet_of(&html);
    let served = r.get(&asset);
    assert_eq!(served.status, 200, "{asset}: {}", served.body);
    assert_ne!(served.body, "");
    let content_type = served.header("content-type").unwrap();
    assert!(
        content_type.starts_with("text/css") || content_type.starts_with("text/javascript"),
        "{asset}: {content_type}"
    );
    // Everything the page links is there, so the preview does not show broken images or styles.
    let linked = assets_of(&html);
    assert!(linked.len() >= 2, "{linked:?}");
    for address in &linked {
        assert_eq!(r.get(address).status, 200, "{address}");
    }
    // The same address without the cookie, or from another site, is refused like any other.
    assert_eq!(send(r.port(), "GET", &asset, &[], None).status, 401);
    let foreign = send(
        r.port(),
        "GET",
        &asset,
        &[("Cookie", &r.cookie), ("Sec-Fetch-Site", "cross-site")],
        None,
    );
    assert_eq!(foreign.status, 403);
}

#[test]
fn an_asset_address_cannot_reach_a_file_of_the_library_or_anything_else() {
    let r = running();
    for path in [
        "/assets/../anaquel.toml",
        "/assets/%2e%2e/anaquel.toml",
        "/assets/..%2Fanaquel.toml",
        "/assets/..%5Canaquel.toml",
        "/assets/missing.css",
        "/assets/",
        "/assets",
        "/volumes/guide/setup.md",
        "/anaquel.toml",
    ] {
        let reply = r.get(path);
        assert!(
            reply.status == 404 || reply.status == 303,
            "{path} gave {}: {}",
            reply.status,
            reply.body
        );
        assert!(
            !reply.body.contains("schema = 1"),
            "{path} leaked the library"
        );
        assert!(!reply.body.contains("Install it"), "{path} leaked a page");
    }
}

// ----- the rest of the address space ---------------------------------------------------------------------------------------------

#[test]
fn an_unknown_address_or_a_method_that_is_not_answered_gets_a_clear_refusal() {
    let r = running();
    assert_eq!(r.get("/nothing-here").status, 404);
    assert_eq!(r.get("/__studio/api/nothing").status, 404);
    assert_eq!(r.get("/__studio/api/nothing").error_code(), "not_found");
    for (method, path) in [
        ("DELETE", "/__studio/api/page"),
        ("POST", "/__studio/api/pages"),
        ("PUT", "/__studio/api/preview"),
        ("GET", "/__studio/api/preview"),
        ("POST", "/__studio/app.js"),
        ("PATCH", "/__studio/"),
    ] {
        let origin = ORIGIN_OF(r.port());
        let reply = send(
            r.port(),
            method,
            path,
            &[
                ("Cookie", &r.cookie),
                ("Origin", &origin),
                ("Content-Type", "application/json"),
            ],
            None,
        );
        assert_eq!(reply.status, 405, "{method} {path}: {}", reply.body);
        assert_eq!(reply.error_code(), "bad_method");
    }
}

#[test]
fn the_body_limit_leaves_room_for_the_largest_page_written_as_json() {
    // A control character is six bytes in JSON (`\u0001`); the largest page is 5 MiB.
    assert!(MAX_BODY_BYTES as u64 >= 6 * anaquel_pipeline::MAX_PAGE_BYTES);
    let r = running();
    let revision = r.page(PAGE)["revision"].as_str().unwrap().to_owned();
    let text = "\u{1}".repeat(2 * 1024 * 1024);
    let reply = r.save(&text, &revision);
    // Accepted as a body and judged as a page: control characters other than NUL are text.
    assert_eq!(
        reply.status,
        200,
        "{}",
        &reply.body[..reply.body.len().min(200)]
    );
}
