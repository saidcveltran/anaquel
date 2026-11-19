//! `anaquel studio` on the real program: it starts, prints an address that carries the key, and
//! that address opens a session; the other things that can go wrong end with a message and an
//! exit code, not with a server that listens on a folder that is not a library.

#[cfg(test)]
mod studio_command {
    use std::fs;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpStream;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    const PROGRAM: &str = env!("CARGO_BIN_EXE_anaquel");

    /// A library folder under the system's temporary folder, removed when it goes out of scope.
    struct Library(PathBuf);

    impl Library {
        fn new(name: &str) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "anaquel-studio-command-{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(path.join("volumes/guide")).unwrap();
            fs::write(
                path.join("anaquel.toml"),
                "schema = 1\nengine = \"0.2\"\ntitle = \"Demo\"\n",
            )
            .unwrap();
            fs::write(
                path.join("volumes/guide/volume.toml"),
                "title = \"Guide\"\n",
            )
            .unwrap();
            fs::write(path.join("volumes/guide/setup.md"), "# Setup\n").unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Library {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A running `anaquel studio`, stopped when it goes out of scope.
    struct Running {
        child: Child,
        lines: Vec<String>,
    }

    impl Running {
        /// Starts the program and reads its output up to the line that says how to stop it.
        fn start(root: &Path, extra: &[&str]) -> Self {
            let mut child = Command::new(PROGRAM)
                .arg("studio")
                .arg(root)
                .args(extra)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let mut reader = BufReader::new(child.stdout.take().unwrap());
            let mut lines = Vec::new();
            loop {
                let mut line = String::new();
                let read = reader.read_line(&mut line).unwrap();
                assert_ne!(read, 0, "the program ended early; it printed {lines:?}");
                let line = line.trim_end().to_owned();
                let done = line.contains("Ctrl+C");
                lines.push(line);
                if done {
                    break;
                }
            }
            Self { child, lines }
        }

        fn address(&self) -> &str {
            self.lines
                .iter()
                .find(|line| line.starts_with("http://"))
                .expect("the program prints the address")
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// Splits `http://127.0.0.1:PORT/PATH?QUERY` into the port and the rest.
    fn split(address: &str) -> (u16, String) {
        let rest = address
            .strip_prefix("http://127.0.0.1:")
            .expect("a loopback address");
        let (port, path) = rest.split_once('/').expect("a path");
        (port.parse().unwrap(), format!("/{path}"))
    }

    fn get(port: u16, path: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        raw
    }

    #[test]
    fn the_program_prints_an_address_that_opens_a_session() {
        let library = Library::new("start");
        let running = Running::start(library.path(), &[]);
        let (port, path) = split(running.address());
        assert_ne!(port, 0);
        assert!(path.starts_with("/__studio/enter?token="), "{path}");
        let key = path.rsplit('=').next().unwrap();
        assert_eq!(key.len(), 64);

        let reply = get(port, &path);
        assert!(reply.starts_with("HTTP/1.1 303"), "{reply}");
        assert!(reply.contains("location: /__studio/"), "{reply}");
        assert!(reply.contains(&format!("anaquel_studio={key}; HttpOnly; SameSite=Strict")));

        // The address without its key opens nothing.
        let refused = get(port, "/__studio/enter");
        assert!(refused.starts_with("HTTP/1.1 401"), "{refused}");
        assert!(!refused.contains(key));
    }

    #[test]
    fn the_key_is_printed_once_and_a_second_run_has_another() {
        let library = Library::new("keys");
        let first = Running::start(library.path(), &[]);
        let second = Running::start(library.path(), &[]);
        assert_ne!(first.address(), second.address());
        assert_eq!(
            first
                .lines
                .iter()
                .filter(|line| line.contains("token="))
                .count(),
            1
        );
    }

    #[test]
    fn a_chosen_port_is_the_one_used() {
        let library = Library::new("port");
        let probe = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let running = Running::start(library.path(), &["--port", &port.to_string()]);
        assert_eq!(split(running.address()).0, port);
    }

    #[test]
    fn a_folder_that_is_not_a_library_ends_with_a_message_and_an_error_code() {
        let empty = std::env::temp_dir().join(format!(
            "anaquel-studio-command-empty-{}",
            std::process::id()
        ));
        fs::create_dir_all(&empty).unwrap();
        let output = Command::new(PROGRAM)
            .arg("studio")
            .arg(&empty)
            .output()
            .unwrap();
        let _ = fs::remove_dir_all(&empty);
        assert_eq!(output.status.code(), Some(2));
        let message = String::from_utf8_lossy(&output.stderr);
        assert!(message.contains("anaquel.toml"), "{message}");
        assert!(String::from_utf8_lossy(&output.stdout).is_empty());
    }

    #[test]
    fn a_folder_that_does_not_exist_ends_with_an_error_code() {
        let output = Command::new(PROGRAM)
            .args(["studio", "this-folder-does-not-exist"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stdout).is_empty());
    }

    #[test]
    fn a_port_that_is_taken_ends_with_a_hint_and_an_error_code() {
        let library = Library::new("taken");
        let holder = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = holder.local_addr().unwrap().port();
        let output = Command::new(PROGRAM)
            .arg("studio")
            .arg(library.path())
            .args(["--port", &port.to_string()])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let message = String::from_utf8_lossy(&output.stderr);
        assert!(message.contains("--port"), "{message}");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("token="));
    }

    #[test]
    fn doctor_and_help_mention_the_command() {
        let doctor = Command::new(PROGRAM).arg("doctor").output().unwrap();
        assert!(String::from_utf8_lossy(&doctor.stdout).contains("anaquel studio"));
        let help = Command::new(PROGRAM)
            .args(["studio", "--help"])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&help.stdout);
        assert!(text.contains("--port"), "{text}");
        assert!(text.contains("127.0.0.1"), "{text}");
    }
}
