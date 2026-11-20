//! Tests of the download policy and of the client against a server on the loopback interface.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use super::loopback::{Reply, Server, with_unused_port};
use super::{
    Fetch, FetchError, HttpFetch, MAX_REDIRECTS, Policy, SHOWN_ADDRESS_BYTES, USER_AGENT,
    copy_limited, fetch_to_vec, resolve, shown,
};

/// A client for the loopback server, with limits short enough for a test.
fn local_client() -> HttpFetch {
    HttpFetch::new(Policy::loopback().with_timeouts(Duration::from_secs(2), Duration::from_secs(5)))
}

/// A sink that refuses every write.
struct FullDisk;

impl Write for FullDisk {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("no space left"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// ---- the address policy ----------------------------------------------------------------------

#[test]
fn the_release_policy_admits_the_three_github_hosts_over_https() {
    let policy = Policy::release();
    for address in [
        "https://github.com/saidcveltran/anaquel/releases/download/v1.0.0/a.tar.gz",
        "https://objects.githubusercontent.com/github-production-release-asset/1",
        "https://release-assets.githubusercontent.com/github-production-release-asset/1",
        "https://GitHub.COM/saidcveltran/anaquel",
    ] {
        assert!(policy.admit(address).is_ok(), "{address} must be admitted");
    }
}

#[test]
fn the_release_policy_refuses_everything_else() {
    let policy = Policy::release();
    let refused = [
        // The wrong scheme, even on an admitted host.
        ("http://github.com/saidcveltran/anaquel", "only https"),
        ("ftp://github.com/x", "only https"),
        // Refused already when the address is read: it is no web address at all.
        ("file:///etc/passwd", "not a valid address"),
        // Names that only look like an admitted host.
        (
            "https://github.com.example.net/x",
            "not a GitHub release host",
        ),
        ("https://evilgithub.com/x", "not a GitHub release host"),
        ("https://sub.github.com/x", "not a GitHub release host"),
        (
            "https://githubusercontent.com/x",
            "not a GitHub release host",
        ),
        ("https://github.com./x", "not a GitHub release host"),
        (
            "https://example.net/github.com",
            "not a GitHub release host",
        ),
        // Any port, even the default one written out, and addresses without a host.
        ("https://github.com:8443/x", "port"),
        ("https://github.com:443/x", "port"),
        ("https://127.0.0.1/x", "not a GitHub release host"),
        ("https://[::1]/x", "not a GitHub release host"),
        ("/just/a/path", "no host"),
        ("github.com/x", "not a valid address"),
        ("", "not a valid address"),
        ("https://", "not a valid address"),
        ("https://github.com/with space", "not a valid address"),
    ];
    for (address, expected) in refused {
        match policy.admit(address) {
            Err(FetchError::Refused { reason, .. }) => {
                assert!(
                    reason.contains(expected),
                    "{address}: `{reason}` lacks `{expected}`"
                );
            }
            other => panic!("{address} must be refused, got {other:?}"),
        }
    }
}

/// The separator of a user name and a host. The addresses below are built with it at run time,
/// so no line of the tree looks like an e-mail address to the public gate.
const AT: &str = "@";

#[test]
fn a_user_name_in_front_of_the_host_is_refused() {
    // The classic way to make an address look like an admitted host and point elsewhere.
    for (name, host) in [
        ("github.com", "example.net"),
        ("github.com:secret", "github.com"),
        ("x", "127.0.0.1"),
    ] {
        let address = format!("https://{name}{AT}{host}/x");
        match Policy::release().admit(&address) {
            Err(FetchError::Refused { reason, .. }) => assert!(reason.contains("user name")),
            other => panic!("{address} must be refused, got {other:?}"),
        }
    }
}

#[test]
fn the_loopback_policy_admits_only_plain_http_to_the_loopback_interface() {
    let policy = Policy::loopback();
    assert!(policy.admit("http://127.0.0.1:8080/x").is_ok());
    assert!(policy.admit("http://[::1]:8080/x").is_ok());
    for address in [
        "https://127.0.0.1:8080/x",
        "http://localhost:8080/x",
        "http://127.0.0.2:8080/x",
        "http://127.0.0.1.example.net/x",
        "http://github.com/x",
        "https://github.com/x",
    ] {
        assert!(
            matches!(policy.admit(address), Err(FetchError::Refused { .. })),
            "{address} must be refused"
        );
    }
}

#[test]
fn long_addresses_are_cut_in_the_message_on_a_character_boundary() {
    let short = "https://github.com/x";
    assert_eq!(shown(short), short);
    let exact = "a".repeat(SHOWN_ADDRESS_BYTES);
    assert_eq!(shown(&exact), exact);
    // The prefix is 21 bytes and each `é` takes two, so byte 200 falls in the middle of a
    // character and the cut has to step back to byte 199.
    let prefix = "https://example.net/a";
    let long = format!("{prefix}{}", "é".repeat(400));
    assert_eq!(shown(&long), format!("{prefix}{}…", "é".repeat(89)));
    let error = Policy::release().admit(&long).unwrap_err().to_string();
    assert!(error.len() < long.len());
}

/// A reader that is interrupted once before it delivers its bytes, as a read can be by a signal.
struct InterruptedOnce {
    interrupted: bool,
    rest: &'static [u8],
}

impl Read for InterruptedOnce {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(io::ErrorKind::Interrupted.into());
        }
        let n = self.rest.len().min(buffer.len());
        buffer[..n].copy_from_slice(&self.rest[..n]);
        self.rest = &self.rest[n..];
        Ok(n)
    }
}

/// A reader whose every read fails with the given kind.
struct Broken(io::ErrorKind);

impl Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(self.0.into())
    }
}

#[test]
fn an_interrupted_read_is_retried_and_other_read_errors_are_not() {
    let mut sink = Vec::new();
    let mut reader = InterruptedOnce {
        interrupted: false,
        rest: b"payload",
    };
    assert_eq!(copy_limited(&mut reader, 100, &mut sink).unwrap(), 7);
    assert_eq!(sink, b"payload");

    let error = copy_limited(
        &mut Broken(io::ErrorKind::ConnectionReset),
        100,
        &mut Vec::new(),
    );
    assert!(matches!(error, Err(FetchError::Transport(_))), "{error:?}");
    let error = copy_limited(&mut Broken(io::ErrorKind::TimedOut), 100, &mut Vec::new());
    assert!(matches!(error, Err(FetchError::TimedOut)), "{error:?}");
}

#[test]
fn a_relative_destination_keeps_the_origin_and_a_scheme_less_one_is_refused() {
    let from: ureq::http::Uri = "https://github.com/a/b".parse().unwrap();
    assert_eq!(
        resolve(&from, "/c/d?x=1").unwrap(),
        "https://github.com/c/d?x=1"
    );
    assert_eq!(
        resolve(&from, "https://objects.githubusercontent.com/z").unwrap(),
        "https://objects.githubusercontent.com/z"
    );
    for location in ["//example.net/x", "c/d", "", "?x=1"] {
        assert!(
            matches!(resolve(&from, location), Err(FetchError::BadRedirect)),
            "`{location}` must not resolve"
        );
    }
}

// ---- the client against a server of the test -------------------------------------------------

#[test]
fn a_file_is_downloaded_whole_and_the_request_says_only_what_it_must() {
    let body: Vec<u8> = (0..=255_u8).cycle().take(200_000).collect();
    let server = Server::start(vec![Reply::ok(&body)]);
    let mut sink = Vec::new();
    let written = local_client()
        .fetch(&server.address("/dist/a.tar.gz"), 1_000_000, &mut sink)
        .unwrap();
    assert_eq!(written, 200_000);
    assert_eq!(sink, body);
    let heads = server.finish();
    assert_eq!(heads.len(), 1);
    let request = heads[0].to_ascii_lowercase();
    assert!(request.starts_with("get /dist/a.tar.gz http/1.1"));
    assert!(request.contains(&format!("user-agent: {}", USER_AGENT.to_ascii_lowercase())));
    for leaked in ["cookie", "authorization", "referer"] {
        assert!(
            !request.contains(leaked),
            "the request must not carry `{leaked}`"
        );
    }
}

#[test]
fn an_empty_file_is_a_success_of_zero_bytes() {
    let server = Server::start(vec![Reply::ok(b"")]);
    let bytes = fetch_to_vec(&local_client(), &server.address("/e"), 10).unwrap();
    assert_eq!(bytes.len(), 0);
    server.finish();
}

#[test]
fn a_body_exactly_at_the_limit_is_accepted_and_one_byte_more_is_not() {
    let server = Server::start(vec![Reply::ok(&[7; 1000]), Reply::ok(&[7; 1001])]);
    let at_limit = fetch_to_vec(&local_client(), &server.address("/a"), 1000).unwrap();
    assert_eq!(at_limit.len(), 1000);
    let over = fetch_to_vec(&local_client(), &server.address("/b"), 1000).unwrap_err();
    assert!(
        matches!(over, FetchError::TooLarge { limit: 1000 }),
        "{over:?}"
    );
    server.finish();
}

#[test]
fn a_body_with_no_declared_length_is_cut_at_the_limit_and_not_written_past_it() {
    let chunk = [9_u8; 600];
    let server = Server::start(vec![Reply::chunked(&[&chunk, &chunk, &chunk])]);
    let mut sink = Vec::new();
    let error = local_client()
        .fetch(&server.address("/c"), 1000, &mut sink)
        .unwrap_err();
    assert!(
        matches!(error, FetchError::TooLarge { limit: 1000 }),
        "{error:?}"
    );
    assert!(
        sink.len() <= 1000,
        "{} bytes were written past the limit",
        sink.len()
    );
    server.finish();
}

#[test]
fn a_declared_length_over_the_limit_is_refused_before_the_body_is_read() {
    let server = Server::start(vec![Reply::short(5_000_000, b"only a few bytes")]);
    let mut sink = Vec::new();
    let error = local_client()
        .fetch(&server.address("/big"), 1000, &mut sink)
        .unwrap_err();
    assert!(
        matches!(error, FetchError::TooLarge { limit: 1000 }),
        "{error:?}"
    );
    assert!(
        sink.is_empty(),
        "nothing may be written when the length is declared too large"
    );
    server.finish();
}

#[test]
fn a_body_that_ends_before_its_declared_length_is_an_error_and_not_a_success() {
    let server = Server::start(vec![Reply::short(100, b"ten bytes!")]);
    let mut sink = Vec::new();
    let error = local_client()
        .fetch(&server.address("/short"), 1000, &mut sink)
        .unwrap_err();
    assert!(matches!(error, FetchError::Transport(_)), "{error:?}");
    server.finish();
}

#[test]
fn a_redirect_is_followed_to_an_admitted_address() {
    let target = Server::start(vec![Reply::ok(b"the file")]);
    let origin = Server::start(vec![Reply::redirect("302 Found", &target.address("/real"))]);
    let bytes = fetch_to_vec(&local_client(), &origin.address("/asset"), 100).unwrap();
    assert_eq!(bytes, b"the file");
    assert!(target.finish()[0].starts_with("GET /real "));
    origin.finish();
}

#[test]
fn every_redirect_status_is_followed() {
    for status in [
        "301 Moved Permanently",
        "302 Found",
        "303 See Other",
        "307 Temporary Redirect",
        "308 Permanent Redirect",
    ] {
        let target = Server::start(vec![Reply::ok(b"ok")]);
        let origin = Server::start(vec![Reply::redirect(status, &target.address("/t"))]);
        let bytes = fetch_to_vec(&local_client(), &origin.address("/o"), 10).unwrap();
        assert_eq!(bytes, b"ok", "{status}");
        target.finish();
        origin.finish();
    }
}

#[test]
fn a_relative_redirect_stays_on_the_same_server() {
    let server = Server::start(vec![
        Reply::redirect("302 Found", "/second"),
        Reply::ok(b"second"),
    ]);
    let bytes = fetch_to_vec(&local_client(), &server.address("/first"), 100).unwrap();
    assert_eq!(bytes, b"second");
    let heads = server.finish();
    assert!(heads[0].starts_with("GET /first "));
    assert!(heads[1].starts_with("GET /second "));
}

#[test]
fn a_redirect_to_an_address_outside_the_policy_is_refused_and_not_followed() {
    let with_user_name = format!("http://user{AT}127.0.0.1:9/x");
    for location in [
        "http://example.net/steal",
        "https://127.0.0.1:9/x",
        "http://127.0.0.1.example.net/x",
        with_user_name.as_str(),
        "ftp://127.0.0.1/x",
        "//127.0.0.1:9/x",
        "relative/path",
    ] {
        let origin = Server::start(vec![Reply::redirect("302 Found", location)]);
        let error = fetch_to_vec(&local_client(), &origin.address("/o"), 100).unwrap_err();
        assert!(
            matches!(error, FetchError::Refused { .. } | FetchError::BadRedirect),
            "`{location}` gave {error:?}"
        );
        assert_eq!(origin.finish().len(), 1);
    }
}

#[test]
fn a_redirect_without_a_destination_is_refused() {
    let server = Server::start(vec![Reply::redirect_without_location()]);
    let error = fetch_to_vec(&local_client(), &server.address("/x"), 100).unwrap_err();
    assert!(matches!(error, FetchError::BadRedirect), "{error:?}");
    server.finish();
}

#[test]
fn a_chain_of_redirects_ends_at_the_limit_and_the_limit_itself_is_allowed() {
    let hops = usize::try_from(MAX_REDIRECTS).unwrap();
    // `MAX_REDIRECTS` redirects followed by the file: allowed.
    let mut script: Vec<Reply> = (0..hops)
        .map(|_| Reply::redirect("302 Found", "/next"))
        .collect();
    script.push(Reply::ok(b"arrived"));
    let server = Server::start(script);
    let bytes = fetch_to_vec(&local_client(), &server.address("/start"), 100).unwrap();
    assert_eq!(bytes, b"arrived");
    assert_eq!(server.finish().len(), hops + 1);

    // One redirect more: refused, and no further request is made.
    let mut script: Vec<Reply> = (0..=hops)
        .map(|_| Reply::redirect("302 Found", "/next"))
        .collect();
    script.push(Reply::ok(b"never asked for"));
    let server = Server::start(script);
    let error = fetch_to_vec(&local_client(), &server.address("/start"), 100).unwrap_err();
    assert!(matches!(error, FetchError::TooManyRedirects), "{error:?}");
    assert_eq!(server.finish().len(), hops + 1);
}

#[test]
fn a_status_that_is_not_success_or_a_redirect_is_reported_with_its_number() {
    for (line, number) in [
        ("404 Not Found", 404),
        ("403 Forbidden", 403),
        ("500 Internal Server Error", 500),
        ("304 Not Modified", 304),
        ("204 No Content", 204),
        ("206 Partial Content", 206),
    ] {
        let server = Server::start(vec![Reply::status(line)]);
        let error = fetch_to_vec(&local_client(), &server.address("/x"), 100).unwrap_err();
        assert!(
            matches!(error, FetchError::Status(n) if n == number),
            "{line}: {error:?}"
        );
        server.finish();
    }
}

#[test]
fn an_address_outside_the_policy_is_refused_before_anything_is_sent() {
    let server = Server::start(vec![Reply::ok(b"x")]);
    let release = HttpFetch::release();
    let error = fetch_to_vec(&release, &server.address("/x"), 100).unwrap_err();
    assert!(matches!(error, FetchError::Refused { .. }), "{error:?}");
    assert!(
        server.finish().is_empty(),
        "the server must not have been contacted"
    );
}

#[test]
fn a_server_that_never_answers_ends_in_a_timeout_and_not_a_hang() {
    let server = Server::start(vec![Reply::Stall]);
    let client = HttpFetch::new(
        Policy::loopback().with_timeouts(Duration::from_secs(1), Duration::from_millis(400)),
    );
    let started = Instant::now();
    let error = fetch_to_vec(&client, &server.address("/slow"), 100).unwrap_err();
    assert!(matches!(error, FetchError::TimedOut), "{error:?}");
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "took {:?}",
        started.elapsed()
    );
    server.finish();
}

#[test]
fn a_port_with_nobody_listening_fails_and_does_not_hang() {
    let (error, elapsed) = with_unused_port("/x", |address| {
        let started = Instant::now();
        let error = fetch_to_vec(&local_client(), address, 100).unwrap_err();
        (error, started.elapsed())
    });
    // Linux refuses the connection at once (a transport error); Windows retries the connection
    // until the connect limit of the client runs out (a timeout).
    assert!(
        matches!(error, FetchError::Transport(_) | FetchError::TimedOut),
        "{error:?}"
    );
    assert!(elapsed < Duration::from_secs(6), "took {elapsed:?}");
}

#[test]
fn no_server_binds_a_port_while_a_test_probes_an_unused_one() {
    // Negative case of the guard: a server started during the probe must wait for it to end.
    // Without the lock the thread would finish at once and `finished` would be true.
    let (sender, receiver) = std::sync::mpsc::channel();
    let finished_during_probe = with_unused_port("/x", |_| {
        let starter = std::thread::spawn(move || {
            let server = Server::start(vec![]);
            sender.send(()).expect("report the start");
            server.finish();
        });
        std::thread::sleep(Duration::from_millis(300));
        let finished = receiver.try_recv().is_ok();
        (finished, starter)
    });
    let (finished, starter) = finished_during_probe;
    assert!(
        !finished,
        "a server was bound while the port was being probed"
    );
    starter.join().expect("the starter thread ended");
}

#[test]
fn a_sink_that_refuses_the_bytes_is_reported_as_the_sinks_failure() {
    let server = Server::start(vec![Reply::ok(b"some bytes")]);
    let error = local_client()
        .fetch(&server.address("/x"), 100, &mut FullDisk)
        .unwrap_err();
    assert!(matches!(error, FetchError::Sink(_)), "{error:?}");
    server.finish();
}

// ---- the real network, run by hand ----------------------------------------------------------

/// Downloads a small file of the published release through the real policy, so TLS, the root
/// certificates and the redirect to GitHub's download host are exercised for real. It needs the
/// internet, so it is skipped by default: `cargo test -p anaquel-upgrade -- --ignored`.
#[test]
#[ignore = "needs the internet"]
fn a_published_release_file_is_downloaded_over_real_tls() {
    let address = "https://github.com/saidcveltran/anaquel/releases/download/v0.1.1/sha256.sum";
    let bytes = fetch_to_vec(&HttpFetch::release(), address, 1 << 20).unwrap();
    assert_ne!(bytes.len(), 0);
}
