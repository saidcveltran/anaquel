use std::fs;
use std::path::{Path, PathBuf};

use super::*;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("anaquel-pipeline-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn put(&self, path: &str, text: &str) {
        let full = self.0.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, text).unwrap();
    }

    fn options(&self) -> Options {
        Options {
            root: self.0.clone(),
            dist: self.0.join("dist"),
            search: false,
        }
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.0.join("dist").join(path)).unwrap()
    }

    fn exists(&self, path: &str) -> bool {
        Path::new(&self.0.join("dist").join(path)).exists()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn library(s: &Scratch) {
    s.put(
        "anaquel.toml",
        "schema = 1\nengine = \"0.1\"\ntitle = \"Demo\"\nbase_url = \"https://docs.example\"\n",
    );
    s.put(
        "volumes/guide/volume.toml",
        "title = \"Guide\"\ndescription = \"How to use it.\"\n",
    );
    s.put(
        "volumes/guide/_index.md",
        "# Guide\n\nWelcome. See [setup](setup.md) and [[cache]].\n",
    );
    s.put("volumes/guide/setup.md", "---\ndescription: Set it up.\n---\n# Setup\n\n## Install\n\nRun it. ![logo](img/logo.txt)\n");
    s.put(
        "volumes/guide/terms.md",
        "# Terms\n\n### Cache\n\nStores copies.\n",
    );
    s.put("volumes/guide/img/logo.txt", "logo");
    s.put("volumes/guide/draft.md", "---\ndraft: true\n---\n# Draft\n");
}

#[test]
fn builds_a_small_library() {
    let s = Scratch::new("small");
    library(&s);
    let outcome = build(&s.options());
    assert!(!outcome.report.has_errors(), "{:?}", outcome.report.items());
    assert_eq!(outcome.pages, 3);
    for path in [
        "index.html",
        "404.html",
        "guide/index.html",
        "guide/setup/index.html",
        "guide/terms/index.html",
        "guide/img/logo.txt",
        "_headers",
        "sitemap.xml",
    ] {
        assert!(s.exists(path), "missing {path}");
    }
    assert!(!s.exists("guide/draft/index.html"));
    let setup = s.read("guide/setup/index.html");
    assert!(setup.contains("<h2 id=\"install\">Install</h2>"), "{setup}");
    assert!(setup.contains("src=\"/guide/img/logo.txt\""));
    assert!(setup.contains("<meta name=\"description\" content=\"Set it up.\">"));
    assert!(
        s.read("guide/index.html")
            .contains("href=\"/guide/terms/#cache\"")
    );
    assert!(
        s.read("sitemap.xml")
            .contains("https://docs.example/guide/setup/")
    );
    assert!(!s.read("sitemap.xml").contains("draft"));
}

#[test]
fn a_second_build_writes_nothing_new() {
    let s = Scratch::new("again");
    library(&s);
    let first = build(&s.options());
    assert!(first.written.unwrap().written > 0);
    let second = build(&s.options());
    let stats = second.written.unwrap();
    assert_eq!((stats.written, stats.removed), (0, 0));
}

#[test]
fn errors_stop_the_build_before_anything_is_written() {
    let s = Scratch::new("errors");
    library(&s);
    s.put("volumes/guide/setup.md", "# Setup\n\n[gone](missing.md)\n");
    let outcome = build(&s.options());
    assert!(outcome.report.has_errors());
    assert!(outcome.written.is_none());
    assert!(!s.exists("index.html"));
}

#[test]
fn check_reports_without_writing() {
    let s = Scratch::new("check");
    library(&s);
    let outcome = check(&s.0);
    assert_eq!(outcome.pages, 3);
    assert!(!s.exists("index.html"));
}

#[test]
fn moving_a_page_without_an_alias_is_an_error_and_with_one_is_a_redirect() {
    let s = Scratch::new("moved");
    library(&s);
    assert!(!build(&s.options()).report.has_errors());

    fs::rename(
        s.0.join("volumes/guide/terms.md"),
        s.0.join("volumes/guide/glossary.md"),
    )
    .unwrap();
    s.put("volumes/guide/_index.md", "# Guide\n\n[[cache]]\n");
    let lost = build(&s.options());
    assert!(
        lost.report.items().iter().any(|d| d.code == Code::LOST_URL),
        "{:?}",
        lost.report.items()
    );
    assert!(
        s.exists("guide/terms/index.html"),
        "the old site stays intact"
    );

    s.put(
        "volumes/guide/glossary.md",
        "---\naliases:\n  - /guide/terms/\n---\n# Terms\n\n### Cache\n\nStores copies.\n",
    );
    let moved = build(&s.options());
    assert!(!moved.report.has_errors(), "{:?}", moved.report.items());
    assert!(
        s.read("guide/terms/index.html")
            .contains("url=/guide/glossary/")
    );
    assert!(
        s.read("_redirects")
            .contains("/guide/terms/ /guide/glossary/ 301")
    );
}

#[test]
fn a_library_without_a_manifest_is_one_clear_error() {
    let s = Scratch::new("empty");
    let outcome = check(&s.0);
    assert_eq!(outcome.report.items().len(), 1);
    assert_eq!(outcome.report.items()[0].code, Code::MANIFEST);
}

#[test]
fn crlf_and_bom_sources_build_like_lf() {
    let s = Scratch::new("crlf");
    library(&s);
    s.put(
        "volumes/guide/setup.md",
        "\u{feff}---\r\ndescription: Set it up.\r\n---\r\n# Setup\r\n\r\n## Install\r\n",
    );
    let outcome = build(&s.options());
    assert!(!outcome.report.has_errors(), "{:?}", outcome.report.items());
    assert!(
        s.read("guide/setup/index.html")
            .contains("<h2 id=\"install\">")
    );
}

#[test]
fn the_reference_library_builds_clean() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/reference-library");
    let outcome = check(&root);
    assert_eq!(
        outcome.report.items().len(),
        0,
        "{:?}",
        outcome.report.items()
    );
    assert!(outcome.pages >= 250, "{} pages", outcome.pages);
}
