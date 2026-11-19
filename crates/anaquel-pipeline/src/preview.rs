//! A preview of one page with text that is not saved yet, for the editor of `anaquel studio`.
//!
//! The preview is the build with one page replaced: the library is read from disk, the text of
//! the page is swapped for the text the editor holds, the catalog is resolved as `build` resolves
//! it, and the one page is composed and rendered with the same theme and the same templates. A
//! page that is saved by hand with that text and then built gives the same HTML, byte for byte;
//! a test builds both and compares.
//!
//! Only the page is composed, not the library, so the diagnostics of the page come from the
//! catalog (front matter, links to files that are missing) and from the composition of that page
//! (directives, raw HTML, links to terms and anchors). The problems of other pages are returned
//! apart, so the editor can say that the library has errors without blaming this page.

use std::path::Path;

use anaquel_catalog::{Catalog, LogicalPath, PageSource, VolumeSource};
use anaquel_compose::{compose, headings_of};
use anaquel_diagnostics::{Code, Diagnostic, Report, Severity};
use anaquel_render::Renderer;
use anaquel_theme::Theme;

use crate::{ENGINE_VERSION, fingerprint_theme_assets, ingest, views};

/// What the editor shows beside the text of a page.
#[derive(Debug)]
pub struct PagePreview {
    /// The problems that name this page, sorted. When one is an error there is no HTML.
    pub page: Vec<Diagnostic>,
    /// The problems of the rest of the library, or of the library itself, sorted. They are the
    /// ones found while reading the library and resolving its catalog (manifests, front matter,
    /// duplicate addresses); the links and directives of the other pages are not checked here,
    /// because only this page is composed. `check` does that.
    pub elsewhere: Vec<Diagnostic>,
    /// The page as the site would serve it, or `None` when the page cannot be built.
    pub html: Option<String>,
    /// The address of the page in the site, once the catalog placed it.
    pub url: Option<String>,
    /// Whether the front matter marks the page as a draft. A draft is rendered for the editor,
    /// but the build does not emit it.
    pub draft: bool,
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
/// folder.
#[must_use]
pub fn preview_page(root: &Path, page: &str, text: &str) -> PagePreview {
    let mut preview = PagePreview {
        page: Vec::new(),
        elsewhere: Vec::new(),
        html: None,
        url: None,
        draft: false,
    };
    let mut report = Report::new();
    let page = match LogicalPath::new(page) {
        Ok(page) => page,
        Err(reason) => {
            report.push(Diagnostic::error(Code::IO, format!("`{page}`: {reason}")).at(page));
            return finish(preview, report, page);
        }
    };
    let Some(mut ingested) = ingest::ingest(root, ENGINE_VERSION, &mut report) else {
        return finish(preview, report, page.as_str());
    };
    if let Err(diagnostic) = put_text(&mut ingested.volumes, &page, text) {
        report.push(diagnostic);
        return finish(preview, report, page.as_str());
    }
    let (catalog, catalog_report) = Catalog::build(
        ingested.library,
        ingested.shelves,
        ingested.volumes,
        &headings_of,
    );
    report.extend(catalog_report);

    // The catalog leaves out a page it cannot use; its problems are already in the report.
    if let Some(entry) = catalog.entries().iter().find(|e| e.source == page) {
        preview.url = Some(entry.url.clone());
        preview.draft = entry.draft;
        let mut composed = compose(&catalog, entry);
        report.extend(std::mem::take(&mut composed.report));
        let page_has_errors = report
            .items()
            .iter()
            .any(|d| d.severity == Severity::Error && d.path.as_deref() == Some(page.as_str()));
        if !page_has_errors {
            let html = Theme::folio().and_then(|theme| {
                let renderer = Renderer::new(&theme)?;
                let (_, asset_urls) = fingerprint_theme_assets(&theme);
                let site = views::site_view(&catalog, false);
                let view =
                    views::page_view(&catalog, entry, composed.html.clone(), &composed.plain);
                renderer.render("page.html", &site, &view, &asset_urls)
            });
            match html {
                Ok(html) => preview.html = Some(html),
                Err(diagnostic) => report.push(diagnostic.at(page.as_str())),
            }
        }
    }
    finish(preview, report, page.as_str())
}

/// Sorts the report into what names the page and what does not.
fn finish(mut preview: PagePreview, mut report: Report, page: &str) -> PagePreview {
    report.sort();
    for diagnostic in report.items() {
        if diagnostic.path.as_deref() == Some(page) {
            preview.page.push(diagnostic.clone());
        } else {
            preview.elsewhere.push(diagnostic.clone());
        }
    }
    preview
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
        let inside = LogicalPath::new(inside).map_err(|reason| {
            Diagnostic::error(Code::IO, format!("`{}`: {reason}", page.as_str())).at(page.as_str())
        })?;
        // The build reads Markdown files only, and skips every name that starts with a dot.
        if !inside.as_str().to_ascii_lowercase().ends_with(".md")
            || inside.as_str().split('/').any(|name| name.starts_with('.'))
        {
            return Err(Diagnostic::error(
                Code::IO,
                format!("`{}` is not a page the build reads", page.as_str()),
            )
            .at(page.as_str())
            .with_help("a page is a `.md` file whose folders and name do not start with a dot"));
        }
        match volume.pages.iter_mut().find(|p| p.path == inside) {
            Some(existing) => existing.text = text,
            None => volume.pages.push(PageSource { path: inside, text }),
        }
        return Ok(());
    }
    Err(Diagnostic::error(
        Code::IO,
        format!(
            "`{}` is not inside a volume folder of the library",
            page.as_str()
        ),
    )
    .at(page.as_str())
    .with_help("a page lives in `volumes/NAME/` or in `shelves/SHELF/volumes/NAME/`"))
}

#[cfg(test)]
mod tests;
