//! A scripted server on the loopback interface, for the tests of [`super`].
//!
//! It answers one connection per scripted reply, in order, and records the head of each request
//! it received. Every reply closes its connection, so a client that would reuse a connection
//! cannot hide a missing request.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// What the server does with one connection.
pub enum Reply {
    /// Send these bytes and close.
    Send(Vec<u8>),
    /// Read the request, send nothing and wait for the client to give up.
    Stall,
}

impl Reply {
    /// A `200` with a body whose length is declared.
    pub fn ok(body: &[u8]) -> Self {
        let mut bytes = head("200 OK", &[("Content-Length", body.len().to_string())]);
        bytes.extend_from_slice(body);
        Self::Send(bytes)
    }

    /// A `200` that declares `declared` bytes and sends only `body`, then closes.
    pub fn short(declared: usize, body: &[u8]) -> Self {
        let mut bytes = head("200 OK", &[("Content-Length", declared.to_string())]);
        bytes.extend_from_slice(body);
        Self::Send(bytes)
    }

    /// A `200` in chunked encoding, with no declared length, that sends `chunks` and ends.
    pub fn chunked(chunks: &[&[u8]]) -> Self {
        let mut bytes = head("200 OK", &[("Transfer-Encoding", "chunked".to_owned())]);
        for chunk in chunks {
            bytes.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
            bytes.extend_from_slice(chunk);
            bytes.extend_from_slice(b"\r\n");
        }
        bytes.extend_from_slice(b"0\r\n\r\n");
        Self::Send(bytes)
    }

    /// A redirect with the given status line and `Location`.
    pub fn redirect(status: &str, location: &str) -> Self {
        Self::Send(head(
            status,
            &[
                ("Location", location.to_owned()),
                ("Content-Length", "0".to_owned()),
            ],
        ))
    }

    /// A redirect (status 302) with no `Location`.
    pub fn redirect_without_location() -> Self {
        Self::Send(head("302 Found", &[("Content-Length", "0".to_owned())]))
    }

    /// An answer that carries only a status line.
    pub fn status(line: &str) -> Self {
        Self::Send(head(line, &[("Content-Length", "0".to_owned())]))
    }
}

/// The head of a response that closes the connection after it.
fn head(status_line: &str, headers: &[(&str, String)]) -> Vec<u8> {
    let mut fields = String::new();
    for (name, value) in headers {
        // Writing into a `String` cannot fail.
        let _ = write!(fields, "{name}: {value}\r\n");
    }
    format!("HTTP/1.1 {status_line}\r\nConnection: close\r\n{fields}\r\n").into_bytes()
}

/// Serializes the binding of loopback ports inside the test process.
///
/// The operating system hands a port that was just released to the next `bind` on port 0, so
/// a test that needs a port nobody listens on would otherwise race with the servers that other
/// tests start in parallel: its client could connect to one of them and consume its only reply.
static PORT_BINDING: Mutex<()> = Mutex::new(());

/// Run `action` with the address of `path` on a loopback port that nobody listens on.
///
/// No server of this module can bind a port while `action` runs, so the port cannot be taken
/// by another test before the client has tried it.
pub fn with_unused_port<T>(path: &str, action: impl FnOnce(&str) -> T) -> T {
    let _binding = PORT_BINDING.lock().unwrap_or_else(PoisonError::into_inner);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a free loopback port");
    let port = listener.local_addr().expect("local address").port();
    drop(listener);
    action(&format!("http://127.0.0.1:{port}{path}"))
}

/// A running scripted server.
pub struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
    worker: JoinHandle<Vec<String>>,
}

impl Server {
    /// Start the server on a free port of `127.0.0.1`, with one reply per expected connection.
    pub fn start(script: Vec<Reply>) -> Self {
        let listener = {
            let _binding = PORT_BINDING.lock().unwrap_or_else(PoisonError::into_inner);
            TcpListener::bind("127.0.0.1:0").expect("bind a free loopback port")
        };
        listener
            .set_nonblocking(true)
            .expect("make accept non-blocking");
        let port = listener.local_addr().expect("local address").port();
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let stop = Arc::clone(&stop);
            thread::spawn(move || serve(&listener, script, &stop))
        };
        Self { port, stop, worker }
    }

    /// The address of `path` on this server.
    pub fn address(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// Stop the server, waiting for the connection it may be in the middle of, and return the
    /// request heads it received, in order.
    pub fn finish(self) -> Vec<String> {
        self.stop.store(true, Ordering::SeqCst);
        self.worker.join().expect("the scripted server panicked")
    }
}

/// The server loop: accept without blocking so the stop flag is seen even when no client came.
fn serve(listener: &TcpListener, script: Vec<Reply>, stop: &AtomicBool) -> Vec<String> {
    let mut heads = Vec::new();
    for reply in script {
        let Some(mut stream) = accept(listener, stop) else {
            break;
        };
        heads.push(read_head(&mut stream));
        match reply {
            Reply::Send(bytes) => {
                // The client may close first, for example when it refuses an oversized body.
                let _ = stream.write_all(&bytes);
                let _ = stream.flush();
                let _ = stream.shutdown(Shutdown::Both);
            }
            Reply::Stall => wait_for_close(&mut stream, stop),
        }
    }
    heads
}

/// Wait for a connection, or return `None` once the stop flag is raised.
fn accept(listener: &TcpListener, stop: &AtomicBool) -> Option<TcpStream> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .expect("make the stream blocking");
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("set the read timeout");
                return Some(stream);
            }
            Err(_) if stop.load(Ordering::SeqCst) => return None,
            Err(_) => thread::sleep(Duration::from_millis(2)),
        }
    }
}

/// Read up to the blank line that ends the request head.
fn read_head(stream: &mut TcpStream) -> String {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&head).into_owned()
}

/// Hold the connection open until the client closes it, the stop flag is raised, or five seconds
/// pass.
fn wait_for_close(stream: &mut TcpStream, stop: &AtomicBool) {
    stream
        .set_read_timeout(Some(Duration::from_millis(20)))
        .expect("set the read timeout");
    let limit = Instant::now() + Duration::from_secs(5);
    let mut byte = [0_u8; 1];
    while Instant::now() < limit && !stop.load(Ordering::SeqCst) {
        if let Ok(0) = stream.read(&mut byte) {
            return;
        }
    }
}
