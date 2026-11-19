# 0003: Release signing keys and the signing tool

Status: proposed (2026-11-19)

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

A small program in this repository, `anaquel-sign`, with two commands and no network access:

- `anaquel-sign keygen --out DIR` writes `DIR/NAME.pub` and `DIR/NAME.key` for the names `active`
  and `reserve`. It asks for the passphrase twice on the terminal (no echo, never an argument or
  a variable) and refuses an empty one. It prints only the public key line.
- `anaquel-sign sign --key FILE manifest.json` asks for the passphrase and writes
  `manifest.json.minisig`.

Formats follow minisign, so a signature made here is read by the independent verifier already used
by `anaquel-update`, and any other minisign program can check it:

- public key: `Ed` + 8-byte key id + 32-byte key, base64;
- signature: the pre-hashed form (`ED`, BLAKE2b-512 of the data) with the trusted comment signed.

The private key file is passphrase-protected with scrypt. It is stored in the layout minisign
documents, but compatibility of this one file with the official program is **not claimed**: it
cannot be tested without that program, and the tests here can only prove that this tool reads its
own files.

Where the keys live: the active key on the maintainer's machine, offline except while a release is
signed; the reserve key on separate media, never loaded on the release machine. The release
workflow does not sign. The maintainer signs the manifest by hand after the build and uploads the
`.minisig`; this keeps the private key out of CI at the cost of one manual step per release.

## Tests this decision requires

- A signature written by `sign` verifies with `anaquel-update` (an independent reader), and fails
  after one changed byte, with the other key, and with the legacy (non-pre-hashed) algorithm.
- A wrong passphrase, a truncated key file, a key file with a changed byte and an empty passphrase
  are refused with distinct errors.
- `keygen` never writes the private key before the passphrase was confirmed twice, writes it with
  owner-only permissions where the system supports it, and refuses to overwrite a file.
- Key ids of the two generated keys differ.

## Costs and limits

- New dependencies: an Ed25519 signer, scrypt, a random source and BLAKE2. None is in the local
  cache except BLAKE2, so they are fetched once and then listed in `THIRD-PARTY.md` and checked by
  `cargo deny`.
- The tool handles secrets, so it is built to hold them briefly: buffers are zeroed after use. It
  cannot defend against malware on the maintainer's machine.
- A leaked active key is replaced by shipping a version that trusts the reserve key as active and a
  new reserve; a leak of both needs a manual reinstall. The manifest's expiry limits how long a
  stolen key can be used to hide a withdrawal.
- The real keys exist only after the maintainer runs `keygen`; until then `anaquel upgrade` has
  nothing to trust and must say so instead of falling back.

## Rejected

- Downloading the official minisign program: an external binary to vet and keep up to date, for a
  job small enough to test fully here. It stays available as an independent cross-check.
- Signing in CI: puts the private key in a place several people and processes can read.
- A key stored unencrypted: a stolen disk would be a stolen release channel.
