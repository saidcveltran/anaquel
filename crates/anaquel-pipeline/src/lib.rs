//! Build phases of a library: `ingest → catalog → compose → render → emit → index`.
//!
//! [`check`] runs the first three phases and reports every problem without
//! writing anything. [`build`] runs all of them and writes the site.

mod ingest;
mod preview;
mod relocate;
mod views;

pub use preview::{PagePreview, ThemeAsset, preview_page, theme_assets};
pub use relocate::{RelocateOptions, RelocateOutcome, relocate};

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anaquel_assets::fingerprinted_name;
use anaquel_catalog::{Catalog, Entry, EntryId};
use anaquel_compose::{Composed, compose, headings_of};
use anaquel_diagnostics::{Code, Diagnostic, Report};
use anaquel_emit::{
    Ledger, OutputFile, WriteStats, headers_file, llms_txt, lost_urls, redirects_file, sitemap,
    write_ledger, write_outputs,
};
use anaquel_render::{Renderer, SiteView, ui_for};
use anaquel_search::{ExternalPagefind, SearchIndexer};
use anaquel_theme::Theme;
use rayon::prelude::*;

/// Version of the engine, compared with `engine` in `anaquel.toml`.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Cloudflare accepts this many static redirects; beyond it only the HTML stubs remain.
const MAX_STATIC_REDIRECTS: usize = 2000;

/// What a run did.
#[derive(Debug)]
pub struct Outcome {
    /// Everything worth telling the author, sorted.
    pub report: Report,
    /// Published pages (drafts excluded).
    pub pages: usize,
    /// File counts, when the site was written.
    pub written: Option<WriteStats>,
    /// Whether a search index was built.
    pub searched: bool,
}

/// How to run a build.
#[derive(Debug, Clone)]
pub struct Options {
    /// The library folder, the one with `anaquel.toml`.
    pub root: PathBuf,
    /// Where to write the site.
    pub dist: PathBuf,
    /// Index the site for search when the `pagefind` program is available.
    pub search: bool,
}

struct Analysis {
    catalog: Catalog,
    composed: BTreeMap<EntryId, Composed>,
}

fn analyse(root: &Path, report: &mut Report) -> Option<Analysis> {
    let ingested = ingest::ingest(root, ENGINE_VERSION, report)?;
    Some(analyse_sources(ingested, report))
}

fn analyse_sources(ingested: ingest::Ingested, report: &mut Report) -> Analysis {
    let (catalog, catalog_report) = Catalog::build(
        ingested.library,
        ingested.shelves,
        ingested.volumes,
        &headings_of,
    );
    report.extend(catalog_report);

    let published: Vec<&Entry> = catalog.entries().iter().filter(|e| !e.draft).collect();
    let results: Vec<(EntryId, Composed)> = published
        .par_iter()
        .map(|entry| (entry.id, compose(&catalog, entry)))
        .collect();
    let mut composed = BTreeMap::new();
    for (id, mut page) in results {
        report.extend(std::mem::take(&mut page.report));
        composed.insert(id, page);
    }
    Analysis { catalog, composed }
}

/// Reads and checks a library without writing anything.
pub fn check(root: &Path) -> Outcome {
    let mut report = Report::new();
    let pages = analyse(root, &mut report).map_or(0, |a| a.composed.len());
    report.sort();
    Outcome {
        report,
        pages,
        written: None,
        searched: false,
    }
}

/// Builds the site.
///
/// Nothing is written when the library has errors: a half-built site is worse
/// than the previous complete one.
pub fn build(options: &Options) -> Outcome {
    let mut report = Report::new();
    let Some(analysis) = analyse(&options.root, &mut report) else {
        report.sort();
        return Outcome {
            report,
            pages: 0,
            written: None,
            searched: false,
        };
    };
    let pages = analysis.composed.len();
    let mut outcome = Outcome {
        report,
        pages,
        written: None,
        searched: false,
    };

    let previous = Ledger::load(&options.dist);
    let urls: BTreeSet<String> = analysis
        .composed
        .keys()
        .filter_map(|id| analysis.catalog.entry(*id))
        .map(|e| e.url.clone())
        .collect();
    let redirects: BTreeSet<String> = analysis
        .catalog
        .redirects()
        .iter()
        .map(|(from, _)| from.clone())
        .collect();
    if let Some(previous) = &previous {
        outcome
            .report
            .extend(lost_urls(previous, &urls, &redirects));
    }
    if outcome.report.has_errors() {
        outcome.report.sort();
        return outcome;
    }

    let indexer = if options.search {
        ExternalPagefind::locate()
    } else {
        None
    };
    if options.search && indexer.is_none() {
        outcome.report.push(
            Diagnostic::info(
                Code::SEARCH,
                "the site is built without search: the `pagefind` program was not found",
            )
            .with_help("install pagefind next to `anaquel` or on PATH; `anaquel doctor` explains"),
        );
    }

    let outputs = match render_site(&options.root, &analysis, indexer.is_some()) {
        Ok(outputs) => outputs,
        Err(diagnostic) => {
            outcome.report.push(diagnostic);
            outcome.report.sort();
            return outcome;
        }
    };

    match write_outputs(&options.dist, &outputs, previous.as_ref()) {
        Ok((mut ledger, stats)) => {
            ledger.urls = urls;
            ledger.redirects = redirects;
            if let Err(diagnostic) = write_ledger(&options.dist, &ledger) {
                outcome.report.push(diagnostic);
            }
            outcome.written = Some(stats);
        }
        Err(diagnostic) => outcome.report.push(diagnostic),
    }

    if let (Some(indexer), Some(_)) = (indexer, outcome.written) {
        let bundle = options.dist.join(anaquel_search::BUNDLE_DIR);
        let _ = std::fs::remove_dir_all(&bundle);
        match indexer.index(&options.dist) {
            Ok(()) => outcome.searched = true,
            Err(diagnostic) => outcome.report.push(diagnostic),
        }
    }
    outcome.report.sort();
    outcome
}

/// The assets of a theme under their fingerprinted names, and the URL each one is served at,
/// keyed by its path in the theme.
fn fingerprint_theme_assets(theme: &Theme) -> (Vec<OutputFile>, BTreeMap<String, String>) {
    let mut asset_urls: BTreeMap<String, String> = BTreeMap::new();
    let mut outputs = Vec::new();
    // Stylesheets last: they may name other assets as `asset:<path>`, which
    // becomes the fingerprinted URL before the stylesheet is fingerprinted itself.
    let (styles, others): (Vec<_>, Vec<_>) = theme.assets.iter().partition(|a| is_css(&a.path));
    for asset in others.into_iter().chain(styles) {
        let bytes = if is_css(&asset.path) {
            let mut text = String::from_utf8_lossy(&asset.bytes).into_owned();
            for (path, url) in &asset_urls {
                text = text.replace(&format!("asset:{path}"), url);
            }
            text.into_bytes()
        } else {
            asset.bytes.clone()
        };
        let name = format!("assets/{}", fingerprinted_name(&asset.path, &bytes));
        asset_urls.insert(asset.path.clone(), format!("/{name}"));
        outputs.push(OutputFile { path: name, bytes });
    }
    (outputs, asset_urls)
}

fn render_site(
    root: &Path,
    analysis: &Analysis,
    search: bool,
) -> Result<Vec<OutputFile>, Diagnostic> {
    let catalog = &analysis.catalog;
    let theme = Theme::folio()?;
    let renderer = Renderer::new(&theme)?;
    let site = views::site_view(catalog, search);

    let (mut outputs, asset_urls) = fingerprint_theme_assets(&theme);

    let pages: Result<Vec<OutputFile>, Diagnostic> = analysis
        .composed
        .par_iter()
        .map(|(id, composed)| {
            let entry = catalog.entry(*id).ok_or_else(|| {
                Diagnostic::error(Code::TEMPLATE, "entry vanished during the build")
            })?;
            let view = views::page_view(catalog, entry, composed.html.clone(), &composed.plain);
            let html = renderer.render("page.html", &site, &view, &asset_urls)?;
            Ok(OutputFile {
                path: url_to_file(&entry.url),
                bytes: html.into_bytes(),
            })
        })
        .collect();
    outputs.extend(pages?);

    let home = views::blank_page("home", &site.title, "/", &site.language);
    outputs.push(OutputFile {
        path: "index.html".to_owned(),
        bytes: renderer
            .render("home.html", &site, &home, &asset_urls)?
            .into_bytes(),
    });
    let missing = views::blank_page(
        "not_found",
        ui_for(&site.language).not_found,
        "/404.html",
        &site.language,
    );
    outputs.push(OutputFile {
        path: "404.html".to_owned(),
        bytes: renderer
            .render("not_found.html", &site, &missing, &asset_urls)?
            .into_bytes(),
    });

    for (library_path, url) in catalog.static_files() {
        let bytes = std::fs::read(root.join(library_path))
            .map_err(|e| Diagnostic::error(Code::IO, format!("{library_path}: {e}")))?;
        outputs.push(OutputFile {
            path: url.trim_start_matches('/').to_owned(),
            bytes,
        });
    }

    let redirects = catalog.redirects();
    for (from, to) in redirects {
        outputs.push(OutputFile {
            path: url_to_file(from),
            bytes: redirect_page(&site.language, to).into_bytes(),
        });
    }
    if !redirects.is_empty() && redirects.len() <= MAX_STATIC_REDIRECTS {
        outputs.push(OutputFile {
            path: "_redirects".to_owned(),
            bytes: redirects_file(redirects).into_bytes(),
        });
    }
    site_files(analysis, &site, search, &mut outputs);
    outputs.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(outputs)
}

/// `_headers`, `llms.txt` and, with a base URL, `sitemap.xml`.
fn site_files(analysis: &Analysis, site: &SiteView, search: bool, outputs: &mut Vec<OutputFile>) {
    let catalog = &analysis.catalog;
    let mut frame_origins: Vec<String> = analysis
        .composed
        .values()
        .flat_map(|page| page.embed_origins.iter().cloned())
        .collect();
    frame_origins.sort();
    frame_origins.dedup();
    outputs.push(OutputFile {
        path: "_headers".to_owned(),
        bytes: headers_file(search, &frame_origins).into_bytes(),
    });
    let sections = views::llms_sections(catalog, |id| analysis.composed.contains_key(&id));
    outputs.push(OutputFile {
        path: "llms.txt".to_owned(),
        bytes: llms_txt(
            &site.title,
            site.description.as_deref(),
            site.base_url.as_deref(),
            &sections,
        )
        .into_bytes(),
    });
    if let Some(base) = &site.base_url {
        let mut urls: Vec<String> = analysis
            .composed
            .keys()
            .filter_map(|id| catalog.entry(*id))
            .map(|e| e.url.clone())
            .collect();
        urls.insert(0, "/".to_owned());
        outputs.push(OutputFile {
            path: "sitemap.xml".to_owned(),
            bytes: sitemap(base, &urls).into_bytes(),
        });
    }
}

fn is_css(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("css"))
}

/// `/a/b/` is served from `a/b/index.html`; a URL with a file name is the file.
fn url_to_file(url: &str) -> String {
    let path = url.trim_start_matches('/');
    if path.is_empty() || url.ends_with('/') {
        format!("{path}index.html")
    } else {
        path.to_owned()
    }
}

fn redirect_page(language: &str, target: &str) -> String {
    let escaped = target
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;");
    format!(
        "<!doctype html>\n<html lang=\"{language}\">\n<head>\n<meta charset=\"utf-8\">\n<title>Moved</title>\n\
         <link rel=\"canonical\" href=\"{escaped}\">\n<meta http-equiv=\"refresh\" content=\"0; url={escaped}\">\n\
         <meta name=\"robots\" content=\"noindex\">\n</head>\n<body>\n<p>This page moved to <a href=\"{escaped}\">{escaped}</a>.</p>\n</body>\n</html>\n"
    )
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod xss;
