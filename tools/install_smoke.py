"""Install smoke test: unpack a release archive and run the program from where it landed.

Usage:  python -I tools/install_smoke.py ARCHIVE [--sha256 HEX | --sums FILE]

Checks the SHA-256 of the archive (when given), unpacks it into a new empty folder, finds
the `anaquel` program, and runs `version`, `new`, `check --strict` and `build` with it.
Nothing is installed outside the temporary folder, which is removed at the end.
Standard library only; works on Windows, Linux and macOS.
"""

import hashlib
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path


def sha256_of(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def expected_from_sums(sums_file, name):
    for line in Path(sums_file).read_text(encoding="utf-8").splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[-1].lstrip("*") == name:
            return parts[0].lower()
    return None


def unpack(archive, target):
    if zipfile.is_zipfile(archive):
        with zipfile.ZipFile(archive) as bundle:
            for member in bundle.namelist():
                if member.startswith("/") or ".." in Path(member).parts:
                    raise ValueError("unsafe path in archive: " + member)
            bundle.extractall(target)
        return
    with tarfile.open(archive) as bundle:
        for member in bundle.getmembers():
            if member.name.startswith("/") or ".." in Path(member.name).parts:
                raise ValueError("unsafe path in archive: " + member.name)
            if member.issym() or member.islnk():
                raise ValueError("link in archive: " + member.name)
        bundle.extractall(target)


def find_program(root):
    wanted = {"anaquel", "anaquel.exe"}
    for path in sorted(Path(root).rglob("*")):
        if path.is_file() and path.name in wanted:
            return path
    return None


def run(step, args, cwd):
    result = subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=300)
    sys.stdout.write(f"{step}: exit {result.returncode}\n")
    if result.returncode != 0:
        sys.stderr.write(result.stdout + result.stderr)
        raise SystemExit(f"{step} failed")
    return result.stdout


def main(argv):
    if not argv:
        raise SystemExit(__doc__)
    archive = Path(argv[0]).resolve()
    expected = None
    if "--sha256" in argv:
        expected = argv[argv.index("--sha256") + 1].lower()
    if "--sums" in argv:
        expected = expected_from_sums(argv[argv.index("--sums") + 1], archive.name)
        if expected is None:
            raise SystemExit("no sum for " + archive.name)
    if expected is not None:
        actual = sha256_of(archive)
        if actual != expected:
            raise SystemExit(f"SHA-256 differs: expected {expected}, got {actual}")
        sys.stdout.write("sha256: ok\n")
    work = Path(tempfile.mkdtemp(prefix="anaquel-smoke-"))
    try:
        unpack(archive, work / "unpacked")
        program = find_program(work / "unpacked")
        if program is None:
            raise SystemExit("the archive holds no anaquel program")
        if os.name != "nt" and not os.access(program, os.X_OK):
            raise SystemExit("the program is not executable")
        out = run("version", [str(program), "version"], work)
        sys.stdout.write("  " + out.strip() + "\n")
        library = work / "library"
        run("new", [str(program), "new", str(library), "--title", "Smoke"], work)
        run("check", [str(program), "check", str(library), "--strict"], work)
        run("build", [str(program), "build", str(library), "--no-search"], work)
        if not (library / "dist" / "index.html").is_file():
            raise SystemExit("build wrote no index.html")
        sys.stdout.write("install smoke: ok\n")
    finally:
        shutil.rmtree(work, ignore_errors=True)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
