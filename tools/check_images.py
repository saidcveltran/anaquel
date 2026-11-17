"""Checks that the images of the repository carry no metadata.

Usage:  python -I tools/check_images.py [--fix] [folder ...]

PNG files may hold only the chunks needed to draw them; anything else (text, time, EXIF,
colour profile, physical size) is metadata. SVG files must not hold editor data, metadata
elements or comments. JPEG and WebP files are not used by the repository and are refused.
With --fix, a PNG is rewritten without its extra chunks. Standard library only.
"""

import struct
import sys
import zlib
from pathlib import Path

KEEP = {b"IHDR", b"PLTE", b"IDAT", b"IEND", b"tRNS"}
SIGNATURE = b"\x89PNG\r\n\x1a\n"
SVG_MARKERS = (b"<metadata", b"inkscape", b"sodipodi", b"<!--", b"<rdf:", b"xmlns:dc", b"<?xml-stylesheet")
SKIP_PARTS = {".git", "target", "node_modules", "dist", ".anaquel"}


def png_chunks(data):
    """Yields (type, body) for each chunk, or raises ValueError."""
    if not data.startswith(SIGNATURE):
        raise ValueError("not a PNG file")
    at = len(SIGNATURE)
    while at < len(data):
        if at + 12 > len(data):
            raise ValueError("truncated chunk")
        (size,) = struct.unpack(">I", data[at : at + 4])
        kind = data[at + 4 : at + 8]
        body = data[at + 8 : at + 8 + size]
        crc = data[at + 8 + size : at + 12 + size]
        if len(body) != size or struct.unpack(">I", crc)[0] != zlib.crc32(kind + body):
            raise ValueError("bad chunk " + kind.decode("latin-1"))
        yield kind, body
        at += 12 + size


def check_png(path, fix):
    data = path.read_bytes()
    try:
        chunks = list(png_chunks(data))
    except ValueError as err:
        return [f"{path}: {err}"]
    extra = sorted({kind.decode("latin-1") for kind, _ in chunks if kind not in KEEP})
    if not extra:
        return []
    if not fix:
        return [f"{path}: metadata chunks {', '.join(extra)}"]
    out = bytearray(SIGNATURE)
    for kind, body in chunks:
        if kind in KEEP:
            out += struct.pack(">I", len(body)) + kind + body
            out += struct.pack(">I", zlib.crc32(kind + body))
    path.write_bytes(bytes(out))
    return []


def check_svg(path):
    data = path.read_bytes().lower()
    found = [m.decode() for m in SVG_MARKERS if m in data]
    return [f"{path}: editor data or metadata ({', '.join(found)})"] if found else []


def main(argv):
    fix = "--fix" in argv
    roots = [Path(a) for a in argv if a != "--fix"] or [Path(".")]
    problems = []
    seen = 0
    for root in roots:
        for path in sorted(root.rglob("*")):
            if not path.is_file() or SKIP_PARTS & set(path.parts):
                continue
            suffix = path.suffix.lower()
            if suffix == ".png":
                problems += check_png(path, fix)
            elif suffix == ".svg":
                problems += check_svg(path)
            elif suffix in {".jpg", ".jpeg", ".webp", ".gif", ".tif", ".tiff", ".heic"}:
                problems.append(f"{path}: format not used by the repository")
            else:
                continue
            seen += 1
    for line in problems:
        sys.stderr.write(line + "\n")
    sys.stdout.write(f"images: {seen} checked, {len(problems)} problems\n")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
