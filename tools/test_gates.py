"""Tests of `public_gate.py` and `naming_gate.py`.

Run:  python -I tools/test_gates.py
Each test builds a small git repository in a temporary folder and runs the gate on it.
"""

import importlib.util
import pathlib
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
TOOLS = pathlib.Path(__file__).resolve().parent


def load(name):
    spec = importlib.util.spec_from_file_location(name, TOOLS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


public_gate = load("public_gate")
naming_gate = load("naming_gate")

PUBLIC = "saidcveltran@users.noreply.github.com"


def make_repo(files, message="feat: start", author=PUBLIC):
    folder = tempfile.TemporaryDirectory()
    root = pathlib.Path(folder.name)
    for relative, content in files.items():
        target = root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content if isinstance(content, bytes) else content.encode("utf-8"))

    def git(*args):
        subprocess.run(
            ["git", "-C", str(root), "-c", "user.name=Test", "-c", f"user.email={author}", *args],
            check=True,
            capture_output=True,
        )

    git("init", "-q")
    git("add", "-A")
    git("commit", "-q", "-m", message)
    return folder, root


class PublicGate(unittest.TestCase):
    def findings(self, files, **kwargs):
        folder, root = make_repo(files, **{k: v for k, v in kwargs.items() if k in ("message", "author")})
        with folder:
            rest = {k: v for k, v in kwargs.items() if k not in ("message", "author")}
            return public_gate.run(root, **rest)

    def test_a_clean_repository_is_clean(self):
        self.assertEqual(self.findings({"README.md": "Hello.\n", "src/a.rs": "fn a() {}\n"}), [])

    def test_a_user_folder_is_found(self):
        windows = "C:" + "\\Users\\someone\\project"
        unix = "/home/" + "someone/project/"
        for text in (windows, "C:/Users/someone/x", unix, "/Users/" + "someone/x"):
            self.assertNotEqual(self.findings({"a.txt": text}), [], text)

    def test_a_trace_of_an_ai_tool_is_found(self):
        for text in ("Co-Authored-" + "By: x", "Generated " + "with a tool", "https://claude" + ".ai/x"):
            self.assertEqual(len(self.findings({"a.txt": text})), 1, text)

    def test_a_key_is_found(self):
        for text in ("AKIA" + "ABCDEFGHIJKLMNOP", "ghp_" + "a" * 24, "-----BEGIN " + "RSA PRIVATE KEY-----"):
            self.assertEqual(len(self.findings({"a.txt": text})), 1, text)

    def test_an_email_is_found_unless_it_is_a_public_github_identity(self):
        self.assertEqual(len(self.findings({"a.txt": "write to someone" + "@example.org"})), 1)
        self.assertEqual(self.findings({"a.txt": "x " + PUBLIC}), [])
        self.assertEqual(self.findings({"a.txt": "12345+name" + "@users.noreply.github.com"}), [])
        self.assertEqual(self.findings({"a.txt": "noreply" + "@github.com"}), [])

    def test_a_binary_file_with_metadata_is_found(self):
        data = b"\x89PNG\0\0\0Exif\0\0"
        self.assertEqual(len(self.findings({"a.png": data})), 1)
        self.assertEqual(self.findings({"a.png": b"\x89PNG\0\0\0clean"}), [])

    def test_a_commit_message_and_identity_are_read(self):
        self.assertEqual(len(self.findings({"a.txt": "x"}, message="feat: x\n\nCo-Authored-" + "By: y")), 1)
        self.assertEqual(len(self.findings({"a.txt": "x"}, author="someone" + "@example.org")), 2)
        self.assertEqual(self.findings({"a.txt": "x"}, message="m\n\nCo-Authored-" + "By: y", history=False), [])

    def test_private_names_are_found_only_when_given(self):
        files = {"a.txt": "see Secretname here", "b.bin": b"\0Secretname"}
        self.assertEqual(self.findings(files), [])
        with tempfile.TemporaryDirectory() as folder:
            private = pathlib.Path(folder) / "private.txt"
            private.write_text("# comment\nname Secretname\nbinary Secretname\nemail me" + "@example.org\n")
            self.assertEqual(len(self.findings(files, private=private)), 2)
            self.assertEqual(self.findings({"a.txt": "me" + "@example.org"}, private=private), [])

    def test_a_bad_private_file_is_an_error(self):
        with tempfile.TemporaryDirectory() as folder:
            private = pathlib.Path(folder) / "private.txt"
            private.write_text("nonsense\n")
            with self.assertRaises(ValueError):
                public_gate.read_private(private)

    def test_the_gate_files_are_not_searched(self):
        text = "Co-Authored-" + "By: x"
        self.assertEqual(self.findings({"tools/public_gate.py": text}), [])
        self.assertEqual(len(self.findings({"other/public_gate.py": text})), 1)


class NamingGate(unittest.TestCase):
    def findings(self, files):
        folder, root = make_repo(files)
        with folder:
            return naming_gate.check(root)

    def test_names_that_say_nothing_are_refused(self):
        for name in ("utils", "Helpers", "common", "Shared", "misc", "base", "core", "Types", "models",
                     "data", "Info", "stuff", "Manager", "handler", "Service", "processor", "Wrapper",
                     "Context", "item", "Thing", "impl"):
            self.assertIsNotNone(naming_gate.problem(name), name)

    def test_names_that_are_not_just_a_banned_word_pass(self):
        for name in ("RenderContext", "render_context", "Fingerprint", "redirect_rules", "ItemCount",
                     "Catalog", "newest", "oldest", "v", "version", "Parsed", "Heading"):
            self.assertIsNone(naming_gate.problem(name), name)

    def test_age_versions_and_impl_suffixes_are_refused(self):
        for name in ("new_catalog", "old_links", "catalog_v2", "V2", "RenderImpl", "render_impl"):
            self.assertIsNotNone(naming_gate.problem(name), name)

    def test_files_folders_and_items_are_checked(self):
        files = {
            "crates/a/src/utils.rs": "// x\n",
            "crates/a/src/ok.rs": "pub struct Manager;\npub(crate) enum Item {}\ntrait Handler {}\nmod helpers;\n",
            "crates/helpers/src/x.rs": "fn x() {}\n",
        }
        found = self.findings(files)
        self.assertEqual(len(found), 6, found)
        self.assertTrue(any(f.startswith("crates/a/src/utils.rs") for f in found))
        self.assertTrue(any("helpers/" in f for f in found))

    def test_cargo_names_and_skipped_folders_are_allowed(self):
        files = {
            "crates/a/src/lib.rs": "pub struct Catalog;\n",
            "crates/a/src/main.rs": "fn main() {}\n",
            "tools/utils.py": "x = 1\n",
            "examples/library/common/page.md": "# Page\n",
            "themes/folio/assets/shared/x.css": "p {}\n",
            "fuzz/corpus/data/seed": "x",
        }
        self.assertEqual(self.findings(files), [])

    def test_items_inside_comments_and_strings_of_other_languages_are_not_read(self):
        files = {"crates/a/src/ok.rs": "fn x() {\n    let s = \"struct Manager\";\n}\n", "a.md": "struct Manager\n"}
        self.assertEqual(self.findings(files), [])


if __name__ == "__main__":
    unittest.main(verbosity=1)
