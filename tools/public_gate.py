"""Looks for private data in the repository tree and in its commit history.

Usage:  python -I tools/public_gate.py [REPO] [--private FILE] [--no-history]

Exit status 0 means clean, 1 means findings (each is printed), 2 means the gate could not run.

What it looks for, with no help: a user folder of a Windows or Unix machine, a trace of an AI
tool in a file or in a commit message, a key or token, an e-mail address that is not a public
GitHub identity, and, in binary files, editor metadata. `--private FILE` adds names that must
never appear, for a maintainer whose list is itself private; the file is outside the repository.
Its lines are `name REGEX`, `email ADDRESS` and `binary TEXT`; `#` starts a comment.
"""

import argparse
import pathlib
import re
import subprocess
import sys

# Pieces are joined so that this file does not contain the strings it searches for.
TEXT_PATTERNS = [
    ("user folder (Windows)", re.compile(r"[A-Za-z]:\\\\?Users\\|[A-Za-z]:/Users/", re.I)),
    ("user folder (Unix)", re.compile(r"/home/[a-z0-9_-]+/|/Users/[A-Za-z0-9_-]+/")),
    (
        "trace of an AI tool",
        re.compile("Co-Authored-" "By|Generated " "with|claude" r"\.ai|anthro" "pic", re.I),
    ),
    (
        "key or token",
        re.compile(
            r"AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY|ghp_[A-Za-z0-9]{20,}"
            r"|xox[bap]-[A-Za-z0-9-]{10,}|sk-[A-Za-z0-9]{20,}"
        ),
    ),
]
EMAIL = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
# Public GitHub identities: they appear on commits and merges made through the web interface.
PUBLIC_EMAIL = re.compile(r"^([0-9]+\+)?[a-z0-9-]+@users\.noreply\.github\.com$|^noreply@github\.com$", re.I)
BINARY_NEEDLES = [b"Exif", b"C:\\Users"]
# Files that name the patterns above and so cannot pass their own search.
SELF = {"tools/public_gate.py", "tools/test_gates.py"}


class Gate:
    """Collects findings for one run."""

    def __init__(self, names=(), emails=(), binary=()):
        self.names = [("private name", re.compile(pattern, re.I)) for pattern in names]
        self.emails = {address.lower() for address in emails}
        self.binary = [text.encode("utf-8") for text in binary]
        self.findings = set()

    def email_ok(self, address):
        return bool(PUBLIC_EMAIL.match(address)) or address.lower() in self.emails

    def scan_text(self, label, text):
        for kind, pattern in TEXT_PATTERNS + self.names:
            for found in pattern.finditer(text):
                self.findings.add(f"{label}: {kind}: {found.group(0)!r}")
        for found in EMAIL.finditer(text):
            if not self.email_ok(found.group(0)):
                self.findings.add(f"{label}: e-mail address: {found.group(0)!r}")

    def scan_bytes(self, label, data):
        if b"\0" not in data[:4096]:
            self.scan_text(label, data.decode("utf-8", "replace"))
            return
        for needle in BINARY_NEEDLES + self.binary:
            if needle in data:
                self.findings.add(f"{label}: binary file contains {needle!r}")

    def scan_history(self, log):
        self.scan_text("commit history", log)
        for line in log.split("\n"):
            identity = re.match(r"(.*) <(.*)>$", line)
            if identity and not self.email_ok(identity.group(2)):
                self.findings.add(f"commit history: identity {line!r}")


def git(repo, *args):
    done = subprocess.run(
        ["git", "-C", str(repo), *args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if done.returncode != 0:
        raise RuntimeError(done.stderr.strip() or f"git {' '.join(args)} failed")
    return done.stdout


def read_private(path):
    names, emails, binary = [], [], []
    for number, raw in enumerate(pathlib.Path(path).read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        kind, _, value = line.partition(" ")
        target = {"name": names, "email": emails, "binary": binary}.get(kind)
        if target is None or not value.strip():
            raise ValueError(f"{path}:{number}: expected `name`, `email` or `binary` and a value")
        target.append(value.strip())
    return names, emails, binary


def run(repo, private=None, history=True):
    gate = Gate(*(read_private(private) if private else ()))
    listing = git(repo, "ls-files", "--cached", "--others", "--exclude-standard")
    for relative in filter(None, listing.split("\n")):
        path = pathlib.Path(repo) / relative
        if relative in SELF or not path.is_file():
            continue
        gate.scan_bytes(relative, path.read_bytes())
    if history:
        gate.scan_history(git(repo, "log", "--all", "--format=%an <%ae>%n%cn <%ce>%n%B%n----"))
    return sorted(gate.findings)


def main(argv):
    parser = argparse.ArgumentParser(description="Looks for private data before anything is public.")
    parser.add_argument("repo", nargs="?", default=".")
    parser.add_argument("--private", help="file with names that must never appear")
    parser.add_argument("--no-history", action="store_true", help="do not read commit messages")
    arguments = parser.parse_args(argv)
    try:
        findings = run(
            pathlib.Path(arguments.repo).resolve(), arguments.private, not arguments.no_history
        )
    except (RuntimeError, ValueError, OSError) as error:
        sys.stdout.write(f"public gate could not run: {error}\n")
        return 2
    if findings:
        sys.stdout.write("\n".join(findings) + f"\n{len(findings)} findings\n")
        return 1
    sys.stdout.write("clean\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
