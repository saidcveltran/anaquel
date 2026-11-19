//! The one place where `anaquel upgrade` touches the network.
//!
//! Nothing here decides what to trust: the manifest is verified and the archive is checked
//! against the digest the manifest names by `anaquel-update`, after the bytes arrive. What this
//! module guarantees is narrower and independent of that check:
//!
//! - only addresses of an explicit [`Policy`] are ever contacted, on every hop of a redirect;
//! - the release policy admits `https` and a short list of GitHub hosts, nothing else;
//! - a body is cut off at a limit the caller states, and a time limit covers the whole download;
//! - redirects are followed by this code, one hop at a time, and not by the HTTP library.
//!
//! The network is reached through the [`Fetch`] trait, so the code that orchestrates an upgrade
//! can be tested with a fake and the real client with a server on the loopback interface.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use ureq::Agent;
use ureq::config::Config;
use ureq::http::Uri;
use ureq::http::header::LOCATION;

/// Hosts that serve the files of a GitHub release: the address of the release, and the two
/// hosts it answers with a redirect to. Compared whole and case-insensitively; a name that merely
/// ends with one of these, or contains one, is not admitted.
const RELEASE_HOSTS: [&str; 3] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// Redirects followed before giving up. A release download takes one or two.
const MAX_REDIRECTS: u32 = 5;

/// Longest address kept in an error message; a redirect can name an address of any length.
const SHOWN_ADDRESS_BYTES: usize = 200;

/// Size of the buffer a body is copied through.
const COPY_BUFFER_BYTES: usize = 64 * 1024;

/// What the program sends as `User-Agent`: the crate and its version, nothing about the machine.
const USER_AGENT: &str = concat!("anaquel-upgrade/", env!("CARGO_PKG_VERSION"));

/// Why a download did not happen.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// An address, given by the caller or named by a redirect, is outside the policy. Nothing
    /// was sent to it.
    #[error("the address `{address}` is not one this program may contact: {reason}")]
    Refused {
        /// The address, cut to a readable length.
        address: String,
        /// The rule it broke.
        reason: &'static str,
    },
    /// The server redirected more often than the limit allows.
    #[error("the server redirected more than {MAX_REDIRECTS} times")]
    TooManyRedirects,
    /// A redirect had no usable `Location`.
    #[error("the server sent a redirect without a usable destination")]
    BadRedirect,
    /// The server answered with a status that is neither success nor a redirect.
    #[error("the server answered with status {0}")]
    Status(u16),
    /// The body is larger than the limit the caller stated.
    #[error("the download is larger than the {limit} bytes allowed")]
    TooLarge {
        /// The limit that was exceeded, in bytes.
        limit: u64,
    },
    /// The whole download did not finish within the time allowed.
    #[error("the download did not finish in time")]
    TimedOut,
    /// The connection or the exchange failed.
    #[error("the network request failed: {0}")]
    Transport(String),
    /// The destination of the bytes refused them.
    #[error("the downloaded bytes could not be written: {0}")]
    Sink(#[source] io::Error),
}

/// A source of files by address. The one implementation that reaches the network is
/// [`HttpFetch`]; anything that orchestrates an upgrade takes this trait, so a test can hand it
/// a fake.
pub trait Fetch {
    /// Download the file at `address` into `sink`, at most `max_bytes` of it, and return the
    /// number of bytes written. When the download fails half way the bytes already written stay
    /// in `sink`: the caller owns it and discards it.
    ///
    /// # Errors
    ///
    /// A [`FetchError`] that says which rule or step stopped the download.
    fn fetch(&self, address: &str, max_bytes: u64, sink: &mut dyn Write)
    -> Result<u64, FetchError>;
}

/// Download into memory, for the small files (the manifest and its signature).
///
/// # Errors
///
/// As [`Fetch::fetch`]. A body of more than `max_bytes` is [`FetchError::TooLarge`].
pub fn fetch_to_vec(
    fetch: &dyn Fetch,
    address: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, FetchError> {
    let mut bytes = Vec::new();
    fetch.fetch(address, max_bytes, &mut bytes)?;
    Ok(bytes)
}

/// Which addresses may be contacted and how long a download may take.
#[derive(Debug, Clone)]
pub struct Policy {
    scope: Scope,
    connect_timeout: Duration,
    total_timeout: Duration,
}

/// The family of addresses a [`Policy`] admits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// `https` to the hosts of [`RELEASE_HOSTS`], on the default port.
    Release,
    /// Plain `http` to the loopback interface, on any port: a server a test starts itself.
    Loopback,
}

impl Policy {
    /// The policy of the program: `https` to the GitHub release hosts, 15 seconds to connect and
    /// 10 minutes for a whole download.
    #[must_use]
    pub fn release() -> Self {
        Self {
            scope: Scope::Release,
            connect_timeout: Duration::from_secs(15),
            total_timeout: Duration::from_secs(600),
        }
    }

    /// Plain `http` to `127.0.0.1` and `[::1]` and nothing else. The program never builds this;
    /// it exists so tests can run the whole download against a server of their own. It is a
    /// function argument and not a setting a user could change through the environment.
    #[must_use]
    pub fn loopback() -> Self {
        Self {
            scope: Scope::Loopback,
            ..Self::release()
        }
    }

    /// The same policy with other time limits.
    #[must_use]
    pub fn with_timeouts(mut self, connect: Duration, total: Duration) -> Self {
        self.connect_timeout = connect;
        self.total_timeout = total;
        self
    }

    /// Check one address, parsed once and then used as it is, so that what is checked and what
    /// is contacted cannot differ.
    fn admit(&self, address: &str) -> Result<Uri, FetchError> {
        let refuse = |reason| FetchError::Refused {
            address: shown(address),
            reason,
        };
        let uri: Uri = address
            .parse()
            .map_err(|_| refuse("it is not a valid address"))?;
        let authority = uri.authority().ok_or_else(|| refuse("it names no host"))?;
        if authority.as_str().contains('@') {
            return Err(refuse("it carries a user name"));
        }
        let scheme = uri.scheme_str().ok_or_else(|| refuse("it has no scheme"))?;
        let host = authority.host();
        match self.scope {
            Scope::Release => {
                if scheme != "https" {
                    return Err(refuse("only https is allowed"));
                }
                if authority.port().is_some() {
                    return Err(refuse("a port is not allowed"));
                }
                if !RELEASE_HOSTS
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(host))
                {
                    return Err(refuse("the host is not a GitHub release host"));
                }
            }
            Scope::Loopback => {
                if scheme != "http" {
                    return Err(refuse("only http is allowed here"));
                }
                if host != "127.0.0.1" && host != "[::1]" {
                    return Err(refuse("only the loopback interface is allowed here"));
                }
            }
        }
        Ok(uri)
    }
}

/// The client that reaches the network, bound to one [`Policy`].
#[derive(Debug)]
pub struct HttpFetch {
    policy: Policy,
    agent: Agent,
}

impl HttpFetch {
    /// A client bound to `policy`. The HTTP library is told not to follow redirects, to accept
    /// any status as an answer, and, for the release policy, to refuse anything but `https`:
    /// this module's own checks come first and the library's are a second wall behind them.
    #[must_use]
    pub fn new(policy: Policy) -> Self {
        let builder = Config::builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .https_only(policy.scope == Scope::Release)
            .user_agent(USER_AGENT)
            .timeout_connect(Some(policy.connect_timeout));
        // The loopback policy never goes through a proxy a test machine may have set up.
        let config = if policy.scope == Scope::Loopback {
            builder.proxy(None).build()
        } else {
            builder.build()
        };
        Self {
            policy,
            agent: Agent::new_with_config(config),
        }
    }

    /// The client of the program: [`Policy::release`].
    #[must_use]
    pub fn release() -> Self {
        Self::new(Policy::release())
    }
}

impl Fetch for HttpFetch {
    fn fetch(
        &self,
        address: &str,
        max_bytes: u64,
        sink: &mut dyn Write,
    ) -> Result<u64, FetchError> {
        let started = Instant::now();
        let mut uri = self.policy.admit(address)?;
        for _ in 0..=MAX_REDIRECTS {
            let remaining = self
                .policy
                .total_timeout
                .checked_sub(started.elapsed())
                .filter(|left| !left.is_zero())
                .ok_or(FetchError::TimedOut)?;
            let mut response = self
                .agent
                .get(uri.clone())
                .config()
                .timeout_global(Some(remaining))
                .build()
                .call()
                .map_err(transport_error)?;
            let status = response.status().as_u16();
            match status {
                200 => {
                    if response
                        .body()
                        .content_length()
                        .is_some_and(|n| n > max_bytes)
                    {
                        return Err(FetchError::TooLarge { limit: max_bytes });
                    }
                    let mut reader = response.body_mut().as_reader();
                    return copy_limited(&mut reader, max_bytes, sink);
                }
                301 | 302 | 303 | 307 | 308 => {
                    let location = response
                        .headers()
                        .get(LOCATION)
                        .and_then(|value| value.to_str().ok())
                        .ok_or(FetchError::BadRedirect)?;
                    uri = self.policy.admit(&resolve(&uri, location)?)?;
                }
                other => return Err(FetchError::Status(other)),
            }
        }
        Err(FetchError::TooManyRedirects)
    }
}

/// Turn the `Location` of a redirect into an address: an absolute one as it stands, a path that
/// starts with a single `/` on the origin of the answer. Anything else, including an address
/// without a scheme (`//host/path`), is refused.
fn resolve(from: &Uri, location: &str) -> Result<String, FetchError> {
    if location.starts_with('/') && !location.starts_with("//") {
        let scheme = from.scheme_str().ok_or(FetchError::BadRedirect)?;
        let authority = from.authority().ok_or(FetchError::BadRedirect)?;
        return Ok(format!("{scheme}://{authority}{location}"));
    }
    if location.contains("://") {
        return Ok(location.to_owned());
    }
    Err(FetchError::BadRedirect)
}

/// Copy a body into `sink`, refusing to write more than `max_bytes`. A read error is the
/// network's; a write error is the sink's.
fn copy_limited(
    reader: &mut dyn Read,
    max_bytes: u64,
    sink: &mut dyn Write,
) -> Result<u64, FetchError> {
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut total: u64 = 0;
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => return Ok(total),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(read_error(&error)),
        };
        total += read as u64;
        if total > max_bytes {
            return Err(FetchError::TooLarge { limit: max_bytes });
        }
        sink.write_all(&buffer[..read]).map_err(FetchError::Sink)?;
    }
}

/// Map an error of the HTTP library to the module's own.
fn transport_error(error: ureq::Error) -> FetchError {
    match error {
        ureq::Error::Timeout(_) => FetchError::TimedOut,
        other => FetchError::Transport(other.to_string()),
    }
}

/// Map an error met while reading a body.
fn read_error(error: &io::Error) -> FetchError {
    if error.kind() == io::ErrorKind::TimedOut {
        FetchError::TimedOut
    } else {
        FetchError::Transport(error.to_string())
    }
}

/// An address cut to a length that is readable in a message.
fn shown(address: &str) -> String {
    if address.len() <= SHOWN_ADDRESS_BYTES {
        return address.to_owned();
    }
    let mut end = SHOWN_ADDRESS_BYTES;
    while !address.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &address[..end])
}

#[cfg(test)]
mod loopback;

#[cfg(test)]
mod tests;
