"""Refuses names that say nothing about what a thing is responsible for.

Usage:  python -I tools/naming_gate.py [REPO]

Exit status 0 means clean, 1 means findings, 2 means the gate could not run.

A crate, a folder, a Rust file, a module, a type or a trait may not be called `utils`, `helpers`,
`common`, `shared`, `misc`, `base`, `core`, `types`, `models`, `data`, `info`, `stuff`, `manager`,
`handler`, `service`, `processor`, `wrapper`, `context`, `item` or `thing`; may not end in `impl`;
and may not start with `new_` or `old_` or be or end with a version such as `v2`. A name earns its
place by answering "what is its one responsibility?" with a noun of the domain (`fingerprint`,
`redirect_rules`), so a qualified name such as `RenderContext` is fine.

Not checked: `tools/` (a folder of scripts, by convention), `target/` and `vendor/`, the sample
libraries in `examples/` and the content folders of themes, whose names belong to their authors,
and `lib.rs` and `main.rs`, which Cargo names.
"""

import pathlib
import re
import subprocess
import sys

BANNED = {
    "utils", "util", "helpers", "helper", "common", "shared", "misc", "base", "core", "types",
    "models", "data", "info", "stuff", "manager", "handler", "service", "processor", "wrapper",
    "context", "item", "thing", "impl", "lib",
}
SKIPPED_FOLDERS = ("tools/", "target/", "vendor/", "examples/", "themes/", ".git/")
CARGO_NAMED = {"lib.rs", "main.rs"}
ITEM = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|trait|type|mod|union)\s+([A-Za-z_][A-Za-z0-9_]*)")
VERSIONED = re.compile(r"(^|_)v[0-9]+$")


def words(name):
    """Splits `RenderContext`, `render_context` and `render-context` into lower-case words."""
    spaced = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", name)
    return [w for w in re.split(r"[_\-]+", spaced.lower()) if w]


def problem(name, mod_in_file=False):
    """Returns why `name` is refused, or None."""
    parts = words(name)
    joined = "_".join(parts)
    if not parts:
        return None
    if len(parts) == 1 and parts[0] in BANNED:
        if parts[0] == "lib" and mod_in_file:
            return None
        return f"`{name}` says nothing about its responsibility"
    if parts[-1] == "impl":
        return f"`{name}` ends in `impl`"
    if parts[0] in {"new", "old"} and len(parts) > 1:
        return f"`{name}` starts with `{parts[0]}_`: name what it is, not its age"
    if VERSIONED.search(joined):
        return f"`{name}` carries a version: name what differs"
    return None


def tracked(repo):
    done = subprocess.run(
        ["git", "-C", str(repo), "ls-files", "--cached", "--others", "--exclude-standard"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if done.returncode != 0:
        raise RuntimeError(done.stderr.strip() or "git ls-files failed")
    return [line for line in done.stdout.split("\n") if line]


def check(repo):
    findings = []
    folders = set()
    for relative in tracked(repo):
        if relative.startswith(SKIPPED_FOLDERS) or relative.startswith("fuzz/corpus/"):
            continue
        path = pathlib.PurePosixPath(relative)
        folders.update(str(parent) for parent in path.parents if str(parent) != ".")
        if path.suffix == ".rs":
            if path.name not in CARGO_NAMED:
                reason = problem(path.stem)
                if reason:
                    findings.append(f"{relative}: file {reason}")
            source = (pathlib.Path(repo) / relative).read_text(encoding="utf-8", errors="replace")
            for number, line in enumerate(source.splitlines(), 1):
                found = ITEM.match(line)
                reason = found and problem(found.group(1))
                if reason:
                    findings.append(f"{relative}:{number}: {reason}")
    for folder in sorted(folders):
        if folder.startswith(SKIPPED_FOLDERS) or folder.startswith("fuzz/corpus"):
            continue
        reason = problem(pathlib.PurePosixPath(folder).name)
        if reason:
            findings.append(f"{folder}/: folder {reason}")
    return findings


def main(argv):
    repo = pathlib.Path(argv[0] if argv else ".").resolve()
    try:
        findings = check(repo)
    except (RuntimeError, OSError) as error:
        sys.stdout.write(f"naming gate could not run: {error}\n")
        return 2
    if findings:
        sys.stdout.write("\n".join(findings) + f"\n{len(findings)} findings\n")
        return 1
    sys.stdout.write("clean\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
