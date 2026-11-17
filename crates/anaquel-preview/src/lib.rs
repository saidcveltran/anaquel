//! Local preview server.
//!
//! Serves a built site from the loopback interface only, rebuilds it when a
//! source file changes and tells open pages to reload. It is a convenience for
//! writing, not a hosting setup: it never listens on another address, refuses
//! requests whose `Host` is not local (so a web page cannot reach it through a
//! rebinding trick) and cannot read outside the folder it serves.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anaquel_diagnostics::{Code, Diagnostic};
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::Response;
use notify::RecursiveMode;
use notify_debouncer_full::new_debouncer;

/// Path of the script that asks the server whether to reload.
const RELOAD_SCRIPT_URL: &str = "/__anaquel/reload.js";
const VERSION_URL: &str = "/__anaquel/version";

const RELOAD_SCRIPT: &str = "(function(){var seen=null;function poll(){fetch(\"/__anaquel/version\",{cache:\"no-store\"})\
.then(function(r){return r.text();}).then(function(v){if(seen===null){seen=v;}else if(seen!==v){location.reload();}})\
.catch(function(){});}setInterval(poll,700);poll();})();\n";

/// What the server needs to know.
pub struct Config {
    /// The library folder, watched for changes.
    pub root: PathBuf,
    /// The folder with the built site, served.
    pub dir: PathBuf,
    /// Folders under `root` that are output, never watched.
    pub ignored: Vec<PathBuf>,
    /// TCP port on `127.0.0.1`.
    pub port: u16,
}

/// Callbacks into the program that owns the build.
pub struct Hooks {
    /// Builds the site into `Config::dir`; returns a one-line status to log.
    pub rebuild: Box<dyn Fn() -> String + Send + Sync>,
    /// Receives each log line.
    pub log: Box<dyn Fn(&str) + Send + Sync>,
}

struct ServedSite {
    dir: PathBuf,
    version: AtomicU64,
}

/// Builds once, then serves and rebuilds until the process is stopped.
///
/// # Errors
/// Returns a diagnostic when the port cannot be bound or the file watcher
/// cannot start.
pub fn serve(config: &Config, hooks: Hooks) -> Result<(), Diagnostic> {
    let hooks = Arc::new(hooks);
    (hooks.log)(&(hooks.rebuild)());
    let shared = Arc::new(ServedSite {
        dir: config.dir.clone(),
        version: AtomicU64::new(1),
    });

    let watch_shared = Arc::clone(&shared);
    let watch_hooks = Arc::clone(&hooks);
    let ignored = config.ignored.clone();
    let mut debouncer = new_debouncer(
        Duration::from_millis(300),
        None,
        move |result: notify_debouncer_full::DebounceEventResult| {
            let Ok(events) = result else { return };
            let relevant = events
                .iter()
                .flat_map(|e| e.event.paths.iter())
                .any(|p| is_source(p, &ignored));
            if relevant {
                (watch_hooks.log)(&(watch_hooks.rebuild)());
                watch_shared.version.fetch_add(1, Ordering::SeqCst);
            }
        },
    )
    .map_err(|e| Diagnostic::error(Code::IO, format!("cannot start the file watcher: {e}")))?;
    debouncer
        .watch(&config.root, RecursiveMode::Recursive)
        .map_err(|e| {
            Diagnostic::error(
                Code::IO,
                format!("cannot watch {}: {e}", config.root.display()),
            )
        })?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_io()
        .build()
        .map_err(|e| Diagnostic::error(Code::IO, format!("cannot start the server: {e}")))?;
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", config.port))
            .await
            .map_err(|e| {
                Diagnostic::error(
                    Code::IO,
                    format!("cannot listen on 127.0.0.1:{}: {e}", config.port),
                )
                .with_help("another program may use the port; choose one with `--port`")
            })?;
        (hooks.log)(&format!(
            "serving on http://127.0.0.1:{}/  (Ctrl+C to stop)",
            config.port
        ));
        let app = Router::new().fallback(handle).with_state(shared);
        axum::serve(listener, app)
            .await
            .map_err(|e| Diagnostic::error(Code::IO, format!("server stopped: {e}")))
    })
}

/// A change to a source file, not to build output, editor temporaries or Git.
fn is_source(path: &Path, ignored: &[PathBuf]) -> bool {
    if ignored.iter().any(|dir| path.starts_with(dir)) {
        return false;
    }
    let hidden_or_temporary = path.components().any(|c| {
        matches!(c, Component::Normal(name) if {
            let name = name.to_string_lossy();
            name.starts_with('.') || name.ends_with('~') || name.ends_with(".tmp") || name == "target"
        })
    });
    !hidden_or_temporary
}

/// Answers like the real host: the rules of the built `_headers` file (the
/// Content-Security-Policy above all) apply to every response, so a page that
/// the policy would break fails here too, not only after publishing.
#[allow(clippy::unused_async, reason = "axum handlers must be async")]
async fn handle(State(shared): State<Arc<ServedSite>>, headers: HeaderMap, uri: Uri) -> Response {
    let mut response = route(&shared, &headers, &uri);
    if let Ok(text) = std::fs::read_to_string(shared.dir.join("_headers")) {
        for (name, value) in global_headers(&text) {
            response.headers_mut().insert(name, value);
        }
    }
    response
}

/// The header lines under the `/*` rule of a Cloudflare `_headers` file.
fn global_headers(text: &str) -> Vec<(header::HeaderName, header::HeaderValue)> {
    let mut found = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if !line.starts_with([' ', '\t']) {
            inside = line.trim() == "/*";
        } else if inside
            && let Some((name, value)) = line.trim().split_once(':')
            && let (Ok(name), Ok(value)) = (
                header::HeaderName::from_bytes(name.trim().as_bytes()),
                header::HeaderValue::from_str(value.trim()),
            )
        {
            found.push((name, value));
        }
    }
    found
}

fn route(shared: &ServedSite, headers: &HeaderMap, uri: &Uri) -> Response {
    if !host_is_local(headers) {
        return plain(StatusCode::BAD_REQUEST, "bad host");
    }
    let path = uri.path();
    if path == VERSION_URL {
        return plain(
            StatusCode::OK,
            &shared.version.load(Ordering::SeqCst).to_string(),
        );
    }
    if path == RELOAD_SCRIPT_URL {
        return typed(
            StatusCode::OK,
            "text/javascript; charset=utf-8",
            RELOAD_SCRIPT.as_bytes().to_vec(),
        );
    }
    if let Some((file, bytes)) = resolve(&shared.dir, path)
        .and_then(|file| std::fs::read(&file).ok().map(|bytes| (file, bytes)))
    {
        return respond(StatusCode::OK, &file, bytes);
    }
    let missing = shared.dir.join("404.html");
    match std::fs::read(&missing) {
        Ok(bytes) => respond(StatusCode::NOT_FOUND, &missing, bytes),
        Err(_) => plain(StatusCode::NOT_FOUND, "not found"),
    }
}

/// Only `localhost`, `127.0.0.1` and `[::1]` may be addressed.
fn host_is_local(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else {
        return false;
    };
    let name = if host.starts_with('[') {
        host.find(']')
            .and_then(|end| host.get(..=end))
            .unwrap_or(host)
    } else {
        host.rsplit_once(':').map_or(host, |(name, _)| name)
    };
    matches!(name, "localhost" | "127.0.0.1" | "[::1]")
}

/// Maps a URL path to a file under `dir`, or `None` if it would leave `dir`.
fn resolve(dir: &Path, url_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(url_path)?;
    let mut target = dir.to_path_buf();
    for segment in decoded.split('/').filter(|s| !s.is_empty()) {
        if segment == ".." || segment == "." || segment.contains(['\\', ':', '\0']) {
            return None;
        }
        target.push(segment);
    }
    if target.is_dir() {
        target.push("index.html");
    }
    target.is_file().then_some(target)
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            out.push(u8::from_str_radix(text.get(i + 1..i + 3)?, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        "xml" => "application/xml",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn respond(status: StatusCode, file: &Path, mut bytes: Vec<u8>) -> Response {
    let kind = content_type(file);
    if kind.starts_with("text/html") {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let tag = format!("<script src=\"{RELOAD_SCRIPT_URL}\"></script>");
        bytes = match text.rfind("</body>") {
            Some(at) => format!("{}{tag}{}", &text[..at], &text[at..]).into_bytes(),
            None => format!("{text}{tag}").into_bytes(),
        };
    }
    typed(status, kind, bytes)
}

fn typed(status: StatusCode, kind: &str, bytes: Vec<u8>) -> Response {
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = status;
    if let Ok(value) = kind.parse() {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    if let Ok(value) = "no-store".parse() {
        response.headers_mut().insert(header::CACHE_CONTROL, value);
    }
    response
}

fn plain(status: StatusCode, text: &str) -> Response {
    typed(
        status,
        "text/plain; charset=utf-8",
        text.as_bytes().to_vec(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(host: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, host.parse().unwrap());
        h
    }

    #[test]
    fn only_local_hosts_are_accepted() {
        for ok in [
            "localhost:1111",
            "127.0.0.1:1111",
            "localhost",
            "[::1]:1111",
        ] {
            assert!(host_is_local(&headers(ok)), "{ok}");
        }
        for bad in [
            "evil.example",
            "evil.example:1111",
            "127.0.0.1.evil.example",
            "localhost.evil.example:80",
        ] {
            assert!(!host_is_local(&headers(bad)), "{bad}");
        }
        assert!(!host_is_local(&HeaderMap::new()));
    }

    #[test]
    fn paths_cannot_leave_the_served_folder() {
        let dir = std::env::temp_dir().join(format!("anaquel-preview-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::write(dir.join("a/index.html"), "x").unwrap();
        std::fs::write(dir.join("a.txt"), "y").unwrap();
        assert_eq!(
            resolve(&dir, "/a/").unwrap(),
            dir.join("a").join("index.html")
        );
        assert_eq!(resolve(&dir, "/a.txt").unwrap(), dir.join("a.txt"));
        for bad in [
            "/../a.txt",
            "/a/../../x",
            "/%2e%2e/a.txt",
            "/a%5c..%5ca.txt",
            "/C:/Windows/win.ini",
            "/a.txt%00",
        ] {
            assert!(resolve(&dir, bad).is_none(), "{bad}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_output_and_hidden_files_do_not_trigger_rebuilds() {
        let ignored = vec![PathBuf::from("/lib/dist")];
        assert!(is_source(Path::new("/lib/volumes/a/page.md"), &ignored));
        assert!(!is_source(Path::new("/lib/dist/index.html"), &ignored));
        assert!(!is_source(Path::new("/lib/.git/HEAD"), &ignored));
        assert!(!is_source(Path::new("/lib/volumes/a/page.md~"), &ignored));
    }

    #[test]
    fn only_the_global_rule_of_the_headers_file_is_applied() {
        let text = "/*\n  X-One: 1\n  Content-Security-Policy: default-src 'self'\n/assets/*\n  X-Two: 2\n";
        let found = global_headers(text);
        let names: Vec<_> = found.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["x-one", "content-security-policy"]);
        assert_eq!(found[1].1, "default-src 'self'");
    }

    #[test]
    fn html_gets_the_reload_script_before_body_end() {
        let response = respond(
            StatusCode::OK,
            Path::new("x.html"),
            b"<body>hi</body>".to_vec(),
        );
        assert_eq!(response.status(), StatusCode::OK);
    }
}
