use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::Path;

use anaquel_diagnostics::Code;

use super::destination::{file_name_problem, windows_device};
use super::disk::{
    Change, Disk, MAX_CHANGES, POSITION_DIGITS, RealDisk, TEMPORARY_EXTRA, apply, within_limit,
};
use super::plan::{Address, check_addresses, lost_addresses};
use super::text::{encode, read_destination, relative, set_aliases};
use super::{RelocateOptions, RelocateOutcome, relocate, relocate_on};
use crate::tests::Scratch;
use crate::{build, check};

fn fixture(name: &str) -> Scratch {
    let s = Scratch::new(name);
    s.put(
        "anaquel.toml",
        "schema = 1\nengine = \"0.1\"\ntitle = \"Demo\"\n",
    );
    s.put("volumes/guide/volume.toml", "title = \"Guide\"\n");
    s.put(
        "volumes/guide/_index.md",
        "# Guide\n\nSee [setup](setup.md#install) and [[cache]].\n",
    );
    s.put(
        "volumes/guide/setup.md",
        "---\ndescription: Set it up.\n---\n# Setup\n\n## Install\n\nBack to [guide](_index.md). ![logo](img/logo.txt)\n",
    );
    s.put(
        "volumes/guide/terms.md",
        "# Terms\n\n### Cache\n\nStores copies.\n",
    );
    s.put("volumes/guide/img/logo.txt", "logo");
    s.put("volumes/guide/ref/_index.md", "# Ref\n");
    s.put(
        "volumes/guide/ref/api.md",
        "# API\n\nSee [setup](../setup.md) and [ref][r].\n\n[r]: _index.md \"Title\"\n",
    );
    s.put("volumes/guide/ref/pic.txt", "pic");
    s.put(
        "volumes/guide/slugged.md",
        "---\nslug: slugged-stable\n---\n# Slugged\n",
    );
    s.put("volumes/other/volume.toml", "title = \"Other\"\n");
    s.put(
        "volumes/other/_index.md",
        "# Other\n\nSee [setup](../guide/setup.md).\n",
    );
    s
}

fn shelves(name: &str) -> Scratch {
    let s = Scratch::new(name);
    s.put(
        "anaquel.toml",
        "schema = 1\nengine = \"0.1\"\ntitle = \"Shelves\"\n",
    );
    for shelf in ["guides", "reference"] {
        s.put(
            &format!("shelves/{shelf}/shelf.toml"),
            &format!("title = \"{shelf}\"\n"),
        );
        s.put(
            &format!("shelves/{shelf}/volumes/start/volume.toml"),
            "title = \"Start\"\n",
        );
        s.put(
            &format!("shelves/{shelf}/volumes/start/_index.md"),
            "# Start\n",
        );
    }
    s.put(
        "shelves/guides/volumes/start/intro.md",
        "# Intro\n\nBack to [start](_index.md).\n",
    );
    s.put("volumes/loose/volume.toml", "title = \"Loose\"\n");
    s.put(
        "volumes/loose/_index.md",
        "# Loose\n\nRead [the intro](../../shelves/guides/volumes/start/intro.md).\n",
    );
    s.put("volumes/loose/note.md", "# Note\n");
    s
}

fn go(s: &Scratch, from: &str, to: &str, dry_run: bool) -> RelocateOutcome {
    relocate(&RelocateOptions {
        root: s.0.clone(),
        from: from.to_owned(),
        to: to.to_owned(),
        dry_run,
    })
}

fn codes(outcome: &RelocateOutcome) -> Vec<Code> {
    outcome.report.items().iter().map(|d| d.code).collect()
}

fn snapshot(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    let mut found = BTreeMap::new();
    for item in walkdir::WalkDir::new(root).min_depth(1) {
        let item = item.unwrap();
        let relative = item
            .path()
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if relative == "dist" || relative.starts_with("dist/") {
            continue;
        }
        let bytes = item
            .file_type()
            .is_file()
            .then(|| fs::read(item.path()).unwrap());
        found.insert(relative, bytes);
    }
    found
}

fn read(s: &Scratch, path: &str) -> String {
    fs::read_to_string(s.0.join(path)).unwrap()
}

fn assert_clean(s: &Scratch) {
    let outcome = check(&s.0);
    assert!(!outcome.report.has_errors(), "{:?}", outcome.report.items());
}

#[test]
fn a_move_leaves_a_redirect_and_fixes_the_links() {
    let s = fixture("reloc-basic");
    assert!(!build(&s.options()).report.has_errors());
    let out = go(
        &s,
        "volumes/guide/setup.md",
        "volumes/guide/install.md",
        false,
    );
    assert!(out.applied, "{:?}", out.report.items());
    assert!(!s.0.join("volumes/guide/setup.md").exists());
    assert!(read(&s, "volumes/guide/_index.md").contains("[setup](install.md#install)"));
    assert!(read(&s, "volumes/other/_index.md").contains("(../guide/install.md)"));
    assert!(read(&s, "volumes/guide/install.md").contains("aliases:\n  - /guide/setup/\n"));
    assert_clean(&s);
    let rebuilt = build(&s.options());
    assert!(!rebuilt.report.has_errors(), "{:?}", rebuilt.report.items());
    assert!(
        s.read("_redirects")
            .contains("/guide/setup/ /guide/install/ 301")
    );
}

#[test]
fn a_dry_run_writes_nothing() {
    let s = fixture("reloc-dry");
    let before = snapshot(&s.0);
    let out = go(
        &s,
        "volumes/guide/setup.md",
        "volumes/guide/install.md",
        true,
    );
    assert!(!out.applied);
    assert!(!out.report.has_errors());
    assert!(out.plan.iter().any(|l| l.starts_with("move ")));
    assert_eq!(snapshot(&s.0), before);
}

#[test]
fn moving_into_a_folder_rewrites_the_links_inside_the_page() {
    let s = fixture("reloc-inside");
    let out = go(
        &s,
        "volumes/guide/setup.md",
        "volumes/guide/ref/setup.md",
        false,
    );
    assert!(out.applied, "{:?}", out.report.items());
    let page = read(&s, "volumes/guide/ref/setup.md");
    assert!(page.contains("[guide](../_index.md)"), "{page}");
    assert!(page.contains("![logo](../img/logo.txt)"), "{page}");
    assert!(read(&s, "volumes/guide/ref/api.md").contains("[setup](setup.md)"));
    assert_clean(&s);
}

#[test]
fn reference_definitions_and_titles_are_rewritten() {
    let s = fixture("reloc-definitions");
    let out = go(
        &s,
        "volumes/guide/ref/_index.md",
        "volumes/guide/api/_index.md",
        false,
    );
    assert!(out.applied, "{:?}", out.report.items());
    let api = read(&s, "volumes/guide/api/api.md");
    assert!(api.contains("[r]: _index.md \"Title\""), "{api}");
    assert!(api.contains("[setup](../setup.md)"), "{api}");
    assert_clean(&s);

    let t = fixture("reloc-definitions-two");
    let out = go(
        &t,
        "volumes/guide/ref/_index.md",
        "volumes/guide/ref/_index.md",
        false,
    );
    assert!(codes(&out).contains(&Code::RELOCATE_TAKEN));
    t.put(
        "volumes/guide/links.md",
        "# Links\n\n[a]: ref/api.md#api \"Title\"\n[b]: <ref/api.md>\n\nSee [one][a], [two][b] and [three](<ref/api.md> \"t\").\n",
    );
    let out = go(
        &t,
        "volumes/guide/ref/api.md",
        "volumes/guide/api.md",
        false,
    );
    assert!(out.applied, "{:?}", out.report.items());
    let links = read(&t, "volumes/guide/links.md");
    assert!(links.contains("[a]: api.md#api \"Title\""), "{links}");
    assert!(links.contains("[b]: <api.md>"), "{links}");
    assert!(links.contains("[three](<api.md> \"t\")"), "{links}");
    assert_clean(&t);
}

#[test]
fn a_folder_moves_with_its_pages_and_files() {
    let s = fixture("reloc-folder");
    let out = go(
        &s,
        "volumes/guide/ref/_index.md",
        "volumes/guide/api/_index.md",
        false,
    );
    assert!(out.applied, "{:?}", out.report.items());
    assert!(!s.0.join("volumes/guide/ref").exists());
    assert!(s.0.join("volumes/guide/api/pic.txt").exists());
    assert!(read(&s, "volumes/guide/api/api.md").contains("  - /guide/ref/api/\n"));
    assert!(read(&s, "volumes/guide/api/_index.md").contains("  - /guide/ref/\n"));
    assert_clean(&s);
}

#[test]
fn a_page_moves_between_volumes() {
    let s = fixture("reloc-volumes");
    let out = go(
        &s,
        "volumes/guide/setup.md",
        "volumes/other/setup.md",
        false,
    );
    assert!(out.applied, "{:?}", out.report.items());
    assert!(read(&s, "volumes/other/setup.md").contains("  - /guide/setup/\n"));
    assert!(read(&s, "volumes/guide/_index.md").contains("(../other/setup.md#install)"));
    assert!(read(&s, "volumes/other/setup.md").contains("(../guide/_index.md)"));
    assert_clean(&s);
}

#[test]
fn pages_move_between_shelves_and_into_a_shelf() {
    let s = shelves("reloc-shelves");
    let out = go(
        &s,
        "shelves/guides/volumes/start/intro.md",
        "shelves/reference/volumes/start/intro.md",
        false,
    );
    assert!(out.applied, "{:?}", out.report.items());
    let moved = read(&s, "shelves/reference/volumes/start/intro.md");
    assert!(moved.contains("  - /guides/start/intro/\n"), "{moved}");
    assert!(
        moved.contains("(../../../guides/volumes/start/_index.md)"),
        "{moved}"
    );
    assert!(
        read(&s, "volumes/loose/_index.md").contains("shelves/reference/volumes/start/intro.md")
    );
    assert_clean(&s);

    let out = go(
        &s,
        "volumes/loose/note.md",
        "shelves/guides/volumes/start/note.md",
        false,
    );
    assert!(out.applied, "{:?}", out.report.items());
    assert!(read(&s, "shelves/guides/volumes/start/note.md").contains("  - /loose/note/\n"));
    assert_clean(&s);
}

#[test]
fn a_move_twice_and_back_keeps_every_old_address() {
    let s = fixture("reloc-twice");
    for (from, to) in [
        ("setup", "install"),
        ("install", "start"),
        ("start", "setup"),
    ] {
        let out = go(
            &s,
            &format!("volumes/guide/{from}.md"),
            &format!("volumes/guide/{to}.md"),
            false,
        );
        assert!(out.applied, "{:?}", out.report.items());
        assert_clean(&s);
    }
    let page = read(&s, "volumes/guide/setup.md");
    assert!(page.contains("  - /guide/install/\n"), "{page}");
    assert!(page.contains("  - /guide/start/\n"), "{page}");
    assert!(!page.contains("  - /guide/setup/\n"), "{page}");
    let built = build(&s.options());
    assert!(!built.report.has_errors(), "{:?}", built.report.items());
    let redirects = s.read("_redirects");
    assert!(redirects.contains("/guide/install/ /guide/setup/ 301"));
    assert!(redirects.contains("/guide/start/ /guide/setup/ 301"));
    assert!(!redirects.contains("/guide/setup/ /guide/setup/"));
}

#[test]
fn a_page_with_a_slug_keeps_its_address() {
    let s = fixture("reloc-slug");
    s.put(
        "volumes/guide/old-name.md",
        "---\nslug: stable\n---\n# Page\n",
    );
    let out = go(
        &s,
        "volumes/guide/old-name.md",
        "volumes/guide/new-name.md",
        false,
    );
    assert!(out.applied, "{:?}", out.report.items());
    assert!(!read(&s, "volumes/guide/new-name.md").contains("aliases"));
    assert!(out.plan.iter().any(|l| l.contains("no redirect")));
    assert_clean(&s);
}

#[test]
fn a_draft_is_renamed_without_a_redirect() {
    let s = fixture("reloc-draft");
    s.put("volumes/guide/wip.md", "---\ndraft: true\n---\n# Wip\n");
    let out = go(&s, "volumes/guide/wip.md", "volumes/guide/later.md", false);
    assert!(out.applied, "{:?}", out.report.items());
    assert!(!read(&s, "volumes/guide/later.md").contains("aliases"));
    assert_clean(&s);
}

#[test]
fn a_redirect_that_differs_only_by_case_is_refused() {
    let s = fixture("reloc-case");
    let before = snapshot(&s.0);
    let out = go(
        &s,
        "volumes/guide/setup.md",
        "volumes/guide/Setup.md",
        false,
    );
    assert!(!out.applied);
    assert!(
        codes(&out).contains(&Code::RELOCATE_DESTINATION),
        "{:?}",
        out.report.items()
    );
    assert_eq!(snapshot(&s.0), before);

    let out = go(
        &s,
        "volumes/guide/slugged.md",
        "volumes/guide/Slugged.md",
        false,
    );
    assert!(out.applied, "{:?}", out.report.items());
    assert!(!read(&s, "volumes/guide/Slugged.md").contains("aliases"));
    assert_clean(&s);
}

#[test]
fn a_destination_name_is_normalised_before_it_is_used() {
    let s = fixture("reloc-nfc");
    let out = go(
        &s,
        "volumes/guide/setup.md",
        "volumes/guide/cafe\u{301}.md",
        false,
    );
    assert!(out.applied, "{:?}", out.report.items());
    let page = read(&s, "volumes/guide/caf\u{e9}.md");
    assert!(page.contains("  - /guide/setup/\n"), "{page}");
    assert_clean(&s);
}

#[test]
fn a_link_that_stays_the_same_is_not_rewritten() {
    let s = fixture("reloc-same-link");
    let out = go(
        &s,
        "volumes/guide/ref/api.md",
        "volumes/guide/other/api.md",
        false,
    );
    assert!(out.applied, "{:?}", out.report.items());
    let page = read(&s, "volumes/guide/other/api.md");
    assert!(page.contains("[setup](../setup.md)"), "{page}");
    assert!(page.contains("[r]: ../ref/_index.md \"Title\""), "{page}");
    assert!(
        out.plan.iter().any(|l| l.contains("rewrite 1 link(s)")),
        "{:?}",
        out.plan
    );
    assert_clean(&s);
}

#[test]
fn chained_folder_moves_and_statics_do_not_overwrite_each_other() {
    let s = fixture("reloc-chain-statics");
    s.put("volumes/guide/a/x/_index.md", "# X\n");
    s.put("volumes/guide/a/x/z.png", "one");
    s.put("volumes/guide/a/x/x/z.png", "two");
    let out = go(
        &s,
        "volumes/guide/a/x/_index.md",
        "volumes/guide/a/_index.md",
        false,
    );
    assert!(out.applied, "{:?}", out.report.items());
    assert_eq!(read(&s, "volumes/guide/a/z.png"), "one");
    assert_eq!(read(&s, "volumes/guide/a/x/z.png"), "two");
}

#[test]
fn a_swap_of_two_pages_through_a_folder_move_is_refused() {
    let s = Scratch::new("reloc-chain-pages");
    s.put(
        "anaquel.toml",
        "schema = 1\nengine = \"0.1\"\ntitle = \"Chain\"\n",
    );
    s.put("volumes/g/volume.toml", "title = \"G\"\n");
    s.put("volumes/g/_index.md", "# G\n");
    s.put("volumes/g/a/b/_index.md", "# B\n");
    s.put("volumes/g/a/b/b/m.md", "# M\n");
    s.put("volumes/g/a/b/b/b/m.md", "# M2\n");
    let before = snapshot(&s.0);
    let out = go(
        &s,
        "volumes/g/a/b/_index.md",
        "volumes/g/a/_index.md",
        false,
    );
    assert!(!out.applied);
    assert!(
        codes(&out).contains(&Code::RELOCATE_CHAIN),
        "{:?}",
        out.report.items()
    );
    assert_eq!(snapshot(&s.0), before);
}

#[test]
fn mixed_line_endings_stop_the_move() {
    let s = fixture("reloc-mixed");
    fs::write(
        s.0.join("volumes/guide/setup.md"),
        "# Setup\r\n\r\n## Install\n\nBack to [guide](_index.md).\r\n",
    )
    .unwrap();
    let before = snapshot(&s.0);
    let out = go(
        &s,
        "volumes/guide/setup.md",
        "volumes/guide/install.md",
        false,
    );
    assert!(
        codes(&out).contains(&Code::RELOCATE_REWRITE),
        "{:?}",
        out.report.items()
    );
    assert_eq!(snapshot(&s.0), before);
}

#[cfg(unix)]
#[test]
fn a_destination_behind_a_symbolic_link_is_refused() {
    let s = fixture("reloc-symlink");
    let outside = Scratch::new("reloc-symlink-outside");
    std::os::unix::fs::symlink(&outside.0, s.0.join("volumes/guide/link")).unwrap();
    let before = snapshot(&s.0);
    let out = go(
        &s,
        "volumes/guide/setup.md",
        "volumes/guide/link/setup.md",
        false,
    );
    assert!(!out.applied);
    assert!(
        codes(&out).contains(&Code::RELOCATE_DESTINATION),
        "{:?}",
        out.report.items()
    );
    assert_eq!(snapshot(&s.0), before);
    assert_eq!(fs::read_dir(&outside.0).unwrap().count(), 0);
}

#[test]
fn line_endings_and_a_byte_order_mark_survive() {
    let s = fixture("reloc-crlf");
    fs::write(
        s.0.join("volumes/guide/setup.md"),
        "\u{feff}---\r\ndescription: Set it up.\r\n---\r\n# Setup\r\n\r\n## Install\r\n\r\nText.\r\n",
    )
    .unwrap();
    let out = go(
        &s,
        "volumes/guide/setup.md",
        "volumes/guide/install.md",
        false,
    );
    assert!(out.applied, "{:?}", out.report.items());
    let bytes = fs::read(s.0.join("volumes/guide/install.md")).unwrap();
    assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    let text = String::from_utf8(bytes).unwrap();
    assert!(
        text.contains("aliases:\r\n  - /guide/setup/\r\n"),
        "{text:?}"
    );
    assert!(!text.replace("\r\n", "").contains('\n'), "{text:?}");
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one table of refusals; splitting it would hide the cases"
)]
fn every_refusal_leaves_the_tree_as_it_was() {
    let long = format!("volumes/guide/{}.md", "a".repeat(240));
    let long_name = format!("volumes/guide/{}.md", "b".repeat(200));
    let cases: Vec<(&str, &str, Code)> = vec![
        (
            "volumes/guide/nope.md",
            "volumes/guide/x.md",
            Code::RELOCATE_SOURCE,
        ),
        (
            "volumes/guide/img/logo.txt",
            "volumes/guide/x.md",
            Code::RELOCATE_SOURCE,
        ),
        ("docs/a.md", "volumes/guide/x.md", Code::RELOCATE_SOURCE),
        (
            "volumes/missing/a.md",
            "volumes/guide/x.md",
            Code::RELOCATE_SOURCE,
        ),
        (
            "volumes/guide/../other/a.md",
            "volumes/guide/x.md",
            Code::RELOCATE_SOURCE,
        ),
        (
            "volumes/guide/setup.md",
            "volumes/guide/new page.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/setup.md",
            "volumes/guide/a#b.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/setup.md",
            "volumes/nowhere/x.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/setup.md",
            "shelves/none/volumes/guide/x.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/setup.md",
            "volumes/guide/con.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/setup.md",
            "volumes/guide/NUL.txt.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/setup.md",
            "volumes/guide/com1.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/setup.md",
            "volumes/guide/Terms.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/setup.md",
            "volumes/guide/x.txt",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/setup.md",
            "guide/x.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/setup.md",
            long.as_str(),
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/ref/_index.md",
            "volumes/guide/ref/in/_index.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/ref/_index.md",
            "volumes/guide/other.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/setup.md",
            "volumes/guide/terms.md",
            Code::RELOCATE_TAKEN,
        ),
        (
            "volumes/guide/setup.md",
            "volumes/guide/setup.md",
            Code::RELOCATE_TAKEN,
        ),
        (
            "volumes/guide/setup.md",
            "volumes/guide/img/logo.txt/x.md",
            Code::RELOCATE_TAKEN,
        ),
        (
            "volumes/other/_index.md",
            "volumes/other/home.md",
            Code::MANIFEST,
        ),
        (
            "volumes/guide/setup.md",
            "volumes/guide/.attic/a.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/setup.md",
            "volumes/guide/.hidden.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/slugged.md",
            "volumes/guide/a%b.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/slugged.md",
            "volumes/guide/a:b.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/slugged.md",
            "volumes/guide/a<b>.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/slugged.md",
            "volumes/guide/a?b.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/ref/_index.md",
            "volumes/guide/.hid/_index.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/setup.md",
            "volumes/guide/dir /setup.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/ref/_index.md",
            "volumes/guide/REF/_index.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/setup.md",
            "volumes/guide/REF/setup.md",
            Code::RELOCATE_DESTINATION,
        ),
        (
            "volumes/guide/setup.md",
            long_name.as_str(),
            Code::RELOCATE_DESTINATION,
        ),
    ];
    for (from, to, expected) in cases {
        let s = fixture("reloc-refusals");
        let before = snapshot(&s.0);
        let out = go(&s, from, to, false);
        assert!(!out.applied, "{from} -> {to}");
        assert!(
            codes(&out).contains(&expected),
            "{from} -> {to}: {:?}",
            out.report.items()
        );
        assert_eq!(snapshot(&s.0), before, "{from} -> {to}");
    }
}

#[test]
fn a_term_the_new_volume_lacks_stops_the_move() {
    let s = fixture("reloc-term");
    s.put("volumes/guide/uses.md", "# Uses\n\nSee [[cache]].\n");
    let before = snapshot(&s.0);
    let out = go(&s, "volumes/guide/uses.md", "volumes/other/uses.md", false);
    assert!(!out.applied);
    assert!(
        codes(&out).contains(&Code::UNKNOWN_TERM),
        "{:?}",
        out.report.items()
    );
    assert_eq!(snapshot(&s.0), before);
}

#[test]
fn an_alias_of_another_page_names_its_owner() {
    let s = fixture("reloc-owner");
    s.put(
        "volumes/guide/claim.md",
        "---\naliases:\n  - /guide/install/\n---\n# Claim\n",
    );
    let before = snapshot(&s.0);
    let out = go(
        &s,
        "volumes/guide/setup.md",
        "volumes/guide/install.md",
        false,
    );
    assert!(codes(&out).contains(&Code::RELOCATE_TAKEN));
    assert!(
        out.report
            .items()
            .iter()
            .any(|d| d.message.contains("volumes/guide/claim.md"))
    );
    assert_eq!(snapshot(&s.0), before);
}

#[test]
fn text_that_cannot_be_rewritten_stops_the_move() {
    let s = fixture("reloc-rewrite");
    s.put(
        "volumes/guide/noted.md",
        "---\naliases:\n  # kept for a reason\n  - /guide/old/\n---\n# Noted\n",
    );
    let before = snapshot(&s.0);
    let out = go(
        &s,
        "volumes/guide/noted.md",
        "volumes/guide/renamed.md",
        false,
    );
    assert!(
        codes(&out).contains(&Code::RELOCATE_REWRITE),
        "{:?}",
        out.report.items()
    );
    assert_eq!(snapshot(&s.0), before);

    for (name, front) in [
        ("flow", "aliases: [/guide/old-flow/]  # note"),
        ("item", "aliases:\n  - /guide/old-item/ # why"),
    ] {
        let from = format!("volumes/guide/{name}.md");
        s.put(&from, &format!("---\n{front}\n---\n# Page\n"));
        let before = snapshot(&s.0);
        let out = go(&s, &from, &format!("volumes/guide/{name}-new.md"), false);
        assert!(
            codes(&out).contains(&Code::RELOCATE_REWRITE),
            "{name}: {:?}",
            out.report.items()
        );
        assert!(out.report.items().iter().all(|d| d.help.is_some()));
        assert_eq!(snapshot(&s.0), before, "{name}");
        fs::remove_file(s.0.join(&from)).unwrap();
    }

    s.put(
        "volumes/guide/cards.md",
        "# Cards\n\n::::cards cols=2\n:::card title=\"Setup\" href=\"setup.md\"\ntext\n:::\n::::\n",
    );
    let before = snapshot(&s.0);
    let out = go(
        &s,
        "volumes/guide/setup.md",
        "volumes/guide/install.md",
        false,
    );
    assert!(
        codes(&out).contains(&Code::RELOCATE_REWRITE),
        "{:?}",
        out.report.items()
    );
    assert_eq!(snapshot(&s.0), before);
}

#[test]
fn a_swap_or_chain_of_addresses_is_refused() {
    let mut addresses: HashMap<String, Address> = HashMap::new();
    addresses.insert(
        "a".to_owned(),
        Address {
            old: "/v/x/".to_owned(),
            new: "/v/y/".to_owned(),
        },
    );
    addresses.insert(
        "b".to_owned(),
        Address {
            old: "/v/y/".to_owned(),
            new: "/v/z/".to_owned(),
        },
    );
    let mut report = anaquel_diagnostics::Report::new();
    assert!(!check_addresses(&addresses, &HashMap::new(), &mut report));
    assert!(
        report
            .items()
            .iter()
            .any(|d| d.code == Code::RELOCATE_CHAIN)
    );
}

struct Failing {
    inner: RealDisk,
    fail_at: Vec<usize>,
    count: usize,
}

impl Failing {
    fn tick(&mut self) -> io::Result<()> {
        self.count += 1;
        if self.fail_at.contains(&self.count) {
            Err(io::Error::other("injected"))
        } else {
            Ok(())
        }
    }
}

impl Disk for Failing {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.inner.read(path)
    }

    fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path)
    }

    fn write(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.tick()?;
        self.inner.write(path, bytes)
    }

    fn create_new(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.tick()?;
        self.inner.create_new(path, bytes)
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        self.tick()?;
        self.inner.rename(from, to)
    }

    fn remove_file(&mut self, path: &Path) -> io::Result<()> {
        self.tick()?;
        self.inner.remove_file(path)
    }

    fn create_dir(&mut self, path: &Path) -> io::Result<()> {
        self.tick()?;
        self.inner.create_dir(path)
    }

    fn remove_dir(&mut self, path: &Path) -> io::Result<()> {
        self.inner.remove_dir(path)
    }
}

fn interrupted(
    name: &str,
    from: &str,
    to: &str,
    fail_at: Vec<usize>,
) -> (Scratch, RelocateOutcome, BTreeMap<String, Option<Vec<u8>>>) {
    let s = fixture(name);
    s.put(
        "volumes/guide/ref/deep/note.md",
        "# Note\n\nSee [api](../api.md).\n",
    );
    let before = snapshot(&s.0);
    let mut disk = Failing {
        inner: RealDisk,
        fail_at,
        count: 0,
    };
    let out = relocate_on(
        &RelocateOptions {
            root: s.0.clone(),
            from: from.to_owned(),
            to: to.to_owned(),
            dry_run: false,
        },
        &mut disk,
    );
    (s, out, before)
}

#[test]
fn an_interrupted_write_restores_every_file() {
    let moves = [
        ("volumes/guide/setup.md", "volumes/guide/new/setup.md"),
        ("volumes/guide/ref/_index.md", "volumes/guide/api/_index.md"),
        (
            "volumes/guide/ref/_index.md",
            "volumes/other/borrowed/_index.md",
        ),
    ];
    for (from, to) in moves {
        let mut failures = 0;
        let mut finished = false;
        for fail_at in 1..300 {
            let (s, out, before) = interrupted("reloc-interrupted", from, to, vec![fail_at]);
            if out.applied {
                assert_clean(&s);
                finished = true;
                break;
            }
            failures += 1;
            assert!(out.report.has_errors());
            assert!(!out.incomplete);
            assert_eq!(snapshot(&s.0), before, "{from}: failing at step {fail_at}");
        }
        assert!(failures >= 5, "{from}: {failures}");
        // The loop must reach a step number past the last step, or it proved nothing about it.
        assert!(finished, "{from}: no run went through");
    }
}

#[test]
fn a_failure_while_putting_files_back_is_reported() {
    let (s, out, before) = interrupted(
        "reloc-stuck",
        "volumes/guide/setup.md",
        "volumes/guide/new/setup.md",
        vec![6, 7, 8, 9, 10, 11, 12],
    );
    assert!(!out.applied);
    assert!(out.incomplete, "{:?}", out.report.items());
    assert!(out.report.items().iter().any(|d| {
        d.help
            .as_deref()
            .is_some_and(|h| h.contains("could not be put back"))
    }));
    assert_ne!(snapshot(&s.0), before);
}

#[test]
fn the_disk_layer_never_overwrites_a_destination() {
    let s = fixture("reloc-no-overwrite");
    let before = snapshot(&s.0);
    let changes = vec![Change::Move {
        from: s.0.join("volumes/guide/setup.md"),
        to: s.0.join("volumes/guide/terms.md"),
        text: None,
    }];
    let failure = apply(&mut RealDisk, &changes, &[]).err().unwrap();
    assert!(failure.restored);
    assert!(
        failure.message.contains("already exists"),
        "{}",
        failure.message
    );
    assert_eq!(snapshot(&s.0), before);

    let changes = vec![Change::Move {
        from: s.0.join("volumes/guide/setup.md"),
        to: s.0.join("volumes/guide/terms.md"),
        text: Some("# changed\n".to_owned()),
    }];
    let failure = apply(&mut RealDisk, &changes, &[]).err().unwrap();
    assert!(failure.restored);
    assert_eq!(snapshot(&s.0), before);
}

#[test]
fn an_address_that_would_vanish_is_caught() {
    let s = fixture("reloc-lost-before");
    let t = fixture("reloc-lost-after");
    fs::remove_file(t.0.join("volumes/guide/setup.md")).unwrap();
    t.put("volumes/guide/_index.md", "# Guide\n\nSee [[cache]].\n");
    let catalog = |root: &Path| {
        let mut report = anaquel_diagnostics::Report::new();
        let ingested = crate::ingest::ingest(root, crate::ENGINE_VERSION, &mut report).unwrap();
        crate::analyse_sources(ingested, &mut report).catalog
    };
    let lost = lost_addresses(&catalog(&s.0), &catalog(&t.0));
    assert_eq!(lost, vec!["/guide/setup/".to_owned()]);
    assert_eq!(
        lost_addresses(&catalog(&s.0), &catalog(&s.0)),
        Vec::<String>::new()
    );
}

#[test]
fn file_names_are_checked_for_every_system() {
    for bad in [
        ".hidden", "a%b", "a:b", "a<b", "a>b", "a|b", "a?b", "a*b", "a\"b", "a#b", "trail ",
        "dot.", "tab\t",
    ] {
        assert!(file_name_problem(bad).is_some(), "{bad:?}");
    }
    for fine in ["setup.md", "café.md", "a b.md", "a-b_c.md", "v1.2.md"] {
        assert_eq!(file_name_problem(fine), None, "{fine:?}");
    }
}

#[test]
fn reserved_and_plain_names_are_told_apart() {
    for bad in [
        "con",
        "CON",
        "nul.txt",
        "Aux.md",
        "com1",
        "LPT9.md",
        "prn ",
        "com¹",
        "COM².md",
        "lpt³",
        "conin$",
        "CONOUT$.md",
    ] {
        assert!(windows_device(bad), "{bad}");
    }
    for fine in [
        "console", "setup", "com10", "com", "lpt", "a.con", "comma", "com⁴", "com¹¹", "conin",
    ] {
        assert!(!windows_device(fine), "{fine}");
    }
}

#[test]
fn alias_lists_are_edited_in_place() {
    let list = |items: &[&str]| items.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        set_aliases("# Hi\n", &list(&["/a/"])).unwrap(),
        "---\naliases:\n  - /a/\n---\n# Hi\n"
    );
    assert_eq!(set_aliases("# Hi\n", &[]).unwrap(), "# Hi\n");
    assert_eq!(
        set_aliases("---\ntitle: T\n---\nBody\n", &list(&["/a/"])).unwrap(),
        "---\ntitle: T\naliases:\n  - /a/\n---\nBody\n"
    );
    assert_eq!(
        set_aliases(
            "---\naliases: [/x/, /y/]\ntitle: T\n---\nB\n",
            &list(&["/x/", "/z/"])
        )
        .unwrap(),
        "---\naliases:\n  - /x/\n  - /z/\ntitle: T\n---\nB\n"
    );
    assert_eq!(
        set_aliases("---\ntitle: T\naliases:\n- /x/\n\n---\nB\n", &[]).unwrap(),
        "---\ntitle: T\n\n---\nB\n"
    );
    assert!(set_aliases("---\naliases: # c\n---\n", &list(&["/a/"])).is_err());
    assert!(set_aliases("---\naliases:\n  # c\n  - /x/\n---\n", &list(&["/a/"])).is_err());
}

#[test]
fn paths_are_made_relative_and_safe_to_write() {
    assert_eq!(relative("a/b", "a/c.md"), "../c.md");
    assert_eq!(relative("a", "a/c.md"), "c.md");
    assert_eq!(relative("", "a/c.md"), "a/c.md");
    assert_eq!(relative("a/b", "x/y.md"), "../../x/y.md");
    assert_eq!(relative("a", "a/b/c.md"), "b/c.md");
    assert_eq!(encode("a b(1).md"), "a%20b%281%29.md");
    assert_eq!(encode("ñ/ü.md"), "ñ/ü.md");
    assert_eq!(read_destination("](a.md \"t\")", 2), Some((2, 6)));
    assert_eq!(read_destination("](<a b.md>)", 2), Some((3, 9)));
    assert_eq!(read_destination("](a(b).md)", 2), Some((2, 9)));
    assert_eq!(read_destination("]()", 2), None);
}

/// Every `MARKER-name` word found in any file under `root`, so a test can tell whether a page
/// survived a move whatever its new name.
fn markers_on_disk(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for item in walkdir::WalkDir::new(root).min_depth(1) {
        let item = item.unwrap();
        if item.file_type().is_file()
            && let Ok(text) = fs::read_to_string(item.path())
        {
            for word in text.split(|c: char| !(c.is_alphanumeric() || c == '-')) {
                if word.starts_with("MARKER-") {
                    found.push(word.to_owned());
                }
            }
        }
    }
    found.sort();
    found
}

#[test]
fn files_that_trade_places_in_a_folder_move_are_all_kept() {
    // `sub/intro.md` (a draft) leaves for `intro.md`, and `sub/sub/intro.md` takes its old place.
    let s = fixture("reloc-trade-places");
    s.put("volumes/guide/a/sub/_index.md", "# Sub\n\nMARKER-index\n");
    s.put(
        "volumes/guide/a/sub/intro.md",
        "---\ndraft: true\n---\n# Draft\n\nMARKER-draft\n",
    );
    s.put(
        "volumes/guide/a/sub/sub/intro.md",
        "# Intro\n\nMARKER-intro\n",
    );
    let before = snapshot(&s.0);

    let out = go(
        &s,
        "volumes/guide/a/sub/_index.md",
        "volumes/guide/a/_index.md",
        false,
    );

    let kept = markers_on_disk(&s.0);
    for marker in ["MARKER-index", "MARKER-draft", "MARKER-intro"] {
        assert!(
            kept.iter().any(|m| m == marker),
            "{marker} is gone: {kept:?}"
        );
    }
    if out.applied {
        assert_clean(&s);
    } else {
        assert_eq!(snapshot(&s.0), before, "{:?}", out.report.items());
    }
}

#[test]
fn a_destination_that_is_a_folder_is_refused_before_anything_is_written() {
    let s = fixture("reloc-folder-destination");
    // A folder the library does not list: it holds only a hidden file.
    s.put("volumes/guide/taken.md/.keep", "");
    let before = snapshot(&s.0);

    let out = go(
        &s,
        "volumes/guide/slugged.md",
        "volumes/guide/taken.md",
        true,
    );

    assert!(
        codes(&out).contains(&Code::RELOCATE_TAKEN),
        "the dry run accepted a folder name: {:?}",
        out.report.items()
    );
    assert_eq!(snapshot(&s.0), before);
}

#[test]
fn a_folder_that_differs_only_by_case_is_refused_even_when_it_holds_only_hidden_files() {
    let s = fixture("reloc-hidden-case");
    s.put("volumes/guide/Docs/.keep", "");
    let before = snapshot(&s.0);

    let out = go(
        &s,
        "volumes/guide/terms.md",
        "volumes/guide/docs/terms.md",
        true,
    );

    assert!(
        codes(&out).contains(&Code::RELOCATE_DESTINATION),
        "{:?}",
        out.report.items()
    );
    assert_eq!(snapshot(&s.0), before);
}

/// A disk that cannot tell `Page.md` from `page.md`, as Windows cannot, on top of the real one.
struct CaseBlind(RealDisk);

impl Disk for CaseBlind {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.0.read(path)
    }

    fn exists(&self, path: &Path) -> bool {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return self.0.exists(path);
        };
        let wanted = name.to_string_lossy().to_lowercase();
        fs::read_dir(parent).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|e| e.file_name().to_string_lossy().to_lowercase() == wanted)
        })
    }

    fn write(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.0.write(path, bytes)
    }

    fn create_new(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.0.create_new(path, bytes)
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        self.0.rename(from, to)
    }

    fn remove_file(&mut self, path: &Path) -> io::Result<()> {
        self.0.remove_file(path)
    }

    fn create_dir(&mut self, path: &Path) -> io::Result<()> {
        self.0.create_dir(path)
    }

    fn remove_dir(&mut self, path: &Path) -> io::Result<()> {
        self.0.remove_dir(path)
    }
}

#[test]
fn a_case_only_rename_with_new_text_goes_through_a_temporary_name() {
    let s = fixture("reloc-case-only");
    s.put(
        "volumes/guide/Page.md",
        "---\nslug: page-stable\n---\n# Page\n\n## Top\n\nSee [top](Page.md#top).\n",
    );

    let out = relocate_on(
        &RelocateOptions {
            root: s.0.clone(),
            from: "volumes/guide/Page.md".to_owned(),
            to: "volumes/guide/page.md".to_owned(),
            dry_run: false,
        },
        &mut CaseBlind(RealDisk),
    );

    assert!(out.applied, "{:?}", out.report.items());
    let names: Vec<String> = fs::read_dir(s.0.join("volumes/guide"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"page.md".to_owned()), "{names:?}");
    assert!(!names.contains(&"Page.md".to_owned()), "{names:?}");
    assert!(read(&s, "volumes/guide/page.md").contains("[top](page.md#top)"));
}

#[test]
fn a_chain_of_moves_that_rewrite_their_text_is_applied_in_full() {
    // `one` goes to `two`'s place and `two` goes on to `three`, both with new text.
    let s = fixture("reloc-text-chain");
    s.put("volumes/guide/one.md", "# One\n");
    s.put("volumes/guide/two.md", "# Two\n");
    let changes = vec![
        Change::Move {
            from: s.0.join("volumes/guide/one.md"),
            to: s.0.join("volumes/guide/two.md"),
            text: Some("# One, moved\n".to_owned()),
        },
        Change::Move {
            from: s.0.join("volumes/guide/two.md"),
            to: s.0.join("volumes/guide/three.md"),
            text: Some("# Two, moved\n".to_owned()),
        },
    ];

    apply(&mut RealDisk, &changes, &[]).unwrap_or_else(|f| panic!("{}", f.message));

    assert_eq!(read(&s, "volumes/guide/two.md"), "# One, moved\n");
    assert_eq!(read(&s, "volumes/guide/three.md"), "# Two, moved\n");
    assert!(!s.0.join("volumes/guide/one.md").exists());
    let stray: Vec<String> = snapshot(&s.0)
        .into_keys()
        .filter(|k| k.contains(".anaquel-"))
        .collect();
    assert!(stray.is_empty(), "{stray:?}");
}

/// Plants a link at `link` that points at `target`. Returns false when the system does not let
/// this user make links, in which case the caller skips the test it belongs to.
fn plant_link(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_file(target, link);
    result.is_ok()
}

/// Whether a test that needs links may skip itself because the system refused one. On the
/// machines of the project's automated checks (they set `CI`) a refusal is a failure, so a skip
/// can never hide a lost test there.
fn links_may_be_missing() -> bool {
    std::env::var_os("CI").is_none()
}

#[test]
fn a_link_at_a_temporary_name_is_never_written_through() {
    // One case per kind of temporary file: the new text of a moved page, a page waiting to be
    // moved, and the new text of a page that only changes its links. Each is the first change,
    // so its temporary file has position 0.
    type Plant = fn(&Scratch) -> (Vec<Change>, &'static str);
    let cases: [(&str, Plant); 3] = [
        ("reloc-link-staged-move", |s| {
            let change = Change::Move {
                from: s.0.join("volumes/guide/setup.md"),
                to: s.0.join("volumes/guide/install.md"),
                text: Some("# Changed\n".to_owned()),
            };
            (vec![change], "volumes/guide/.anaquel-staged-000000")
        }),
        ("reloc-link-held", |s| {
            let change = Change::Move {
                from: s.0.join("volumes/guide/setup.md"),
                to: s.0.join("volumes/guide/install.md"),
                text: None,
            };
            (vec![change], "volumes/guide/.anaquel-held-000000")
        }),
        ("reloc-link-staged-rewrite", |s| {
            let change = Change::Rewrite {
                path: s.0.join("volumes/guide/terms.md"),
                text: "# Changed\n".to_owned(),
            };
            (vec![change], "volumes/guide/.anaquel-staged-000000")
        }),
    ];
    for (name, plant) in cases {
        let s = fixture(name);
        let outside = s.0.join("outside.txt");
        fs::write(&outside, "OUTSIDE").unwrap();
        let (changes, link) = plant(&s);
        if !plant_link(&outside, &s.0.join(link)) {
            assert!(links_may_be_missing(), "{name}: the system refused a link");
            return;
        }
        let before = snapshot(&s.0);

        let failure = apply(&mut RealDisk, &changes, &[]).err();

        let failure = failure.unwrap_or_else(|| panic!("{name}: the move went through a link"));
        assert!(failure.restored, "{name}: {}", failure.message);
        assert_eq!(fs::read_to_string(&outside).unwrap(), "OUTSIDE", "{name}");
        assert_eq!(snapshot(&s.0), before, "{name}");
    }
}

#[test]
fn a_temporary_name_that_is_taken_stops_the_move_and_keeps_the_file() {
    let s = fixture("reloc-taken-temporary-name");
    s.put("volumes/guide/.anaquel-held-000000", "someone else's file");
    let before = snapshot(&s.0);
    let changes = vec![Change::Move {
        from: s.0.join("volumes/guide/setup.md"),
        to: s.0.join("volumes/guide/install.md"),
        text: None,
    }];

    let failure = apply(&mut RealDisk, &changes, &[]).err().unwrap();

    assert!(failure.restored);
    assert!(
        failure.message.contains("already exists"),
        "{}",
        failure.message
    );
    assert_eq!(snapshot(&s.0), before);
}

#[test]
fn a_planted_link_stops_a_real_move_and_leaves_the_library_as_it_was() {
    let s = fixture("reloc-planted-link");
    let outside = s.0.join("outside.txt");
    fs::write(&outside, "OUTSIDE").unwrap();
    // The first move of the plan is `setup.md`, and its new text is staged in this folder.
    if !plant_link(&outside, &s.0.join("volumes/guide/.anaquel-staged-000000")) {
        assert!(links_may_be_missing(), "the system refused a link");
        return;
    }
    let before = snapshot(&s.0);

    let out = go(
        &s,
        "volumes/guide/setup.md",
        "volumes/guide/install.md",
        false,
    );

    assert!(!out.applied);
    assert!(out.report.has_errors());
    assert_eq!(fs::read_to_string(&outside).unwrap(), "OUTSIDE");
    assert_eq!(snapshot(&s.0), before);
}

/// A disk that cannot remove a file held by a move, as a program that has it open cannot on
/// Windows.
struct Stubborn(RealDisk);

impl Disk for Stubborn {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.0.read(path)
    }

    fn exists(&self, path: &Path) -> bool {
        self.0.exists(path)
    }

    fn write(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.0.write(path, bytes)
    }

    fn create_new(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.0.create_new(path, bytes)
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        self.0.rename(from, to)
    }

    fn remove_file(&mut self, path: &Path) -> io::Result<()> {
        let held = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(".anaquel-held-"));
        if held {
            Err(io::Error::other("held open"))
        } else {
            self.0.remove_file(path)
        }
    }

    fn create_dir(&mut self, path: &Path) -> io::Result<()> {
        self.0.create_dir(path)
    }

    fn remove_dir(&mut self, path: &Path) -> io::Result<()> {
        self.0.remove_dir(path)
    }
}

#[test]
fn an_original_that_cannot_be_removed_is_reported_and_the_move_stays_done() {
    let s = fixture("reloc-leftover");
    let changes = vec![Change::Move {
        from: s.0.join("volumes/guide/setup.md"),
        to: s.0.join("volumes/guide/install.md"),
        text: Some("# Changed\n".to_owned()),
    }];

    let leftovers = apply(&mut Stubborn(RealDisk), &changes, &[]).unwrap_or_else(|f| {
        panic!("{}", f.message);
    });

    assert_eq!(
        leftovers,
        vec![s.0.join("volumes/guide/.anaquel-held-000000")]
    );
    assert_eq!(read(&s, "volumes/guide/install.md"), "# Changed\n");
    assert!(!s.0.join("volumes/guide/setup.md").exists());
}

#[test]
fn a_move_with_no_leftovers_reports_none_and_leaves_no_temporary_file() {
    let s = fixture("reloc-no-leftover");
    let changes = vec![
        Change::Move {
            from: s.0.join("volumes/guide/setup.md"),
            to: s.0.join("volumes/guide/install.md"),
            text: Some("# Changed\n".to_owned()),
        },
        Change::Rewrite {
            path: s.0.join("volumes/guide/terms.md"),
            text: "# Terms, edited\n".to_owned(),
        },
    ];

    let leftovers = apply(&mut RealDisk, &changes, &[]).unwrap_or_else(|f| panic!("{}", f.message));

    assert_eq!(leftovers, Vec::<std::path::PathBuf>::new());
    assert_eq!(read(&s, "volumes/guide/terms.md"), "# Terms, edited\n");
    let stray: Vec<String> = snapshot(&s.0)
        .into_keys()
        .filter(|k| k.contains(".anaquel-"))
        .collect();
    assert!(stray.is_empty(), "{stray:?}");
}

#[test]
fn the_plan_names_the_folders_that_are_removed_when_empty_and_a_hidden_file_keeps_one() {
    let s = fixture("reloc-emptied");
    s.put("volumes/guide/ref/.keep", "");

    let planned = go(
        &s,
        "volumes/guide/ref/_index.md",
        "volumes/guide/api/_index.md",
        true,
    );
    assert!(!planned.report.has_errors(), "{:?}", planned.report.items());
    assert!(
        planned
            .plan
            .iter()
            .any(|l| l.contains("removed if the move leaves them empty")
                && l.contains("volumes/guide/ref")),
        "{:?}",
        planned.plan
    );

    let done = go(
        &s,
        "volumes/guide/ref/_index.md",
        "volumes/guide/api/_index.md",
        false,
    );
    assert!(done.applied, "{:?}", done.report.items());
    assert!(s.0.join("volumes/guide/ref/.keep").exists());
    assert!(s.0.join("volumes/guide/api/_index.md").exists());
    assert!(!s.0.join("volumes/guide/ref/_index.md").exists());
}

#[test]
fn a_move_may_hold_only_as_many_files_as_a_temporary_name_can_count() {
    assert!(within_limit(MAX_CHANGES));
    assert!(!within_limit(MAX_CHANGES + 1));
    assert_eq!(MAX_CHANGES.to_string(), "9".repeat(POSITION_DIGITS));
}

fn messages(outcome: &RelocateOutcome) -> String {
    outcome
        .report
        .items()
        .iter()
        .map(|d| d.message.clone())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn the_temporary_name_budget_is_the_length_of_the_longest_temporary_name() {
    // Written out, so the check does not repeat the code it checks.
    let longest = ".anaquel-staged-000000";
    assert_eq!(TEMPORARY_EXTRA, longest.len());
    assert!(".anaquel-held-000000".len() <= TEMPORARY_EXTRA);
}

#[test]
fn a_page_with_a_slug_may_take_any_file_name_and_a_page_without_one_may_not() {
    // The address of a `slug` page does not come from its file name, so a name that cannot be
    // part of an address is fine for it. `setup.md` has no slug, and the same name is refused.
    let s = fixture("reloc-slug-name");

    let slugged = go(
        &s,
        "volumes/guide/slugged.md",
        "volumes/guide/has space.md",
        true,
    );
    let plain = go(
        &s,
        "volumes/guide/terms.md",
        "volumes/guide/has space.md",
        true,
    );

    assert!(!slugged.report.has_errors(), "{:?}", slugged.report.items());
    assert!(
        codes(&plain).contains(&Code::RELOCATE_DESTINATION),
        "{:?}",
        plain.report.items()
    );
}

#[test]
fn a_static_file_may_take_a_name_a_page_may_not() {
    // A static file keeps its own name when its folder moves, so that name is not judged as a
    // page name is.
    let s = fixture("reloc-static-name");
    s.put("volumes/guide/ref/my pic.txt", "pic");

    let out = go(
        &s,
        "volumes/guide/ref/_index.md",
        "volumes/guide/api/_index.md",
        true,
    );

    assert!(!out.report.has_errors(), "{:?}", out.report.items());
    assert!(
        out.plan
            .iter()
            .any(|l| l.contains("my pic.txt") && l.contains("volumes/guide/api/my pic.txt")),
        "{:?}",
        out.plan
    );
}

#[test]
fn a_file_name_may_be_255_bytes_and_not_256() {
    let s = fixture("reloc-segment-length");
    let go_to = |letters: usize| {
        go(
            &s,
            "volumes/guide/slugged.md",
            &format!("volumes/guide/{}.md", "a".repeat(letters)),
            true,
        )
    };

    let fits = messages(&go_to(252));
    let too_long = messages(&go_to(253));

    assert!(!fits.contains("longer than 255 bytes"), "{fits}");
    assert!(too_long.contains("longer than 255 bytes"), "{too_long}");
}

#[test]
fn the_destination_path_may_be_259_utf16_units_with_the_temporary_name_and_not_260() {
    let s = fixture("reloc-path-length");
    let absolute = std::path::absolute(&s.0).unwrap();
    let probe = absolute.join("volumes/guide/a.md");
    let probe_units = probe.to_string_lossy().encode_utf16().count();
    // A name of `letters` letters and `.md` makes the whole path `probe_units + letters - 1` long.
    let letters_for_limit = 259 - TEMPORARY_EXTRA + 1 - probe_units;
    assert!(
        (1..=252).contains(&letters_for_limit),
        "{letters_for_limit}"
    );
    let go_to = |letters: usize| {
        go(
            &s,
            "volumes/guide/slugged.md",
            &format!("volumes/guide/{}.md", "a".repeat(letters)),
            true,
        )
    };

    let fits = messages(&go_to(letters_for_limit));
    let too_long = messages(&go_to(letters_for_limit + 1));

    assert!(!fits.contains("Windows stops at"), "{fits}");
    assert!(too_long.contains("Windows stops at"), "{too_long}");
}

/// As `plant_link`, for a folder.
fn plant_folder_link(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir(target, link);
    result.is_ok()
}

#[test]
fn a_destination_inside_a_linked_folder_is_refused() {
    let s = fixture("reloc-linked-folder");
    let elsewhere = s.0.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    if !plant_folder_link(&elsewhere, &s.0.join("volumes/guide/linked")) {
        assert!(links_may_be_missing(), "the system refused a link");
        return;
    }
    let before = snapshot(&s.0);

    let out = go(
        &s,
        "volumes/guide/slugged.md",
        "volumes/guide/linked/slugged.md",
        true,
    );

    assert!(
        codes(&out).contains(&Code::RELOCATE_DESTINATION),
        "{:?}",
        out.report.items()
    );
    assert!(
        messages(&out).contains("symbolic link"),
        "{}",
        messages(&out)
    );
    assert_eq!(snapshot(&s.0), before);
}

#[test]
fn a_name_that_differs_from_an_existing_file_only_by_case_is_refused() {
    let s = fixture("reloc-case-file");
    let before = snapshot(&s.0);

    let out = go(
        &s,
        "volumes/guide/slugged.md",
        "volumes/guide/TERMS.md",
        true,
    );

    assert!(
        codes(&out).contains(&Code::RELOCATE_DESTINATION),
        "{:?}",
        out.report.items()
    );
    assert!(messages(&out).contains("letter case"), "{}", messages(&out));
    assert_eq!(snapshot(&s.0), before);
}

#[test]
fn a_file_in_the_way_of_a_folder_is_refused() {
    let s = fixture("reloc-file-as-folder");
    let before = snapshot(&s.0);

    let out = go(
        &s,
        "volumes/guide/slugged.md",
        "volumes/guide/terms.md/slugged.md",
        true,
    );

    assert!(
        codes(&out).contains(&Code::RELOCATE_TAKEN),
        "{:?}",
        out.report.items()
    );
    assert!(
        messages(&out).contains("so it cannot also be a folder"),
        "{}",
        messages(&out)
    );
    assert_eq!(snapshot(&s.0), before);
}
