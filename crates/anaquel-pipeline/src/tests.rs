use std::fs;
use std::path::{Path, PathBuf};

use super::*;

pub(crate) struct Scratch(pub(crate) PathBuf);

impl Scratch {
    pub(crate) fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("anaquel-pipeline-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub(crate) fn put(&self, path: &str, text: &str) {
        let full = self.0.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, text).unwrap();
    }

    pub(crate) fn options(&self) -> Options {
        Options {
            root: self.0.clone(),
            dist: self.0.join("dist"),
            search: false,
        }
    }

    pub(crate) fn read(&self, path: &str) -> String {
        fs::read_to_string(self.0.join("dist").join(path)).unwrap()
    }

    pub(crate) fn exists(&self, path: &str) -> bool {
        Path::new(&self.0.join("dist").join(path)).exists()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn library(s: &Scratch) {
    s.put(
        "anaquel.toml",
        "schema = 1\nengine = \"0.2\"\ntitle = \"Demo\"\nbase_url = \"https://docs.example\"\n",
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

#[test]
fn the_stylesheet_names_the_fingerprinted_font() {
    let s = Scratch::new("font");
    library(&s);
    assert!(!build(&s.options()).report.has_errors());
    let styles = s.0.join("dist/assets/styles");
    let css_path = fs::read_dir(&styles)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let css = fs::read_to_string(css_path).unwrap();
    assert!(
        !css.contains("asset:"),
        "placeholder left in the stylesheet"
    );
    let url = css
        .split("url(\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap();
    assert!(url.starts_with("/assets/fonts/literata-latin."), "{url}");
    assert!(s.exists(url.trim_start_matches('/')));
}

/// Copies `examples/NAME` into `scratch`, leaving out any `dist` from an earlier build.
fn copy_example(name: &str, scratch: &Scratch) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name);
    let walk = walkdir::WalkDir::new(&root)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| e.file_name() != "dist");
    for item in walk {
        let item = item.unwrap();
        if item.file_type().is_file() {
            let relative = item.path().strip_prefix(&root).unwrap();
            let text = fs::read_to_string(item.path()).unwrap();
            scratch.put(
                &relative
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/"),
                &text,
            );
        }
    }
}

#[test]
fn the_bilingual_example_builds_clean_and_links_the_languages() {
    let s = Scratch::new("bilingual");
    copy_example("bilingual", &s);
    let outcome = build(&s.options());
    assert_eq!(
        outcome.report.items().len(),
        0,
        "{:?}",
        outcome.report.items()
    );

    let english = s.read("guide/setup/index.html");
    assert!(english.contains("<html lang=\"en\">"), "{english}");
    assert!(
        english.contains(
            "rel=\"alternate\" hreflang=\"en\" href=\"https://docs.example/guide/setup/\""
        )
    );
    assert!(
        english.contains(
            "rel=\"alternate\" hreflang=\"es\" href=\"https://docs.example/guia/setup/\""
        )
    );
    assert!(english.contains("Skip to content"), "{english}");

    let spanish = s.read("guia/setup/index.html");
    assert!(spanish.contains("<html lang=\"es\">"), "{spanish}");
    assert!(spanish.contains("Saltar al contenido"), "{spanish}");
    assert!(!spanish.contains("Skip to content"), "{spanish}");
    assert!(
        spanish.contains("hreflang=\"en\" lang=\"en\""),
        "the switcher lists English"
    );
    assert!(
        spanish.contains("aria-current=\"true\""),
        "the switcher marks the current page"
    );

    let lone = s.read("guide/english-only/index.html");
    assert!(lone.contains("<html lang=\"en\">"));
    assert!(
        !lone.contains("hreflang"),
        "a page with no translation lists no alternates: {lone}"
    );
    assert!(!lone.contains("class=\"languages\""), "{lone}");
}

#[test]
fn the_multi_shelf_example_builds_clean_and_groups_the_home_page() {
    let s = Scratch::new("shelves");
    copy_example("multi-shelf", &s);
    let outcome = build(&s.options());
    assert_eq!(
        outcome.report.items().len(),
        0,
        "{:?}",
        outcome.report.items()
    );
    assert_eq!(outcome.pages, 5);
    for path in [
        "about/index.html",
        "guides/start/index.html",
        "guides/start/first-steps/index.html",
        "guides/advanced/index.html",
        "reference/start/index.html",
    ] {
        assert!(s.exists(path), "missing {path}");
    }
    let home = s.read("index.html");
    let guides = home.find("<h2>Guides</h2>").unwrap();
    let reference = home.find("<h2>Reference</h2>").unwrap();
    assert!(home.find("href=\"/about/\"").unwrap() < guides);
    assert!(guides < home.find("href=\"/guides/start/\"").unwrap());
    assert!(home.find("href=\"/guides/advanced/\"").unwrap() < reference);
    assert!(reference < home.find("href=\"/reference/start/\"").unwrap());

    let llms = s.read("llms.txt");
    assert!(llms.starts_with("# "), "{llms}");
    let first = llms.find("](/about/)").expect("loose volume listed");
    let step = llms
        .find("](/guides/start/first-steps/)")
        .expect("page of a shelved volume listed");
    let last = llms
        .find("](/reference/start/)")
        .expect("second shelf listed");
    assert!(first < step && step < last, "{llms}");
    assert!(llms.contains("\n## Guides: "), "{llms}");
}

#[test]
fn a_library_may_have_shelves_and_no_loose_volumes() {
    let s = Scratch::new("shelves-only");
    s.put(
        "anaquel.toml",
        "schema = 1\nengine = \"0.2\"\ntitle = \"Demo\"\n",
    );
    s.put("shelves/a/shelf.toml", "title = \"A\"\n");
    s.put("shelves/a/volumes/v/volume.toml", "title = \"V\"\n");
    s.put("shelves/a/volumes/v/_index.md", "# V\n");
    let outcome = build(&s.options());
    assert!(!outcome.report.has_errors(), "{:?}", outcome.report.items());
    assert!(s.exists("a/v/index.html"));
}

#[test]
fn a_shelf_without_its_manifest_is_an_error() {
    let s = Scratch::new("shelf-no-manifest");
    s.put(
        "anaquel.toml",
        "schema = 1\nengine = \"0.2\"\ntitle = \"Demo\"\n",
    );
    s.put("shelves/a/volumes/v/volume.toml", "title = \"V\"\n");
    s.put("shelves/a/volumes/v/_index.md", "# V\n");
    let outcome = check(&s.0);
    assert!(outcome.report.has_errors());
    assert!(
        outcome
            .report
            .items()
            .iter()
            .any(|d| d.message.contains("shelf.toml")),
        "{:?}",
        outcome.report.items()
    );
}

#[test]
fn the_not_found_page_and_the_home_page_use_the_library_language() {
    let s = Scratch::new("spanish-library");
    s.put(
        "anaquel.toml",
        "schema = 1\nengine = \"0.2\"\ntitle = \"Demo\"\nlanguage = \"es\"\n",
    );
    s.put("volumes/guia/volume.toml", "title = \"Guía\"\n");
    s.put("volumes/guia/_index.md", "# Guía\n");
    let outcome = build(&s.options());
    assert_eq!(
        outcome.report.items().len(),
        0,
        "{:?}",
        outcome.report.items()
    );
    let missing = s.read("404.html");
    assert!(missing.contains("<html lang=\"es\">"), "{missing}");
    assert!(missing.contains("Página no encontrada"), "{missing}");
    assert!(!missing.contains("Page not found"), "{missing}");
    assert!(s.read("index.html").contains("<html lang=\"es\">"));
}

#[test]
fn a_translation_mistake_stops_the_build_with_a_located_error() {
    let s = Scratch::new("bad-translation");
    s.put(
        "anaquel.toml",
        "schema = 1\nengine = \"0.2\"\ntitle = \"Demo\"\n",
    );
    s.put(
        "volumes/guia/volume.toml",
        "title = \"Guía\"\nlanguage = \"es\"\ntranslation_of = \"nada\"\n",
    );
    s.put("volumes/guia/_index.md", "# Guía\n");
    let outcome = build(&s.options());
    let errors: Vec<_> = outcome
        .report
        .items()
        .iter()
        .filter(|d| d.code == Code::MANIFEST)
        .collect();
    assert_eq!(errors.len(), 1, "{:?}", outcome.report.items());
    assert_eq!(errors[0].path.as_deref(), Some("volumes/guia/volume.toml"));
    assert!(
        outcome.written.is_none(),
        "nothing is written after an error"
    );
}
