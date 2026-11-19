# 0005: The `upgrade` command

Status: accepted as a design (2026-11-19), with the questions at the end settled by the
maintainer. Built so far: `anaquel-update` (the trust core), the two release public keys,
`anaquel-sign manifest` and, in `anaquel-upgrade`, the archive (`.tar.gz` and `.zip`), swap and
state pieces, and the download client with its address policy (not yet linked into the program).
The command itself is not built.

## Context

`anaquel-update` decides what may be believed and installed, and has no network, clock or file
access. `anaquel-sign` signs the release manifest. The two public keys are in the program. What
is missing is the command that does the work: fetch the signed manifest, ask `decide`, download
the archive, check it, replace the program and keep a way back.

The project's own rule is no hidden network use: the program contacts nothing unless the user
runs a command that says so.

## Decision

`anaquel upgrade [--check] [--version X.Y.Z] [--allow-downgrade] [--rollback]`

- **Only on command by default.** Out of the box there is no check at start-up, no timer and no
  telemetry. `--check` reports what `upgrade` would do and changes nothing. The only host
  contacted is the project's release address (GitHub releases), and only over HTTPS.
- **An optional notice, off by default.** The maintainer chose to offer it. `anaquel upgrade
  --notice on` turns it on and `--notice off` turns it off; the choice is kept in a small
  settings file in the user's state folder, not in a library's `anaquel.toml`. When on, at most
  once in 24 hours, after `build`, `check` or `doctor` has finished, the program reads the signed
  manifest with a short time limit and prints one line to the error output if a newer version
  exists. It never delays or changes the exit code of the command, a failure is silent, it is
  skipped when the `CI` variable is set, and it never installs anything. It is built after the
  command itself, as a second step.
- **Order of trust**, which is the order of the steps: download `release-manifest.json` and its
  `.minisig` (size-limited), `verify_manifest` with `TrustedKeys::release()`, `decide` with the
  platform, channel, the clock, the last manifest time seen and the project's URL prefix, then
  download the one archive the plan names, check its size and SHA-256 with `DigestCheck`, and only
  then touch the installed program. Any refusal stops before any file is changed and says why in
  words, with the reason the library returned.
- **Last manifest time seen** is kept in a one-line file in the user's state folder, and is
  written only after the signature of a manifest has been verified, so a replayed older manifest
  is refused. Its location uses the standard variables of each system and no new dependency.
- **Replacing the program.** The new program is unpacked beside the installed one, tested by
  running its `version`, then swapped in: the installed file is renamed to a `.previous` name and
  the new one takes its place. Windows allows renaming a running program but not overwriting it,
  and the same two steps are atomic enough on Linux. If any step fails the rename is undone.
  `--rollback` swaps the `.previous` file back. One previous version is kept, no more.
- **Where it refuses.** When the installed program is not writable (installed under a system
  folder), or sits where a package manager owns it (Scoop, winget), the command says to use that
  tool and does nothing.
- **Archives.** `dist` writes `.tar.xz` for Linux today. Reading it needs a C library for LZMA
  or a slower pure-Rust one. The release configuration is changed to `.tar.gz`, which a small
  pure-Rust crate reads. Windows keeps `.zip`: on 2026-11-19 the maintainer chose one `.tar.gz`
  format for both, then went back after the documentation of winget was read: its manifest
  documents zip archives (nested installers, since winget 1.5) and no tar archive, and Scoop's
  page lists `.tar` and `.gz` but does not name `.tar.gz`. Neither is proof that a `.tar.gz`
  would be refused, but the one format both document is `.zip`, and the last step of this page
  is those two packages. So `anaquel-upgrade` reads both formats, with the `zip` library at a
  version more than two weeks old. This changes the Linux archive names of the next release
  only; the installers follow the configuration.
- **Network library.** A small blocking HTTPS client with a pure-Rust TLS stack, added to the
  program (`ureq`). The release address answers with a redirect to GitHub's download hosts, so
  redirects are followed only within a short list of GitHub hosts and only over HTTPS; integrity
  does not depend on the host because the digest comes from the signed manifest.
- **The manifest has to be made.** A step of the release produces `release-manifest.json` from
  the checksums and sizes of the archives of that release, and the maintainer signs it by hand
  with `anaquel-sign` and uploads the `.minisig`, as ADR 0003 describes. A command of
  `anaquel-sign` writes the manifest from the release plan so no one types a hash.

## Tests this decision requires

- Every refusal of `decide` reaches the user as a message and leaves the installed program
  byte-for-byte as it was: expired, old, withdrawn, wrong channel, downgrade, foreign address,
  missing platform, bad signature, wrong digest, wrong size, an archive that is too large.
- A local test server serves a signed manifest and an archive, so the whole path runs without the
  internet: success, the swap, `--rollback`, a failure half way (the old program must still run),
  and a server that sends a different archive than the manifest promised.
- The swap and the rollback run on Windows and on Linux in CI, including a program that is
  running while it is replaced.
- The test server cannot be reached by default: the address of the release is a constant, and
  the tests supply another one through a function argument and not through an environment
  variable a user could set.

## Costs and the best argument against

- It adds a network client and an archive reader to a program that has had neither, and with them
  more code that handles hostile input. The preview server is the only network code today and
  listens on `127.0.0.1` only.
- A pure-Rust TLS stack may need a C compiler or assembler for one of the five targets (Windows
  ARM64 and static Linux are the likely ones); CI shows it, and if one target cannot have it the
  command is left out of that build with a message and not faked.
- The strongest argument against: package managers and the installer script already update the
  program, and each is independently maintained. The reply is that the installers do not check
  the project's signature and that the project asked for a verified, reversible path.

- The notice is the strongest source of risk in this page: it is the one place the program
  contacts a server without the user typing a command that says so. It stays off unless asked, and
  its first version may be dropped if it proves to cause trouble. The recommendation was to have
  no such check; the maintainer chose to offer it.

## Settled by the maintainer (2026-11-19)

1. The version check is an optional notice, off by default, and not a rule of no check at all.
2. The Linux archives change to `.tar.gz`; Windows stays `.zip`. The next release's file names
   change.
3. `anaquel-sign` gets a command that writes the manifest from the release plan.

Revised by the maintainer later on 2026-11-19, with the reasons:

2. (revised) **Linux `.tar.gz`, Windows `.zip`, both read.** The maintainer first chose one
   `.tar.gz` format for both, to avoid the `zip` library, which published a new version nearly
   every day in November 2026 (9.4 to 9.7 in two weeks). The documentation of winget names zip
   archives and no tar archive, and Scoop's page does not name `.tar.gz`, so Windows keeps `.zip`
   and the library is added at a version more than two weeks old. `anaquel-upgrade` reads both.
4. **The HTTPS client is `ureq` with `rustls` and the `ring` provider.** Everything in Rust, with
   no dependence on `curl` or PowerShell. Costs stated when it was chosen: `deny.toml` does not
   admit the licence of the root certificate bundle (CDLA-Permissive-2.0), so the licence policy
   is extended for that one crate by name and not in general; the binary grows by a few MiB;
   `ring` needs a C compiler or assembler on Windows ARM64 and musl, which CI has not yet shown.
   Corrected when the client was built: the choice text first said `ring` also needed a licence
   exception. Its manifest declares `Apache-2.0 AND ISC`, and both are already admitted, so it
   needs none.

## Order of work

1. `anaquel-sign` writes the manifest from the archive names, sizes and sums of a release.
2. `anaquel upgrade`, `--check` and `--rollback`, against a local test server.
3. The release configuration moves to `.tar.gz`.
4. The optional notice.
5. The Scoop and winget packages, which point at the same signed archives.
