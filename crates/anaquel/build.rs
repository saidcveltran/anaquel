//! Records the target triple this program is built for, so that `anaquel upgrade` asks for the
//! release file of its own platform: `x86_64-unknown-linux-gnu` and `x86_64-unknown-linux-musl`
//! are different programs and the operating system and architecture alone cannot tell them apart.

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Ok(target) = std::env::var("TARGET") else {
        let _ = writeln!(
            std::io::stderr(),
            "cargo did not say which TARGET this is built for"
        );
        return ExitCode::FAILURE;
    };
    let _ = writeln!(
        std::io::stdout(),
        "cargo:rustc-env=ANAQUEL_TARGET_TRIPLE={target}"
    );
    let _ = writeln!(std::io::stdout(), "cargo:rerun-if-changed=build.rs");
    ExitCode::SUCCESS
}
