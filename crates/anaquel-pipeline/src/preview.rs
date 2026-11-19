//! A preview of one page with text that is not saved yet, for the editor of `anaquel studio`.
//!
//! The preview is the build with one page replaced: the library is read from disk, the text of
//! the page is swapped for the text the editor holds, the catalog is resolved as `build` resolves
//! it, and the one page is composed and rendered with the same theme and the same templates. A
//! page that is saved by hand with that text and then built (with search off, as the preview
//! renders it) gives the same HTML, byte for byte; a test builds both and compares.
//!
//! Only the page is composed, not the library, so the problems of the page come from the catalog
//! (front matter, addresses, terms) and from the composition of that page (links, directives, raw
//! HTML). A problem that the catalog finds in another page but that only exists because of this
//! text (two pages that now share an address) belongs to the page too: the catalog is resolved a
//! second time with the text on disk, and what the new text adds is the page's. The problems that
//! were already there are returned apart, so the editor can say that the library has errors
//! without blaming this page.
//!
//! The drafts differ from the build on purpose: a draft is not composed by `build`, and is here,
//! so that the editor can show its problems while it is written.

use std::path::Path;

use anaquel_catalog::{Catalog, LogicalPath, PageSource, VolumeSource};
use anaquel_compose::{compose, headings_of};
use anaquel_diagnostics::{Code, Diagnostic, Report, Severity};
use anaquel_render::Renderer;
use anaquel_theme::Theme;

use crate::{ENGINE_VERSION, MAX_PAGE_BYTES, fingerprint_theme_assets, ingest, views};

/// What the editor shows beside the text of a page.
#[derive(Debug)]
pub struct PagePreview {
    /// The problems of this page, sorted: those that name it and those that only exist because of
    /// its text. When one is an error there is no HTML.
    pub page: Vec<Diagnostic>,
    /// The problems of the rest of the library, or of the library itself, that are there with or
    /// without this text, sorted. They are the ones found while reading the library and resolving
    /// its catalog (manifests, front matter, duplicate addresses); the links and directives of the
    /// other pages are not checked here, because only this page is composed. `check` does that.
    /// Paths on disk in the messages are replaced by `<library>`.
    pub elsewhere: Vec<Diagnostic>,
    /// The page as the site would serve it, or `None` when the page cannot be built.
    pub html: Option<String>,
    /// The address of the page in the site, once the catalog placed it.
    pub url: Option<String>,
    /// Whether the front matter marks the page as a draft. A draft is rendered for the editor,
    /// but the build does not emit it.
    pub draft: bool,
    /// The origins the page embeds frames from, which the site's `Content-Security-Policy` has to
    /// allow; the server that shows the HTML needs them to answer as the site does.
    pub frame_origins: Vec<String>,
}

/// A file of the theme as the site serves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeAsset {
    /// The path in the site, without a leading slash, e.g. `assets/folio.3f2a9c1b.css`.
    pub path: String,
    /// The content.
    pub bytes: Vec<u8>,
}

/// The files the HTML of a preview refers to: the stylesheets, scripts and fonts of the theme,
/// under the fingerprinted names that [`preview_page`] writes into the HTML.
///
/// # Errors
/// Returns the diagnostic of the theme when it cannot be loaded.
pub fn theme_assets() -> Result<Vec<ThemeAsset>, Diagnostic> {
    let theme = Theme::folio()?;
    let (files, _) = fingerprint_theme_assets(&theme);
    Ok(files
        .into_iter()
        .map(|file| ThemeAsset {
            path: file.path,
            bytes: file.bytes,
        })
        .collect())
}

/// Previews the page at `page`, a path from the library folder such as `volumes/guide/setup.md`,
/// as if its text were `text`. Nothing is written.
///
/// The page may be one that is not on disk yet, as long as it names a file inside a volume
/// folder. A text longer than the build reads ([`MAX_PAGE_BYTES`]) is refused with a diagnostic
/// before anything is parsed.
#[must_use]
pub fn preview_page(root: &Path, page: &str, text: &str) -> PagePreview {
    let mut preview = PagePreview {
        page: Vec::new(),
        elsewhere: Vec::new(),
        html: None,
        url: None,
        draft: false,
        frame_origins: Vec::new(),
    };
    let page = match LogicalPath::new(page) {
        Ok(page) => page,
        Err(reason) => {
            preview
                .page
                .push(refusal(page, &format!("`{page}`: {reason}")));
            return preview;
        }
    };
    if u64::try_from(text.len()).map_or(true, |len| len > MAX_PAGE_BYTES) {
        preview.page.push(
            refusal(page.as_str(), "the text is larger than the build reads")
                .with_help(format!("a page may hold at most {MAX_PAGE_BYTES} bytes")),
        );
        return preview;
    }

    let mut ingest_report = Report::new();
    let Some(mut ingested) = ingest::ingest(root, ENGINE_VERSION, &mut ingest_report) else {
        preview.elsewhere = prepared(ingest_report.items().to_vec(), root);
        return preview;
    };
    // What ingest says about the file at this path is about the copy on disk, which the text of
    // the editor replaces (a file that is too large, or is not text, is fine once replaced). The
    // rules about the name are checked again below, on the name itself.
    let mut elsewhere: Vec<Diagnostic> = ingest_report
        .items()
        .iter()
        .filter(|d| !names(d, &page))
        .cloned()
        .collect();

    // The catalog with the page as it is on disk, to tell what the new text adds.
    let (_, baseline) = Catalog::build(
        ingested.library.clone(),
        ingested.shelves.clone(),
        ingested.volumes.clone(),
        &headings_of,
    );
    if let Err(diagnostic) = put_text(&mut ingested.volumes, &page, text) {
        preview.page.push(diagnostic);
        preview.elsewhere = prepared(elsewhere, root);
        return preview;
    }
    let (catalog, catalog_report) = Catalog::build(
        ingested.library,
        ingested.shelves,
        ingested.volumes,
        &headings_of,
    );
    let mut own = Vec::new();
    for diagnostic in catalog_report.items() {
        if names(diagnostic, &page) || !baseline.items().contains(diagnostic) {
            own.push(diagnostic.clone());
        } else {
            elsewhere.push(diagnostic.clone());
        }
    }

    // The catalog leaves out a page it cannot use; its problems are already in `own`.
    if let Some(entry) = catalog.entries().iter().find(|e| e.source == page) {
        preview.url = Some(entry.url.clone());
        preview.draft = entry.draft;
        let mut composed = compose(&catalog, entry);
        own.extend(std::mem::take(&mut composed.report).items().iter().cloned());
        if !own.iter().any(|d| d.severity == Severity::Error) {
            let html = Theme::folio().and_then(|theme| {
                let renderer = Renderer::new(&theme)?;
                let (_, asset_urls) = fingerprint_theme_assets(&theme);
                let site = views::site_view(&catalog, false);
                let view = views::page_view(
                    &catalog,
                    entry,
                    std::mem::take(&mut composed.html),
                    &composed.plain,
                );
                renderer.render("page.html", &site, &view, &asset_urls)
            });
            match html {
                Ok(html) => preview.html = Some(html),
                Err(diagnostic) => own.push(diagnostic.at(page.as_str())),
            }
        }
        preview.frame_origins = std::mem::take(&mut composed.embed_origins);
        preview.frame_origins.sort();
        preview.frame_origins.dedup();
    }
    preview.page = sorted(own);
    preview.elsewhere = prepared(elsewhere, root);
    preview
}

/// An error about the request itself, at the page.
fn refusal(page: &str, message: &str) -> Diagnostic {
    Diagnostic::error(Code::IO, message).at(page)
}

/// Whether a diagnostic names the page. Ingest writes paths as the disk spells them, so both sides
/// are compared in Unicode normal form C, which is how the build keeps every path.
fn names(diagnostic: &Diagnostic, page: &LogicalPath) -> bool {
    diagnostic
        .path
        .as_deref()
        .and_then(|path| LogicalPath::new(path).ok())
        .is_some_and(|path| path == *page)
}

fn sorted(diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    let mut report = Report::new();
    for diagnostic in diagnostics {
        report.push(diagnostic);
    }
    report.sort();
    report.items().to_vec()
}

/// Sorts the diagnostics of the rest of the library and keeps the folder of the library out of
/// them, since they go to a web page.
fn prepared(diagnostics: Vec<Diagnostic>, root: &Path) -> Vec<Diagnostic> {
    let spelled = root.display().to_string();
    let forward = spelled.replace('\\', "/");
    let hide = |text: &str| {
        text.replace(&spelled, "<library>")
            .replace(&forward, "<library>")
    };
    let hidden = diagnostics
        .into_iter()
        .map(|mut diagnostic| {
            diagnostic.message = hide(&diagnostic.message);
            diagnostic.help = diagnostic.help.as_deref().map(hide);
            diagnostic
        })
        .collect();
    sorted(hidden)
}

/// Puts `text` in the place of the page in its volume, or adds the page to the volume when the
/// volume has no such page yet.
fn put_text(
    volumes: &mut [VolumeSource],
    page: &LogicalPath,
    text: &str,
) -> Result<(), Diagnostic> {
    // The build reads a page without its byte order mark and with `\n` line ends.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let text = text.replace("\r\n", "\n");
    for volume in volumes {
        let Some(inside) = page
            .as_str()
            .strip_prefix(volume.folder().as_str())
            .and_then(|rest| rest.strip_prefix('/'))
        else {
            continue;
        };
        let inside = LogicalPath::new(inside)
            .map_err(|reason| refusal(page.as_str(), &format!("`{}`: {reason}", page.as_str())))?;
        // The build reads Markdown files only, and skips every name that starts with a dot.
        if !inside.as_str().to_ascii_lowercase().ends_with(".md")
            || inside.as_str().split('/').any(|name| name.starts_with('.'))
        {
            return Err(refusal(
                page.as_str(),
                &format!("`{}` is not a page the build reads", page.as_str()),
            )
            .with_help("a page is a `.md` file whose folders and name do not start with a dot"));
        }
        // These would end or change the address of the page, and the build refuses the file.
        if let Some(bad) = inside
            .as_str()
            .chars()
            .find(|c| matches!(c, '#' | '%' | '?'))
        {
            return Err(refusal(
                page.as_str(),
                &format!("`{bad}` in a file or folder name would end or change its address"),
            )
            .with_help("rename it; letters, digits, spaces and `- _ . ( ) , ;` are fine"));
        }
        match volume.pages.iter_mut().find(|p| p.path == inside) {
            Some(existing) => existing.text = text,
            None => volume.pages.push(PageSource { path: inside, text }),
        }
        return Ok(());
    }
    Err(refusal(
        page.as_str(),
        &format!(
            "`{}` is not inside a volume folder of the library",
            page.as_str()
        ),
    )
    .with_help("a page lives in `volumes/NAME/` or in `shelves/SHELF/volumes/NAME/`"))
}

#[cfg(test)]
mod tests;
