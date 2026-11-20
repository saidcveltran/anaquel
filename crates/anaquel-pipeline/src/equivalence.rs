//! Equivalence tests of the analysis phases (ingest, catalog, compose).
//!
//! The analysis must give the same catalog, the same diagnostics in the same order and the same
//! site no matter how many threads run it. These tests fix that contract before any phase is made
//! parallel: they build a generated library full of faults, render everything the analysis decides
//! into plain text and compare it with a recorded reference, once per thread count.
//!
//! The reference lives in `equivalence/hostile_library.golden`. After a deliberate change of the
//! analysis, record it again with `ANAQUEL_BLESS_GOLDEN=1 cargo test -p anaquel-pipeline equivalence`
//! and review the diff of the file: it is the whole observable behaviour of the analysis.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anaquel_diagnostics::Diagnostic;

use super::*;
use crate::tests::Scratch;

/// Thread counts every scenario runs under; `1` is the serial order the others must reproduce.
const THREAD_COUNTS: [usize; 4] = [1, 2, 3, 8];

/// Pages per generated volume: enough for several blocks of work per worker at the largest count.
const PAGES_PER_VOLUME: usize = 300;

/// Pages of one generated volume live in this many sections.
const SECTIONS: usize = 9;

const GOLDEN: &str = include_str!("equivalence/hostile_library.golden");
const GOLDEN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/equivalence/hostile_library.golden"
);

/// Terms every generated volume defines, so that `[[term]]` links have something to resolve.
const TERMS: [&str; 5] = ["alpha", "bravo", "charlie", "delta", "echo"];

/// Runs `work` on a rayon pool of exactly `threads` threads.
fn within_pool<T: Send>(threads: usize, work: impl FnOnce() -> T + Send) -> T {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("a rayon pool can be built")
        .install(work)
}

/// One front-matter fault per rule, chosen by the page number so the faults spread over the volume.
fn page_text(volume: &str, number: usize, hostile: bool) -> Vec<u8> {
    let section = number % SECTIONS;
    let mut front = String::new();
    let mut body = format!(
        "# Page {number} of {volume}\n\nIntro to [[{}]].\n\n## Part {number}\n\n",
        TERMS[number % TERMS.len()]
    );
    if number + SECTIONS < PAGES_PER_VOLUME {
        let _ = writeln!(body, "[next](page-{:04}.md)\n", number + SECTIONS);
    }
    let _ = writeln!(body, "[up](_index.md) and [terms](../terms.md).\n");
    if hostile {
        if number % 17 == 3 {
            let _ = writeln!(body, "[gone](nowhere-{number}.md)\n");
        }
        if number % 41 == 9 {
            body.push_str("[[undefined-term]]\n\n");
        }
        if number % 19 == 5 {
            front.push_str("draft: true\n");
        }
        if number % 29 == 11 {
            let _ = writeln!(front, "slug: shared-{section}");
        }
        if number % 31 == 13 {
            let _ = writeln!(front, "aliases:\n  - /old/{}/", number % 4);
        }
        if number % 37 == 2 {
            front.push_str("colour: red\n");
        }
        if number % 43 == 4 {
            let mut bytes = b"# Not text\n\n".to_vec();
            bytes.extend_from_slice(&[0xff, 0xfe, 0xfd]);
            return bytes;
        }
        if number % 47 == 6 {
            return format!(
                "\u{feff}---\r\ntitle: Crlf {number}\r\n---\r\n# Crlf\r\n\r\n[[alpha]]\r\n"
            )
            .into_bytes();
        }
        if number % 53 == 8 {
            return format!("---\ntitle: [unclosed\n---\n# Broken {number}\n").into_bytes();
        }
    }
    if front.is_empty() {
        body.into_bytes()
    } else {
        format!("---\n{front}---\n{body}").into_bytes()
    }
}

/// Writes one generated volume: a section index per section, a terms page and the numbered pages.
fn generated_volume(s: &Scratch, slug: &str, weight: i64, hostile: bool) {
    let base = format!("volumes/{slug}");
    s.put(
        &format!("{base}/volume.toml"),
        &format!("title = \"Volume {slug}\"\ndescription = \"Generated.\"\nweight = {weight}\n"),
    );
    s.put(
        &format!("{base}/_index.md"),
        &format!("# Volume {slug}\n\n[[alpha]]\n"),
    );
    let mut terms = String::from("# Terms\n\n");
    for term in TERMS {
        let _ = writeln!(terms, "### {term}\n\nThe term {term}.\n");
    }
    s.put(&format!("{base}/terms.md"), &terms);
    for section in 0..SECTIONS {
        s.put(
            &format!("{base}/section-{section}/_index.md"),
            &format!("# Section {section}\n\n[terms](../terms.md)\n"),
        );
    }
    for number in 0..PAGES_PER_VOLUME {
        let path =
            s.0.join(&base)
                .join(format!("section-{}/page-{number:04}.md", number % SECTIONS));
        fs::write(path, page_text(slug, number, hostile)).expect("a generated page is written");
    }
}

/// A library with no fault at all: it builds, so the whole output tree can be compared.
fn clean_library(s: &Scratch) {
    s.put(
        "anaquel.toml",
        "schema = 1\nengine = \"0.2\"\ntitle = \"Clean\"\nbase_url = \"https://docs.example\"\n",
    );
    generated_volume(s, "north", 1, false);
    generated_volume(s, "south", 2, false);
}

/// A library that trips every kind of finding the analysis can make, across several blocks of work.
fn hostile_library(s: &Scratch) {
    s.put(
        "anaquel.toml",
        "schema = 1\nengine = \"0.2\"\ntitle = \"Hostile\"\nbase_url = \"https://docs.example\"\n",
    );
    generated_volume(s, "north", 1, true);
    generated_volume(s, "south", 2, true);

    // Faults of the file names and of the folders around them.
    s.put("volumes/south/section-0/has#hash.md", "# Hash\n");
    s.put("volumes/south/section-0/100%.md", "# Percent\n");
    s.put("volumes/south/section-0/sp ace.md", "# Space\n");
    s.put("volumes/south/section-0/dollar$.md", "# Dollar\n");
    s.put("volumes/south/section-0/trailing-dot..md", "# Dot\n");
    s.put("volumes/south/section-1/UPPER.md", "# Upper\n");
    s.put(
        "volumes/south/orphan-folder/lonely.md",
        "# No index above\n",
    );
    s.put("volumes/south/orphan-folder/nested/deeper.md", "# Deeper\n");
    s.put("volumes/south/assets/data.txt", "plain file\n");

    // A page over the size limit is refused before it is read.
    let oversize = vec![b'x'; usize::try_from(MAX_PAGE_BYTES).expect("the limit fits") + 1];
    fs::write(s.0.join("volumes/south/section-2/huge.md"), oversize).expect("huge page written");

    // Volumes that fail on their own: a bad folder name, an unreadable manifest, no home page.
    s.put("volumes/Bad Name/volume.toml", "title = \"Bad\"\n");
    s.put("volumes/Bad Name/_index.md", "# Bad\n");
    s.put("volumes/broken/volume.toml", "title = [\n");
    s.put("volumes/broken/_index.md", "# Broken\n");
    s.put("volumes/headless/volume.toml", "title = \"Headless\"\n");
    s.put("volumes/headless/only.md", "# Only\n\n[[alpha]]\n");
}

fn diagnostic_line(d: &Diagnostic, root: &str) -> String {
    format!(
        "{:?}|{}|{}|{:?}|{}|{:?}",
        d.severity,
        d.code,
        d.path.as_deref().unwrap_or("-"),
        d.line,
        d.message,
        d.help
    )
    .replace(root, "<root>")
    .replace('\\', "/")
}

/// Everything the analysis decides, as text: the report in the order the phases produced it, the
/// catalog entry by entry, and the report `check` prints.
fn analysis_text(root: &Path) -> String {
    let root_text = root.to_string_lossy().into_owned();
    let mut out = String::new();

    let mut report = Report::new();
    let analysis = analyse(root, &mut report);
    out.push_str("## report as produced\n");
    for d in report.items() {
        let _ = writeln!(out, "{}", diagnostic_line(d, &root_text));
    }
    out.push_str("## catalog\n");
    if let Some(analysis) = &analysis {
        for entry in analysis.catalog.entries() {
            let _ = writeln!(out, "{entry:?}");
        }
        let _ = writeln!(out, "published={}", analysis.composed.len());
        for id in analysis.composed.keys() {
            let _ = writeln!(out, "composed {}", id.0);
        }
    } else {
        out.push_str("no analysis\n");
    }
    out.push_str("## check\n");
    let outcome = check(root);
    let _ = writeln!(out, "pages={}", outcome.pages);
    for d in outcome.report.items() {
        let _ = writeln!(out, "{}", diagnostic_line(d, &root_text));
    }
    out.replace("\r\n", "\n")
}

/// Everything compose produced, page by page; too long for the reference, so it is only compared
/// between runs of one process.
fn composed_text(root: &Path) -> String {
    let mut report = Report::new();
    let analysis = analyse(root, &mut report).expect("the library is readable");
    let mut out = String::new();
    for (id, page) in &analysis.composed {
        let _ = writeln!(out, "{} {page:?}", id.0);
    }
    out
}

/// Every file of `dir` with its bytes, keyed by path with `/`.
fn tree_of(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for item in fs::read_dir(&folder).expect("the folder is readable") {
            let path = item.expect("the entry is readable").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let key = path
                    .strip_prefix(dir)
                    .expect("inside the tree")
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(key, fs::read(&path).expect("the file is readable"));
            }
        }
    }
    files
}

#[test]
fn the_hostile_library_is_analysed_as_recorded_with_any_thread_count() {
    let s = Scratch::new("equivalence-golden");
    hostile_library(&s);
    if std::env::var_os("ANAQUEL_BLESS_GOLDEN").is_some() {
        // The reference is compiled in, so a recorded one is compared by the next run.
        let text = within_pool(1, || analysis_text(&s.0));
        fs::write(GOLDEN_PATH, text).expect("the reference is written");
        return;
    }
    let recorded = GOLDEN.replace("\r\n", "\n");
    for threads in THREAD_COUNTS {
        let text = within_pool(threads, || analysis_text(&s.0));
        assert!(
            text == recorded,
            "the analysis under {threads} thread(s) differs from the recorded reference at line {}",
            first_difference(&recorded, &text)
        );
    }
}

/// The 1-based number of the first line where two texts differ.
fn first_difference(expected: &str, actual: &str) -> usize {
    expected
        .lines()
        .zip(actual.lines())
        .position(|(a, b)| a != b)
        .map_or_else(
            || expected.lines().count().min(actual.lines().count()) + 1,
            |i| i + 1,
        )
}

#[test]
fn the_composed_pages_do_not_depend_on_the_thread_count() {
    let s = Scratch::new("equivalence-composed");
    hostile_library(&s);
    let serial = within_pool(1, || composed_text(&s.0));
    assert_ne!(serial, "", "the hostile library composes some pages");
    for threads in THREAD_COUNTS {
        let text = within_pool(threads, || composed_text(&s.0));
        assert!(
            text == serial,
            "compose under {threads} thread(s) differs from the serial run at line {}",
            first_difference(&serial, &text)
        );
    }
}

#[test]
fn the_built_site_is_the_same_file_by_file_with_any_thread_count() {
    let s = Scratch::new("equivalence-tree");
    clean_library(&s);
    let mut trees = Vec::new();
    for threads in THREAD_COUNTS {
        let options = Options {
            root: s.0.clone(),
            dist: s.0.join(format!("dist-{threads}")),
            search: false,
        };
        let outcome = within_pool(threads, || build(&options));
        assert!(
            !outcome.report.has_errors(),
            "{threads} thread(s): {:?}",
            outcome.report.items()
        );
        assert_eq!(outcome.pages, 2 * (PAGES_PER_VOLUME + SECTIONS + 2));
        trees.push((
            threads,
            outcome.report.items().to_vec(),
            tree_of(&options.dist),
        ));
    }
    let (_, serial_report, serial_tree) = &trees[0];
    assert!(serial_tree.len() > 2 * PAGES_PER_VOLUME);
    for (threads, report, tree) in &trees[1..] {
        assert_eq!(report, serial_report, "report under {threads} thread(s)");
        let missing: Vec<_> = serial_tree
            .keys()
            .filter(|k| !tree.contains_key(*k))
            .collect();
        let extra: Vec<_> = tree
            .keys()
            .filter(|k| !serial_tree.contains_key(*k))
            .collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "{threads} thread(s): missing {missing:?}, extra {extra:?}"
        );
        for (path, bytes) in serial_tree {
            assert!(
                &tree[path] == bytes,
                "{threads} thread(s): `{path}` differs from the serial site"
            );
        }
    }
}
