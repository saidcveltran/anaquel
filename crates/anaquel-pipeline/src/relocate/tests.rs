use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::Path;

use anaquel_diagnostics::Code;

use super::disk::{Disk, RealDisk};
use super::text::{encode, read_destination, relative, set_aliases};
use super::{Address, RelocateOptions, RelocateOutcome, check_addresses, relocate, relocate_on};
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
    assert!(
        links.contains("[a]: ../api.md#api \"Title\"")
            || links.contains("[a]: api.md#api \"Title\""),
        "{links}"
    );
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
fn only_the_case_of_a_name_can_change() {
    let s = fixture("reloc-case");
    let out = go(
        &s,
        "volumes/guide/setup.md",
        "volumes/guide/Setup.md",
        false,
    );
    assert!(out.applied, "{:?}", out.report.items());
    assert!(read(&s, "volumes/guide/Setup.md").contains("  - /guide/setup/\n"));
    assert_clean(&s);
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
    fail_at: usize,
    count: usize,
}

impl Failing {
    fn tick(&mut self) -> io::Result<()> {
        self.count += 1;
        if self.count == self.fail_at {
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

    fn write(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.tick()?;
        self.inner.write(path, bytes)
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

#[test]
fn an_interrupted_write_restores_every_file() {
    let mut failures = 0;
    for fail_at in 1..60 {
        let s = fixture("reloc-interrupted");
        let before = snapshot(&s.0);
        let mut disk = Failing {
            inner: RealDisk,
            fail_at,
            count: 0,
        };
        let out = relocate_on(
            &RelocateOptions {
                root: s.0.clone(),
                from: "volumes/guide/setup.md".to_owned(),
                to: "volumes/guide/new/setup.md".to_owned(),
                dry_run: false,
            },
            &mut disk,
        );
        if out.applied {
            assert_clean(&s);
            break;
        }
        failures += 1;
        assert!(out.report.has_errors());
        assert_eq!(snapshot(&s.0), before, "failing at step {fail_at}");
    }
    assert!(failures >= 5, "{failures}");
}

#[test]
fn reserved_and_plain_names_are_told_apart() {
    for bad in ["con", "CON", "nul.txt", "Aux.md", "com1", "LPT9.md", "prn "] {
        assert!(super::windows_device(bad), "{bad}");
    }
    for fine in ["console", "setup", "com10", "com", "lpt", "a.con", "comma"] {
        assert!(!super::windows_device(fine), "{fine}");
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
