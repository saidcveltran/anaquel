# 0003: Release signing keys and the signing tool

Status: accepted and implemented in `anaquel-sign` (2026-11-19)

## Context

`anaquel-update` verifies a detached minisign signature of `release-manifest.json` against two
public keys the program carries, an active one and a reserve one. The crate embeds none. Three
things are missing before `anaquel upgrade` can trust a real release:

1. the two key pairs;
2. a way to sign the manifest in the release process;
3. the public keys inside the program.

The private keys must never be seen by a session, a log or the repository. The maintainer makes
them in their own terminal; the repository receives only the public halves.

## Decision

A small program in this repository, `anaquel-sign`, with two commands and no network access. It
is built from the repository and is not part of the release archives.

- `anaquel-sign keygen --name NAME --out DIR` writes `DIR/NAME.key` (private) and `DIR/NAME.pub`.
  Run it once for `active` and once for `reserve`, with different passphrases. It asks for the
  passphrase twice on the terminal (no echo, never an argument or a variable), refuses one shorter
  than 12 characters or with fewer than 6 different ones, never replaces a file, opens the sealed
  file again with the typed passphrase before writing anything, and prints the key id and the public
  key line. A passphrase is compared in Unicode normal form C, so an accented letter typed as one
  character or as two is the same passphrase.
- `anaquel-sign sign --key FILE --public FILE [--comment TEXT] manifest.json` asks for the
  passphrase and writes `manifest.json.minisig`. The key file is refused, before the passphrase is
  asked, when it is not a key file this version reads. The private key must be the one the public
  key file names, or nothing is signed: a reserve key chosen by mistake cannot sign a release that
  the active key is expected to sign. Before signing it checks that the manifest is one the program
  would accept; after signing it checks the signature with the same verifier the program ships,
  against that public key, and it writes nothing if either fails. It never replaces an existing
  signature file, and a file that fails while being written is removed. The trusted comment may not
  hold control characters, line or paragraph separators, or bidirectional overrides.

Public keys and signatures follow minisign, so the independent verifier in `anaquel-update` reads
what the tool writes, and any other minisign reader can check it:

- public key: `Ed` + 8-byte key id + 32-byte key, base64;
- signature: the pre-hashed form (`ED`, BLAKE2b-512 of the data), with the trusted comment signed.

The private key file is **this tool's own format**, not minisign's. An early draft of this page
planned to copy minisign's secret key layout; that was dropped because copying it from memory could
not be checked without the official program, and a file that looks compatible and is not is worse
than one that says it is different. The file's first line says so. It holds the format version, the
scrypt cost (N = 2^18, r = 8, p = 1, about 256 MiB), a random salt, the key id, the seed masked with
a stream derived from the passphrase, and a BLAKE2b-256 checksum keyed by the same derivation. A
wrong passphrase and a changed file give the same answer, because they cannot be told apart.
Opening a file refuses scrypt costs outside N 2^10 to 2^22, r up to 16, p up to 4 and 128 * r * N
of at most 1 GiB before doing any work, so a changed file cannot exhaust memory. A test pins the
exact bytes this version writes for a fixed key, passphrase and salt, so the format cannot change
without a new format version.

Where the keys live: the active key on the maintainer's machine, offline except while a release is
signed; the reserve key on separate media, never loaded on the release machine. The release
workflow does not sign. The maintainer signs the manifest by hand after the build and uploads the
`.minisig`; this keeps the private key out of CI at the cost of one manual step per release.

## Tests this decision requires, and what was checked

- A signature written by `sign` verifies with `anaquel-update`, and fails after changed data, with
  another key and after a changed trusted comment. Done.
- Every single changed byte of a key file makes it unusable; a wrong, empty or case-changed
  passphrase is refused; costs outside the bounds are refused before any stretching. Done.
- `keygen` writes nothing when the two passphrases differ, when the passphrase is short, when the
  name could leave the folder, or when a file already exists; the file writer itself also refuses
  to replace a file that appears after the check. Done.
- `sign` with a private key that is not the one the public key file names signs nothing, and a
  public key file that is missing, not text or not a key is refused before any prompt. Done.
- Hand-made mutations were caught: skipping the checksum, signing the data without hashing it,
  overwriting a file, skipping the key match, leaving a half-written file, dropping the memory
  bound, dropping the normalization, dropping the different-characters rule, narrowing the
  rejected comment characters and skipping the early format check each fail a test.
- Not done: the terminal prompt itself is not driven by a test (the same code runs against a
  scripted source), and nothing was checked on the private key file permissions on Windows.

## Costs and limits

- New dependencies: `ed25519-compact`, `scrypt`, `blake2`, `getrandom`, `zeroize`, `rpassword` and
  `base64`, with what they pull in. They are used only by this tool, which is not shipped.
- The tool holds the seed in memory that is zeroed when dropped, but the signing library keeps its
  own copies while it signs; it cannot defend against malware on the maintainer's machine.
- On Linux the private file is created readable by its owner only. On Windows it keeps the
  permissions of its folder, so keep it under your own profile; `keygen` prints a reminder there.
- `sign` does not warn about a manifest that is already expired when it is signed; the program
  refuses it when it is used, so the cost is a wasted release, not a wrong update.
- The size limit on the files read is checked from the file's metadata before reading, and the read
  is bounded as well; the tests check the sizes in the error, not the memory used.
- A leaked active key is replaced by shipping a version that trusts the reserve key as active and a
  new reserve; a leak of both needs a manual reinstall. The manifest's expiry limits how long a
  stolen key can be used to hide a withdrawal.
- The real keys exist only after the maintainer runs `keygen`; until then `anaquel upgrade` has
  nothing to trust and must say so instead of falling back.

## Rejected

- Downloading the official minisign program: an external binary to vet and keep up to date, for a
  job small enough to test fully here. It stays available as an independent cross-check of
  signatures.
- Signing in CI: puts the private key in a place several people and processes can read.
- A key stored unencrypted: a stolen disk would be a stolen release channel.
- Copying minisign's secret key layout without the official program to test against.
