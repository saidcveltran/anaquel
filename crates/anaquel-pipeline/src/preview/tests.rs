use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anaquel_diagnostics::{Code, Diagnostic, Severity};

use super::{preview_page, theme_assets};
use crate::tests::Scratch;
use crate::{build, check};

const PAGE: &str = "volumes/guide/setup.md";
const SETUP: &str = "---\ndescription: Set it up.\n---\n# Setup\n\n## Install\n\nBack to the [guide](_index.md) and on to [terms](terms.md#cache).\n\n```sh\nanaquel build\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";

fn library(name: &str) -> Scratch {
    let s = Scratch::new(name);
    s.put(
        "anaquel.toml",
        "schema = 1\nengine = \"0.2\"\ntitle = \"Demo\"\n",
    );
    s.put("volumes/guide/volume.toml", "title = \"Guide\"\n");
    s.put(
        "volumes/guide/_index.md",
        "# Guide\n\nStart with [setup](setup.md).\n",
    );
    s.put(PAGE, "# Old setup\n");
    s.put(
        "volumes/guide/terms.md",
        "# Terms\n\n### Cache\n\nStores copies.\n",
    );
    s
}

fn errors(diagnostics: &[Diagnostic]) -> Vec<Code> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code)
        .collect()
}

fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut found = BTreeMap::new();
    for item in walkdir::WalkDir::new(root).min_depth(1) {
        let item = item.unwrap();
        if item.file_type().is_file() {
            let name = item
                .path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            found.insert(name, fs::read(item.path()).unwrap());
        }
    }
    found
}

#[test]
fn a_preview_is_the_page_the_build_would_write_for_the_same_text() {
    // One library holds the old text on disk and previews the new one; the other has the new
    // text on disk and is built. The two pages must be the same bytes.
    let previewed = library("preview-parity-a");
    let preview = preview_page(&previewed.0, PAGE, SETUP);
    assert!(preview.page.is_empty(), "{:?}", preview.page);
    assert_eq!(preview.url.as_deref(), Some("/guide/setup/"));

    let built = library("preview-parity-b");
    built.put(PAGE, SETUP);
    let outcome = build(&built.options());
    assert!(!outcome.report.has_errors(), "{:?}", outcome.report.items());
    assert_eq!(preview.html.unwrap(), built.read("guide/setup/index.html"));
}

#[test]
fn the_largest_example_previews_without_a_diagnostic() {
    // 271 pages; the preview must give the same page the example builds, with no diagnostic.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/reference-library");
    let page = "volumes/clockwork/foundations/careful-ledger-3.md";
    let text = fs::read_to_string(root.join(page)).unwrap();
    let preview = preview_page(&root, page, &text);
    assert!(preview.page.is_empty(), "{:?}", preview.page);
    assert!(preview.elsewhere.is_empty(), "{:?}", preview.elsewhere);
    assert!(preview.html.is_some());
}

#[test]
fn a_preview_writes_nothing() {
    let s = library("preview-writes-nothing");
    let before = files(&s.0);
    let preview = preview_page(&s.0, PAGE, SETUP);
    assert!(preview.html.is_some());
    assert_eq!(files(&s.0), before);
    assert!(!s.0.join("dist").exists());
}

#[test]
fn the_text_of_the_editor_replaces_the_text_on_disk() {
    let s = library("preview-replaces");
    let html = preview_page(&s.0, PAGE, "# Brand new title\n")
        .html
        .unwrap();
    assert!(html.contains("Brand new title"));
    assert!(!html.contains("Old setup"));
}

#[test]
fn a_byte_order_mark_and_crlf_make_no_difference() {
    let s = library("preview-crlf");
    let plain = preview_page(&s.0, PAGE, SETUP).html.unwrap();
    let windows = format!("\u{feff}{}", SETUP.replace('\n', "\r\n"));
    assert_eq!(preview_page(&s.0, PAGE, &windows).html.unwrap(), plain);
}

#[test]
fn a_link_to_a_page_that_does_not_exist_is_a_diagnostic_of_the_page_and_gives_no_html() {
    let s = library("preview-broken-link");
    let preview = preview_page(&s.0, PAGE, "# Setup\n\nSee [nothing](missing.md).\n");
    assert_eq!(errors(&preview.page), [Code::BROKEN_LINK]);
    assert!(preview.page.iter().all(|d| d.path.as_deref() == Some(PAGE)));
    assert!(preview.html.is_none(), "an error must not render a page");
    // The page still has an address, so the editor can say which page it is.
    assert_eq!(preview.url.as_deref(), Some("/guide/setup/"));
    assert!(preview.elsewhere.is_empty(), "{:?}", preview.elsewhere);
}

#[test]
fn broken_front_matter_gives_a_diagnostic_and_no_html() {
    let s = library("preview-front-matter");
    for text in [
        "---\ntitle: never closed\n# Setup\n",
        "---\nflavour: sweet\n---\n# Setup\n",
    ] {
        let preview = preview_page(&s.0, PAGE, text);
        assert!(!errors(&preview.page).is_empty(), "{text:?}");
        assert!(preview.html.is_none(), "{text:?}");
    }
}

#[test]
fn the_check_and_the_preview_agree_on_the_diagnostics_of_the_page() {
    let s = library("preview-agrees-with-check");
    let text = "# Setup\n\nSee [nothing](missing.md) and [bad](terms.md#no-such-heading).\n";
    let preview = preview_page(&s.0, PAGE, text);
    s.put(PAGE, text);
    let outcome = check(&s.0);
    let mut expected: Vec<(Code, String)> = outcome
        .report
        .items()
        .iter()
        .filter(|d| d.path.as_deref() == Some(PAGE))
        .map(|d| (d.code, d.message.clone()))
        .collect();
    let mut shown: Vec<(Code, String)> = preview
        .page
        .iter()
        .map(|d| (d.code, d.message.clone()))
        .collect();
    expected.sort();
    shown.sort();
    assert_eq!(shown, expected);
    assert!(shown.len() >= 2, "{shown:?}");
}

#[test]
fn a_problem_in_another_page_does_not_stop_the_preview_and_is_kept_apart() {
    let s = library("preview-elsewhere");
    s.put(
        "volumes/guide/terms.md",
        "---\ntitle: never closed\n# Terms\n",
    );
    let preview = preview_page(&s.0, PAGE, "# Setup\n");
    assert!(preview.page.is_empty(), "{:?}", preview.page);
    assert!(preview.html.is_some());
    assert_eq!(errors(&preview.elsewhere), [Code::FRONT_MATTER]);
    assert_eq!(
        preview.elsewhere[0].path.as_deref(),
        Some("volumes/guide/terms.md")
    );
}

#[test]
fn a_draft_is_rendered_for_the_editor() {
    let s = library("preview-draft");
    let preview = preview_page(&s.0, PAGE, "---\ndraft: true\n---\n# Later\n");
    assert!(preview.draft);
    assert!(preview.html.unwrap().contains("Later"));
    let published = preview_page(&s.0, PAGE, "# Now\n");
    assert!(!published.draft);
}

#[test]
fn a_page_that_is_not_on_disk_yet_is_previewed_in_its_volume() {
    let s = library("preview-new-page");
    let page = "volumes/guide/fresh.md";
    let preview = preview_page(&s.0, page, "# Fresh\n\nBack to [setup](setup.md).\n");
    assert!(preview.page.is_empty(), "{:?}", preview.page);
    assert_eq!(preview.url.as_deref(), Some("/guide/fresh/"));
    assert!(preview.html.unwrap().contains("Fresh"));
    assert!(!s.0.join(page).exists());
}

#[test]
fn a_page_in_a_shelf_volume_is_previewed() {
    let s = Scratch::new("preview-shelf");
    s.put(
        "anaquel.toml",
        "schema = 1\nengine = \"0.2\"\ntitle = \"Demo\"\n",
    );
    s.put("shelves/team/shelf.toml", "title = \"Team\"\n");
    s.put("shelves/team/volumes/ops/volume.toml", "title = \"Ops\"\n");
    s.put("shelves/team/volumes/ops/runbook.md", "# Runbook\n");
    let page = "shelves/team/volumes/ops/runbook.md";
    let preview = preview_page(&s.0, page, "# Runbook, edited\n");
    assert!(preview.page.is_empty(), "{:?}", preview.page);
    assert!(preview.html.unwrap().contains("edited"));
}

#[test]
fn a_path_that_is_not_a_page_of_a_volume_is_refused_with_a_reason() {
    let s = library("preview-refused");
    for page in [
        "anaquel.toml",
        "volumes/stray.md",
        "volumes/other/page.md",
        "volumes/guide2/page.md",
        "volumes/guide/volume.toml",
        "volumes/guide/.hidden.md",
        "volumes/guide/.cache/page.md",
        "../outside.md",
        "/volumes/guide/setup.md",
        "volumes\\guide\\setup.md",
        "",
    ] {
        let preview = preview_page(&s.0, page, "# Hijacked\n");
        assert!(preview.html.is_none(), "{page}");
        assert_eq!(preview.url, None, "{page}");
        assert!(
            !errors(&preview.page).is_empty(),
            "{page}: {:?}",
            preview.page
        );
    }
}

#[test]
fn a_folder_that_is_not_a_library_gives_the_problem_of_the_library() {
    let s = Scratch::new("preview-no-library");
    let preview = preview_page(&s.0, PAGE, "# Setup\n");
    assert!(preview.html.is_none());
    assert_eq!(preview.page, []);
    assert_eq!(errors(&preview.elsewhere), [Code::MANIFEST]);
}

#[test]
fn a_page_name_in_another_unicode_form_is_the_same_page() {
    let s = library("preview-nfc");
    // "é" as one character on disk, and as `e` with a combining accent in the request.
    s.put("volumes/guide/caf\u{e9}.md", "# Cafe\n");
    let composed = "volumes/guide/caf\u{e9}.md";
    let decomposed = "volumes/guide/cafe\u{301}.md";
    let one = preview_page(&s.0, composed, "# Edited\n");
    let other = preview_page(&s.0, decomposed, "# Edited\n");
    assert!(one.html.is_some(), "{:?}", one.page);
    assert_eq!(other.html, one.html);
    assert_eq!(other.url, one.url);
}

#[test]
fn the_files_the_html_refers_to_are_the_assets_of_the_theme() {
    let s = library("preview-assets");
    let html = preview_page(&s.0, PAGE, SETUP).html.unwrap();
    let assets = theme_assets().unwrap();
    assert_ne!(assets.len(), 0);
    assert!(
        assets
            .iter()
            .all(|a| a.path.starts_with("assets/") && !a.bytes.is_empty())
    );
    let stylesheets: Vec<&str> = assets
        .iter()
        .map(|a| a.path.as_str())
        .filter(|path| {
            Path::new(path)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("css"))
        })
        .collect();
    assert_ne!(stylesheets.len(), 0);
    for path in stylesheets {
        assert!(html.contains(&format!("/{path}")), "{path} is not linked");
    }
}

#[test]
fn a_text_longer_than_the_build_reads_is_refused_before_it_is_parsed() {
    let s = library("preview-too-long");
    let limit = usize::try_from(crate::MAX_PAGE_BYTES).unwrap();
    let preview = preview_page(&s.0, PAGE, &"a".repeat(limit + 1));
    assert_eq!(errors(&preview.page), [Code::IO]);
    assert!(
        preview.page[0]
            .message
            .contains("larger than the build reads")
    );
    assert!(preview.html.is_none());
    assert_eq!(preview.url, None);
    assert_eq!(preview.elsewhere, []);
    // The same text a byte shorter is a page like any other.
    let preview = preview_page(&s.0, PAGE, &"a".repeat(limit));
    assert_eq!(preview.page, []);
    assert!(preview.html.is_some());
}

#[test]
fn a_new_page_that_takes_the_address_of_another_is_an_error_of_the_new_page() {
    let s = library("preview-duplicate-slug");
    // The catalog reports the clash on whichever of the two it orders last; either way it exists
    // only because of this text, so it is the page's and the build would stop on it.
    let preview = preview_page(
        &s.0,
        "volumes/guide/a-dup.md",
        "---\nslug: setup\n---\n# Dup\n",
    );
    assert_eq!(errors(&preview.page), [Code::DUPLICATE_URL]);
    assert!(preview.html.is_none());
    assert_eq!(preview.elsewhere, []);
    // The build agrees.
    s.put("volumes/guide/a-dup.md", "---\nslug: setup\n---\n# Dup\n");
    assert!(check(&s.0).report.has_errors());
}

#[test]
fn a_clash_that_was_already_on_disk_is_not_blamed_on_the_page() {
    let s = library("preview-old-clash");
    s.put("volumes/guide/one.md", "---\nslug: same\n---\n# One\n");
    s.put("volumes/guide/two.md", "---\nslug: same\n---\n# Two\n");
    let preview = preview_page(&s.0, PAGE, "# Setup\n");
    assert_eq!(preview.page, []);
    assert!(preview.html.is_some());
    assert_eq!(errors(&preview.elsewhere), [Code::DUPLICATE_URL]);
}

#[test]
fn a_file_on_disk_that_cannot_be_read_does_not_stop_its_replacement() {
    let limit = usize::try_from(crate::MAX_PAGE_BYTES).unwrap();
    for (name, bytes) in [
        ("preview-disk-huge", vec![b'a'; limit + 1]),
        ("preview-disk-binary", vec![0xFF, 0xFE, 0x00]),
    ] {
        let s = library(name);
        fs::write(s.0.join(PAGE), &bytes).unwrap();
        // The file is unreadable, which `check` reports; the editor holds a text that replaces it.
        assert!(check(&s.0).report.has_errors(), "{name}");
        let preview = preview_page(&s.0, PAGE, "# Fixed\n");
        assert_eq!(preview.page, [], "{name}");
        assert!(preview.html.unwrap().contains("Fixed"), "{name}");
        assert_eq!(preview.elsewhere, [], "{name}");
    }
}

#[test]
fn a_name_that_would_change_the_address_is_refused_as_the_build_refuses_it() {
    let s = library("preview-address-chars");
    for page in [
        "volumes/guide/c#.md",
        "volumes/guide/100%.md",
        "volumes/guide/folder#/page.md",
    ] {
        let preview = preview_page(&s.0, page, "---\nslug: fine\n---\n# X\n");
        assert_eq!(errors(&preview.page), [Code::IO], "{page}");
        assert!(preview.html.is_none(), "{page}");
        s.put(page, "# X\n");
        assert!(check(&s.0).report.has_errors(), "{page}");
        fs::remove_file(s.0.join(page)).unwrap();
    }
    // `?` cannot be a file name on Windows, so this one is only asked of the preview.
    let preview = preview_page(&s.0, "volumes/guide/what?.md", "# X\n");
    assert_eq!(errors(&preview.page), [Code::IO]);
    assert!(preview.html.is_none());
}

#[test]
fn the_folder_of_the_library_is_not_sent_to_the_client() {
    let s = library("preview-hide-root");
    let root = s.0.display().to_string();
    let diagnostics = vec![
        Diagnostic::error(Code::IO, format!("{root}: access denied")),
        Diagnostic::error(
            Code::IO,
            format!("{}: access denied", root.replace('\\', "/")),
        )
        .with_help(format!("look in {root}")),
    ];
    let shown = super::prepared(diagnostics, &s.0);
    assert_eq!(shown.len(), 2);
    for diagnostic in &shown {
        assert_eq!(diagnostic.message, "<library>: access denied");
    }
    assert!(
        shown
            .iter()
            .filter_map(|d| d.help.as_deref())
            .all(|h| h == "look in <library>")
    );
}

#[test]
fn a_page_that_is_an_index_or_has_a_capital_extension_is_previewed() {
    let s = library("preview-index");
    let index = preview_page(&s.0, "volumes/guide/_index.md", "# A new guide title\n");
    assert!(index.html.unwrap().contains("A new guide title"));
    let loud = preview_page(&s.0, "volumes/guide/LOUD.MD", "# Loud\n");
    assert_eq!(loud.page, []);
    assert!(loud.html.is_some());
}

#[test]
fn two_previews_in_a_row_share_nothing() {
    let s = library("preview-independent");
    let first = preview_page(&s.0, PAGE, "# First text\n").html.unwrap();
    let second = preview_page(&s.0, PAGE, "# Second text\n").html.unwrap();
    assert!(first.contains("First text") && !first.contains("Second text"));
    assert!(second.contains("Second text") && !second.contains("First text"));
    assert_eq!(fs::read_to_string(s.0.join(PAGE)).unwrap(), "# Old setup\n");
}

#[test]
fn a_directive_that_does_not_exist_and_raw_html_are_errors_of_the_page() {
    let s = library("preview-composition-errors");
    let unknown = preview_page(&s.0, PAGE, "# Setup\n\n:::nonexistent\nbody\n:::\n");
    assert_eq!(errors(&unknown.page), [Code::DIRECTIVE]);
    assert!(unknown.html.is_none());
    let raw = preview_page(&s.0, PAGE, "# Setup\n\n<script>alert(1)</script>\n");
    assert_eq!(errors(&raw.page), [Code::RAW_HTML]);
    assert!(raw.html.is_none());
}

#[test]
fn the_frames_a_page_embeds_are_reported_for_the_policy_of_the_server() {
    let s = library("preview-frames");
    let plain = preview_page(&s.0, PAGE, "# Setup\n");
    assert_eq!(plain.frame_origins, Vec::<String>::new());
    let embed = "# Setup\n\n:::embed url=\"https://www.youtube.com/watch?v=aqz-KE-bpKQ\"\n:::\n";
    let preview = preview_page(&s.0, PAGE, embed);
    assert_eq!(preview.page, []);
    assert_eq!(
        preview.frame_origins.len(),
        1,
        "{:?}",
        preview.frame_origins
    );
    assert!(preview.frame_origins[0].starts_with("https://"));
}
