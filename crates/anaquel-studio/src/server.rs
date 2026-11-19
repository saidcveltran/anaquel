//! The HTTP layer of `anaquel studio`: binds the loopback address, checks every request with the
//! [`guard`](crate::guard) and answers the routes of the interface and its API.
//!
//! Every route checks the request before it does anything, and nothing is served on the way that
//! does not pass: a wrong `Host`, a missing or wrong token or a foreign `Origin` is refused with
//! a message the person can act on. The server never listens on another address and never
//! follows a path the [`LibraryPath`] type refuses.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use anaquel_diagnostics::{Code, Diagnostic};
use anaquel_pipeline::{preview_page, theme_assets};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::Response;
use serde_json::{Value, json};
use tokio::sync::oneshot;

use crate::api::{
    ApiError, PreviewRequest, SaveRequest, page_body, pages_body, parse_body, parse_revision,
    preview_body, query_parameter, saved_body,
};
use crate::guard::{Access, COOKIE_NAME, Guard, Refusal, TOKEN_PARAMETER, Token};
use crate::library_path::LibraryPath;
use crate::page_store::{read_page, save_page};
use crate::pages::list_pages;
use crate::ui;

/// Largest request body read: six times the 5 MiB of a page, because JSON writes a control
/// character as six bytes (`\u0001`), and room for the path and the revision. A test keeps this
/// at least six times the page limit of the pipeline.
const MAX_BODY_BYTES: usize = 6 * 5 * 1024 * 1024 + 4096;

/// The policy of every response. Scripts and styles only from this server; frames and images may
/// come from `https:` because the preview shows a page that embeds a video or an image, and the
/// build already limits which addresses a page can name. Inline script and style are never
/// allowed.
const POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self'; \
img-src 'self' https: data:; font-src 'self'; connect-src 'self'; frame-src https:; \
object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// The address of the route that exchanges the token for the cookie.
const ENTER_PATH: &str = "/__studio/enter";
const API_PAGES: &str = "/__studio/api/pages";
const API_PAGE: &str = "/__studio/api/page";
const API_PREVIEW: &str = "/__studio/api/preview";
const ASSETS_PREFIX: &str = "/assets/";

/// What the server needs to know.
#[derive(Debug, Clone)]
pub struct Config {
    /// The library folder.
    pub root: PathBuf,
    /// TCP port on `127.0.0.1`; `0` lets the system choose one.
    pub port: u16,
}

/// A running server. It stops when it is dropped.
pub struct Studio {
    port: u16,
    token: String,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Studio {
    /// Binds `127.0.0.1` and starts answering, on a thread of its own.
    ///
    /// # Errors
    /// Returns a diagnostic when the folder is not a library, the theme cannot be loaded, the port
    /// cannot be bound or the system cannot make a random key.
    pub fn start(config: Config) -> Result<Self, Diagnostic> {
        if !config.root.join("anaquel.toml").is_file() {
            return Err(Diagnostic::error(
                Code::MANIFEST,
                format!(
                    "{} is not a library folder: no anaquel.toml",
                    config.root.display()
                ),
            )
            .with_help(
                "run the command inside a library folder, or create one with `anaquel new`",
            ));
        }
        let assets = theme_assets()?
            .into_iter()
            .map(|asset| (asset.path, asset.bytes))
            .collect();
        let token = Token::generate().map_err(|error| {
            Diagnostic::error(Code::IO, format!("cannot make a random key: {error}"))
        })?;
        let token_text = token.as_str().to_owned();

        let (ready_sender, ready_receiver) = mpsc::channel();
        let (stop_sender, stop_receiver) = oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("anaquel-studio".to_owned())
            .spawn(move || run(config, assets, token, ready_sender, stop_receiver))
            .map_err(|error| {
                Diagnostic::error(Code::IO, format!("cannot start the server thread: {error}"))
            })?;
        match ready_receiver.recv() {
            Ok(Ok(port)) => Ok(Self {
                port,
                token: token_text,
                stop: Some(stop_sender),
                thread: Some(thread),
            }),
            Ok(Err(diagnostic)) => {
                let _ = thread.join();
                Err(diagnostic)
            }
            Err(_) => Err(Diagnostic::error(
                Code::IO,
                "the server stopped before it was ready",
            )),
        }
    }

    /// The port the server listens on.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The address to open: it carries the key, which the page trades for a cookie and drops from
    /// the address bar. Anyone who sees this address can use the studio, so it is shown to the
    /// person who started the program and nowhere else.
    #[must_use]
    pub fn entry_url(&self) -> String {
        format!(
            "http://127.0.0.1:{}{ENTER_PATH}?{TOKEN_PARAMETER}={}",
            self.port, self.token
        )
    }

    /// The key itself, for a caller that makes requests of its own.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Blocks until the server ends, which it does only when it fails.
    pub fn wait(mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    /// Stops the server and waits for its thread.
    pub fn stop(mut self) {
        self.shut_down();
    }

    fn shut_down(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Studio {
    fn drop(&mut self) {
        self.shut_down();
    }
}

/// What every request handler shares.
struct App {
    root: PathBuf,
    guard: Guard,
    /// The files of the theme, by path in the site (`assets/folio.3f2a9c1b.css`).
    assets: HashMap<String, Vec<u8>>,
    /// One save at a time in this process; the revision check protects against other programs.
    save_lock: Mutex<()>,
}

fn run(
    config: Config,
    assets: HashMap<String, Vec<u8>>,
    token: Token,
    ready: mpsc::Sender<Result<u16, Diagnostic>>,
    stop: oneshot::Receiver<()>,
) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_io()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = ready.send(Err(Diagnostic::error(
                Code::IO,
                format!("cannot start the server: {error}"),
            )));
            return;
        }
    };
    runtime.block_on(async move {
        let listener = match tokio::net::TcpListener::bind(("127.0.0.1", config.port)).await {
            Ok(listener) => listener,
            Err(error) => {
                let _ = ready.send(Err(Diagnostic::error(
                    Code::IO,
                    format!("cannot listen on 127.0.0.1:{}: {error}", config.port),
                )
                .with_help("another program may use the port; choose one with `--port`")));
                return;
            }
        };
        let port = match listener.local_addr() {
            Ok(address) => address.port(),
            Err(error) => {
                let _ = ready.send(Err(Diagnostic::error(
                    Code::IO,
                    format!("cannot read the address of the server: {error}"),
                )));
                return;
            }
        };
        let app = Arc::new(App {
            root: config.root,
            guard: Guard::new(token, port),
            assets,
            save_lock: Mutex::new(()),
        });
        let router = Router::new().fallback(handle).with_state(app);
        if ready.send(Ok(port)).is_err() {
            return;
        }
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = stop.await;
            })
            .await;
    });
}

async fn handle(State(app): State<Arc<App>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let response = route(&app, &parts, body).await;
    secured(response)
}

/// The headers every response carries, whatever it holds.
fn secured(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(POLICY),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    response
}

/// What a request asks for, by its address.
enum Target<'a> {
    Enter,
    Home,
    Interface(&'static ui::UiFile),
    Pages,
    Page,
    Preview,
    Asset(&'a str),
    Unknown,
}

impl<'a> Target<'a> {
    fn of(path: &'a str) -> Self {
        match path {
            "/" => Self::Home,
            ENTER_PATH => Self::Enter,
            API_PAGES => Self::Pages,
            API_PAGE => Self::Page,
            API_PREVIEW => Self::Preview,
            _ => {
                if let Some(file) = ui::find(path) {
                    Self::Interface(file)
                } else if let Some(name) = path.strip_prefix(ASSETS_PREFIX) {
                    Self::Asset(name)
                } else {
                    Self::Unknown
                }
            }
        }
    }

    /// What the guard must require: a change, or work for the editor, needs more than a read.
    fn access(&self, method: &Method) -> Access {
        match (self, method) {
            (Self::Enter, _) => Access::Enter,
            (Self::Page, &Method::PUT) | (Self::Preview, &Method::POST) => Access::Write,
            _ => Access::Read,
        }
    }
}

async fn route(app: &Arc<App>, parts: &Parts, body: Body) -> Response {
    let path = parts.uri.path();
    let target = Target::of(path);
    let access = target.access(&parts.method);
    if let Err(refusal) = app
        .guard
        .check(access, &parts.method, &parts.uri, &parts.headers)
    {
        return refused(refusal, &parts.headers);
    }
    let method = &parts.method;
    match target {
        Target::Enter => enter(app),
        Target::Home => redirect(ui::PREFIX),
        Target::Interface(file) if method == Method::GET => {
            text(StatusCode::OK, file.content_type, file.body)
        }
        Target::Asset(name) if method == Method::GET => asset(app, name),
        Target::Pages if method == Method::GET => pages(app).await,
        Target::Page if method == Method::GET => page(app, parts.uri.query()).await,
        Target::Page if method == Method::PUT => save(app, body).await,
        Target::Preview if method == Method::POST => preview(app, body).await,
        Target::Unknown => api_error(&ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "There is nothing at this address.",
        )),
        _ => api_error(&ApiError::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "bad_method",
            "This address does not answer this method.",
        )),
    }
}

/// Exchanges the token in the address for the cookie, and sends the browser to the interface
/// without the token in its address.
fn enter(app: &App) -> Response {
    let cookie = format!(
        "{COOKIE_NAME}={}; HttpOnly; SameSite=Strict; Path=/",
        app.guard.token().as_str()
    );
    let mut response = redirect(ui::PREFIX);
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

fn asset(app: &App, name: &str) -> Response {
    let key = format!("assets/{name}");
    match app.assets.get(&key) {
        Some(bytes) => bytes_response(StatusCode::OK, content_type_of(&key), bytes.clone()),
        None => api_error(&ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "There is no such file.",
        )),
    }
}

fn content_type_of(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "json" | "map" => "application/json; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

async fn pages(app: &Arc<App>) -> Response {
    let root = app.root.clone();
    match tokio::task::spawn_blocking(move || list_pages(&root)).await {
        Ok(list) => json_response(StatusCode::OK, &pages_body(&list)),
        Err(_) => internal(),
    }
}

async fn page(app: &Arc<App>, query: Option<&str>) -> Response {
    let Some(path) = query_parameter(query, "path") else {
        return api_error(&ApiError::new(
            StatusCode::BAD_REQUEST,
            "missing_path",
            "The request has no readable `path`.",
        ));
    };
    let root = app.root.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let library_path = LibraryPath::new(&root, &path).map_err(|e| ApiError::from(&e))?;
        let text = read_page(&library_path).map_err(|e| ApiError::from(&e))?;
        Ok::<_, ApiError>(page_body(library_path.as_str(), &text))
    })
    .await;
    match outcome {
        Ok(Ok(body)) => json_response(StatusCode::OK, &body),
        Ok(Err(error)) => api_error(&error),
        Err(_) => internal(),
    }
}

async fn save(app: &Arc<App>, body: Body) -> Response {
    let bytes = match read_body(body).await {
        Ok(bytes) => bytes,
        Err(error) => return api_error(&error),
    };
    let app = Arc::clone(app);
    let outcome = tokio::task::spawn_blocking(move || {
        let request: SaveRequest = parse_body(&bytes)?;
        let revision = parse_revision(&request.revision)?;
        let library_path =
            LibraryPath::new(&app.root, &request.path).map_err(|e| ApiError::from(&e))?;
        // A poisoned lock only means another save panicked; the files are protected by the
        // revision check, not by this lock.
        let _one_at_a_time = app
            .save_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let saved =
            save_page(&library_path, &request.text, &revision).map_err(|e| ApiError::from(&e))?;
        Ok::<_, ApiError>(saved_body(library_path.as_str(), &saved))
    })
    .await;
    match outcome {
        Ok(Ok(body)) => json_response(StatusCode::OK, &body),
        Ok(Err(error)) => api_error(&error),
        Err(_) => internal(),
    }
}

async fn preview(app: &Arc<App>, body: Body) -> Response {
    let bytes = match read_body(body).await {
        Ok(bytes) => bytes,
        Err(error) => return api_error(&error),
    };
    let root = app.root.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let request: PreviewRequest = parse_body(&bytes)?;
        Ok::<_, ApiError>(preview_body(&preview_page(
            &root,
            &request.path,
            &request.text,
        )))
    })
    .await;
    match outcome {
        Ok(Ok(body)) => json_response(StatusCode::OK, &body),
        Ok(Err(error)) => api_error(&error),
        Err(_) => internal(),
    }
}

async fn read_body(body: Body) -> Result<Vec<u8>, ApiError> {
    to_bytes(body, MAX_BODY_BYTES)
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|_| {
            ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "body_too_large",
                "The request is larger than a page may be.",
            )
        })
}

/// A refusal of the guard: JSON for the interface, a small page for a browser that was sent here
/// by hand.
fn refused(refusal: Refusal, headers: &HeaderMap) -> Response {
    let wants_page = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| accept.contains("text/html"));
    if wants_page {
        let page = format!(
            "<!doctype html><meta charset=\"utf-8\"><title>anaquel studio</title><p>{}</p>",
            refusal.message()
        );
        return text(refusal.status(), "text/html; charset=utf-8", &page);
    }
    let body = json!({ "error": { "code": refusal.code(), "message": refusal.message() } });
    json_response(refusal.status(), &body)
}

fn api_error(error: &ApiError) -> Response {
    json_response(error.status, &error.body())
}

fn internal() -> Response {
    api_error(&ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
        "The studio failed while it answered; the library was not changed by this request.",
    ))
}

fn json_response(status: StatusCode, body: &Value) -> Response {
    text(status, "application/json; charset=utf-8", &body.to_string())
}

fn text(status: StatusCode, content_type: &'static str, body: &str) -> Response {
    bytes_response(status, content_type, body.as_bytes().to_vec())
}

fn bytes_response(status: StatusCode, content_type: &'static str, body: Vec<u8>) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

fn redirect(location: &'static str) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::SEE_OTHER;
    response
        .headers_mut()
        .insert(header::LOCATION, HeaderValue::from_static(location));
    response
}

#[cfg(test)]
mod tests;
