#!/usr/bin/env python3
"""Generates the signed-manifest fixtures of the `anaquel-update` crate.

The fixtures are deterministic: Ed25519 signatures are deterministic and the
signing seeds below are fixed, public test constants. They protect nothing and
must never be reused as release keys.

    python -I tools/update_fixtures.py          write the fixtures
    python -I tools/update_fixtures.py --check  fail when the files on disk differ

Minisign file formats written here (see https://jedisct1.github.io/minisign/):

    public key   "Ed" + key id (8) + public key (32), base64
    signature    algorithm "ED" (BLAKE2b-512 pre-hashed) or "Ed" (legacy, raw data)
                 + key id (8) + signature (64), base64
    global       signature over (signature || trusted comment), base64
"""

from __future__ import annotations

import base64
import hashlib
import json
import sys
from pathlib import Path

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

FIXTURES = Path(__file__).resolve().parent.parent / "crates" / "anaquel-update" / "tests" / "fixtures"

# Public test seeds. Three distinct signers: two trusted by the tests, one foreign.
SEEDS = {
    "active": bytes([0x11]) * 32,
    "reserve": bytes([0x22]) * 32,
    "foreign": bytes([0x33]) * 32,
}

TRUSTED_COMMENT = "timestamp:1795046400 file:release-manifest.json"
DOWNLOAD_PREFIX = "https://github.com/saidcveltran/anaquel/releases/download/"

# A tiny stand-in for a release archive; the manifest promises its exact digest.
PAYLOAD = b"anaquel update fixture payload: not a real program\n"


class TestKey:
    """A deterministic Ed25519 key with a minisign key id."""

    def __init__(self, seed: bytes) -> None:
        self.private = Ed25519PrivateKey.from_private_bytes(seed)
        self.public = self.private.public_key().public_bytes(
            serialization.Encoding.Raw, serialization.PublicFormat.Raw
        )
        self.key_id = hashlib.sha256(self.public).digest()[:8]

    def public_line(self) -> str:
        return base64.b64encode(b"Ed" + self.key_id + self.public).decode()

    def signature_file(self, data: bytes, *, legacy: bool = False) -> str:
        """Writes a minisign signature; `legacy` signs the raw data without pre-hashing."""
        algorithm = b"Ed" if legacy else b"ED"
        message = data if legacy else hashlib.blake2b(data).digest()
        signature = self.private.sign(message)
        global_signature = self.private.sign(signature + TRUSTED_COMMENT.encode())
        return (
            "untrusted comment: signature from minisign secret key\n"
            f"{base64.b64encode(algorithm + self.key_id + signature).decode()}\n"
            f"trusted comment: {TRUSTED_COMMENT}\n"
            f"{base64.b64encode(global_signature).decode()}\n"
        )


def manifest_document() -> dict:
    payload_sha = hashlib.sha256(PAYLOAD).hexdigest()

    def asset(version: str, triple: str, ext: str, *, real: bool = False) -> dict:
        digest = payload_sha if real else hashlib.sha256(f"{version}-{triple}".encode()).hexdigest()
        return {
            "url": f"{DOWNLOAD_PREFIX}v{version}/anaquel-{triple}.{ext}",
            "sha256": digest,
            "size": len(PAYLOAD) if real else 1_000_000,
        }

    linux, windows = "x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"
    return {
        "schema": 1,
        "channel": "stable",
        "issued": "2026-11-19T00:00:00Z",
        "expires": "2026-12-19T00:00:00Z",
        "releases": [
            {
                "version": "0.1.1",
                "published": "2026-11-17T00:00:00Z",
                "assets": {linux: asset("0.1.1", linux, "tar.xz"), windows: asset("0.1.1", windows, "zip")},
            },
            {
                "version": "0.1.2",
                "published": "2026-11-19T00:00:00Z",
                "assets": {linux: asset("0.1.2", linux, "tar.xz", real=True), windows: asset("0.1.2", windows, "zip")},
            },
            {
                "version": "0.1.3",
                "published": "2026-11-19T00:00:00Z",
                "withdrawn": {"reason": "the installer failed on some Windows machines"},
                "assets": {linux: asset("0.1.3", linux, "tar.xz"), windows: asset("0.1.3", windows, "zip")},
            },
        ],
        "advisories": [
            {
                "id": "ANAQUEL-2026-001",
                "severity": "high",
                "affects": "<0.1.2",
                "message": "fixed in 0.1.2; upgrade as soon as possible",
            }
        ],
    }


def expected_files() -> dict[str, bytes]:
    keys = {name: TestKey(seed) for name, seed in SEEDS.items()}
    manifest = (json.dumps(manifest_document(), indent=2) + "\n").encode()
    texts = {
        "release-manifest.json": manifest.decode(),
        "release-manifest.json.minisig": keys["active"].signature_file(manifest),
        "release-manifest.reserve.minisig": keys["reserve"].signature_file(manifest),
        "release-manifest.foreign.minisig": keys["foreign"].signature_file(manifest),
        "release-manifest.legacy.minisig": keys["active"].signature_file(manifest, legacy=True),
        "active.pub": keys["active"].public_line() + "\n",
        "reserve.pub": keys["reserve"].public_line() + "\n",
        "foreign.pub": keys["foreign"].public_line() + "\n",
    }
    files = {name: text.encode() for name, text in texts.items()}
    files["payload.bin"] = PAYLOAD
    return files


def main(argv: list[str]) -> int:
    check = argv[1:] == ["--check"]
    if argv[1:] and not check:
        sys.stderr.write("usage: update_fixtures.py [--check]\n")
        return 2
    differing = []
    for name, content in expected_files().items():
        path = FIXTURES / name
        if check:
            if not path.is_file() or path.read_bytes() != content:
                differing.append(name)
        else:
            FIXTURES.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
    if differing:
        sys.stderr.write("fixtures out of date: " + ", ".join(differing) + "\n")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
