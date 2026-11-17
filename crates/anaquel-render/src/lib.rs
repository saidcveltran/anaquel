//! Template rendering of pages.
//!
//! The views in this crate are the data a theme can see: nothing else reaches
//! a template. Templates cannot read files, the network or the environment, an
//! undefined variable is an error, and every value is HTML-escaped unless the
//! template marks it `safe`, which only the engine's own HTML should be.

use std::collections::BTreeMap;

use anaquel_diagnostics::{Code, Diagnostic};
use anaquel_theme::Theme;
use minijinja::value::Serde;
use minijinja::{Environment, UndefinedBehavior, Value};
use serde::Serialize;

/// A link with a title.
#[derive(Debug, Clone, Serialize)]
pub struct LinkView {
    /// Text to show.
    pub title: String,
    /// Site URL.
    pub url: String,
    /// One sentence, when there is one.
    pub description: Option<String>,
}

/// One item of the sidebar tree.
#[derive(Debug, Clone, Serialize)]
pub struct NavNode {
    /// Text to show.
    pub title: String,
    /// Site URL.
    pub url: String,
    /// The page being rendered.
    pub current: bool,
    /// On the path from the volume root to the current page.
    pub active: bool,
    /// Shown below this item; empty for collapsed sections.
    pub children: Vec<NavNode>,
}

/// One item of the table of contents.
#[derive(Debug, Clone, Serialize)]
pub struct TocItem {
    /// 2 or 3.
    pub level: u8,
    /// Heading text.
    pub text: String,
    /// Target anchor, without `#`.
    pub anchor: String,
}

/// What a template may know about the library.
#[derive(Debug, Clone, Serialize)]
pub struct SiteView {
    /// Library title.
    pub title: String,
    /// Library description.
    pub description: Option<String>,
    /// Content language tag.
    pub language: String,
    /// Absolute base URL, without trailing slash.
    pub base_url: Option<String>,
    /// Volumes in display order.
    pub volumes: Vec<LinkView>,
    /// Whether a search index is part of the output.
    pub search: bool,
    /// Where the search bundle lives, with a trailing slash.
    pub search_path: String,
}

/// What a template may know about the page being rendered.
#[derive(Debug, Clone, Serialize)]
pub struct PageView {
    /// `page`, `section`, `home` or `not_found`.
    pub kind: &'static str,
    /// Page title.
    pub title: String,
    /// Page description.
    pub description: Option<String>,
    /// Site URL of the page.
    pub url: String,
    /// HTML of the body, produced by the engine.
    pub content: String,
    /// Headings of levels 2 and 3.
    pub toc: Vec<TocItem>,
    /// Ancestors, from the volume root down to the parent.
    pub breadcrumbs: Vec<LinkView>,
    /// The volume the page belongs to.
    pub volume: LinkView,
    /// Sidebar tree.
    pub nav: Vec<NavNode>,
    /// Entries listed under a section.
    pub children: Vec<LinkView>,
    /// Previous page in reading order.
    pub prev: Option<LinkView>,
    /// Next page in reading order.
    pub next: Option<LinkView>,
}

#[derive(Serialize)]
struct Context<'a> {
    site: &'a SiteView,
    page: &'a PageView,
    assets: &'a BTreeMap<String, String>,
}

/// Renders pages with the templates of one theme.
pub struct Renderer {
    env: Environment<'static>,
}

impl Renderer {
    /// Compiles every template of `theme`.
    ///
    /// # Errors
    /// Returns a diagnostic when a template has a syntax error.
    pub fn new(theme: &Theme) -> Result<Self, Diagnostic> {
        let mut env = Environment::new();
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        for template in &theme.templates {
            env.add_template_owned(template.name.clone(), template.source.clone())
                .map_err(|e| {
                    Diagnostic::error(Code::TEMPLATE, format!("template `{}`: {e}", template.name))
                })?;
        }
        Ok(Self { env })
    }

    /// Renders `template` for a page.
    ///
    /// `assets` maps a theme asset path to its public URL.
    ///
    /// # Errors
    /// Returns a diagnostic when the template is missing or fails, for
    /// example by using an undefined variable.
    pub fn render(
        &self,
        template: &str,
        site: &SiteView,
        page: &PageView,
        assets: &BTreeMap<String, String>,
    ) -> Result<String, Diagnostic> {
        let compiled = self.env.get_template(template).map_err(|e| {
            Diagnostic::error(Code::TEMPLATE, format!("template `{template}`: {e}"))
        })?;
        let html = compiled
            .render(Value::from(Serde(Context { site, page, assets })))
            .map_err(|e| {
                Diagnostic::error(Code::TEMPLATE, format!("rendering `{template}`: {e:#}"))
            })?;
        // The escaper writes `/` as `&#x2f;`. A slash is harmless in HTML, and the
        // entity would make every URL of every page six bytes longer per slash.
        // Literal text `&#x2f;` from an author is escaped to `&amp;#x2f;`, which
        // does not contain this sequence, so nothing else is touched.
        Ok(html.replace("&#x2f;", "/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> SiteView {
        SiteView {
            title: "Lib <&>".to_owned(),
            description: None,
            language: "en".to_owned(),
            base_url: Some("https://docs.example".to_owned()),
            volumes: vec![LinkView {
                title: "Guide".to_owned(),
                url: "/guide/".to_owned(),
                description: None,
            }],
            search: false,
            search_path: "/_pagefind/".to_owned(),
        }
    }

    fn page() -> PageView {
        let guide = LinkView {
            title: "Guide".to_owned(),
            url: "/guide/".to_owned(),
            description: None,
        };
        PageView {
            kind: "page",
            title: "Setup <script>".to_owned(),
            description: None,
            url: "/guide/setup/".to_owned(),
            content: "<p>body</p>".to_owned(),
            toc: vec![TocItem {
                level: 2,
                text: "Part".to_owned(),
                anchor: "part".to_owned(),
            }],
            breadcrumbs: vec![guide.clone()],
            volume: guide,
            nav: vec![NavNode {
                title: "Setup".to_owned(),
                url: "/guide/setup/".to_owned(),
                current: true,
                active: true,
                children: vec![NavNode {
                    title: "Child".to_owned(),
                    url: "/guide/setup/child/".to_owned(),
                    current: false,
                    active: false,
                    children: vec![],
                }],
            }],
            children: vec![],
            prev: None,
            next: None,
        }
    }

    fn assets() -> BTreeMap<String, String> {
        Theme::folio()
            .unwrap()
            .assets
            .iter()
            .map(|a| (a.path.clone(), format!("/assets/{}", a.path)))
            .collect()
    }

    #[test]
    fn page_template_renders_and_escapes() {
        let r = Renderer::new(&Theme::folio().unwrap()).unwrap();
        let html = r.render("page.html", &site(), &page(), &assets()).unwrap();
        assert!(
            html.contains("<title>Setup &lt;script&gt; · Lib &lt;&amp;&gt;</title>"),
            "{html}"
        );
        assert!(html.contains("<p>body</p>"));
        assert!(html.contains("aria-current=\"page\""));
        assert!(html.contains("href=\"/guide/setup/child/\""), "{html}");
        assert!(html.contains("rel=\"canonical\" href=\"https://docs.example/guide/setup/\""));
        assert!(!html.contains("<script>"), "{html}");
    }

    #[test]
    fn home_and_not_found_render() {
        let r = Renderer::new(&Theme::folio().unwrap()).unwrap();
        let mut p = page();
        p.kind = "home";
        assert!(
            r.render("home.html", &site(), &p, &assets())
                .unwrap()
                .contains("Guide")
        );
        assert!(
            r.render("not_found.html", &site(), &p, &assets())
                .unwrap()
                .contains("Page not found")
        );
    }

    #[test]
    fn a_missing_asset_is_an_error_not_an_empty_string() {
        let r = Renderer::new(&Theme::folio().unwrap()).unwrap();
        let err = r
            .render("page.html", &site(), &page(), &BTreeMap::new())
            .unwrap_err();
        assert_eq!(err.code, Code::TEMPLATE);
    }
}
