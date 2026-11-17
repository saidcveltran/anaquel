use std::collections::HashSet;

use anaquel_catalog::{Catalog, Entry, EntryId, EntryKind};
use anaquel_render::{LinkView, NavNode, PageView, SiteView, TocItem};

/// What the theme may know about the whole library.
pub(crate) fn site_view(catalog: &Catalog, search: bool) -> SiteView {
    let library = catalog.library();
    SiteView {
        title: library.title.clone(),
        description: library.description.clone(),
        language: library.language.clone(),
        base_url: library
            .base_url
            .as_ref()
            .map(|u| u.trim_end_matches('/').to_owned()),
        volumes: catalog
            .volumes()
            .iter()
            .filter_map(|v| {
                volume_link(
                    catalog,
                    v.root,
                    &v.manifest.title,
                    v.manifest.description.as_deref(),
                )
            })
            .collect(),
        search,
        search_path: format!("/{}/", anaquel_search::BUNDLE_DIR),
    }
}

fn volume_link(
    catalog: &Catalog,
    root: Option<EntryId>,
    title: &str,
    description: Option<&str>,
) -> Option<LinkView> {
    let root = catalog.entry(root?)?;
    Some(LinkView {
        title: title.to_owned(),
        url: root.url.clone(),
        description: description.map(str::to_owned),
    })
}

fn link(entry: &Entry) -> LinkView {
    LinkView {
        title: entry.title.clone(),
        url: entry.url.clone(),
        description: entry.description.clone(),
    }
}

/// Builds the view of one published entry.
pub(crate) fn page_view(catalog: &Catalog, entry: &Entry, html: String, plain: &str) -> PageView {
    let volume = &catalog.volumes()[entry.volume];
    let volume_link = volume_link(
        catalog,
        volume.root,
        &volume.manifest.title,
        volume.manifest.description.as_deref(),
    )
    .unwrap_or_else(|| link(entry));

    let mut ancestors = Vec::new();
    let mut cursor = entry.parent;
    while let Some(parent) = cursor.and_then(|id| catalog.entry(id)) {
        ancestors.push(parent);
        cursor = parent.parent;
    }
    ancestors.reverse();
    let on_path: HashSet<EntryId> = ancestors
        .iter()
        .map(|e| e.id)
        .chain(Some(entry.id))
        .collect();

    let roots = volume
        .root
        .and_then(|id| catalog.entry(id))
        .map(|root| root.children.as_slice())
        .unwrap_or_default();

    PageView {
        kind: if entry.kind == EntryKind::Section {
            "section"
        } else {
            "page"
        },
        title: entry.title.clone(),
        description: entry.description.clone().or_else(|| summarise(plain)),
        url: entry.url.clone(),
        content: html,
        toc: entry
            .headings
            .iter()
            .filter(|h| (2..=3).contains(&h.level))
            .map(|h| TocItem {
                level: h.level,
                text: h.text.clone(),
                anchor: h.anchor.clone(),
            })
            .collect(),
        breadcrumbs: ancestors.iter().map(|e| link(e)).collect(),
        volume: volume_link,
        nav: nav_nodes(catalog, roots, entry.id, &on_path),
        children: entry
            .children
            .iter()
            .filter_map(|id| catalog.entry(*id))
            .filter(|c| !c.draft)
            .map(link)
            .collect(),
        prev: entry.prev.and_then(|id| catalog.entry(id)).map(link),
        next: entry.next.and_then(|id| catalog.entry(id)).map(link),
    }
}

/// The sidebar shows the volume's top level and opens only the path to the
/// current page, so its size does not grow with the size of the volume.
fn nav_nodes(
    catalog: &Catalog,
    ids: &[EntryId],
    current: EntryId,
    on_path: &HashSet<EntryId>,
) -> Vec<NavNode> {
    ids.iter()
        .filter_map(|id| catalog.entry(*id))
        .filter(|e| !e.draft)
        .map(|e| NavNode {
            title: e.title.clone(),
            url: e.url.clone(),
            current: e.id == current,
            active: on_path.contains(&e.id),
            children: if on_path.contains(&e.id) {
                nav_nodes(catalog, &e.children, current, on_path)
            } else {
                Vec::new()
            },
        })
        .collect()
}

/// An empty page view for the home page and the 404 page.
pub(crate) fn blank_page(kind: &'static str, title: &str, url: &str) -> PageView {
    let placeholder = LinkView {
        title: title.to_owned(),
        url: "/".to_owned(),
        description: None,
    };
    PageView {
        kind,
        title: title.to_owned(),
        description: None,
        url: url.to_owned(),
        content: String::new(),
        toc: Vec::new(),
        breadcrumbs: Vec::new(),
        volume: placeholder,
        nav: Vec::new(),
        children: Vec::new(),
        prev: None,
        next: None,
    }
}

/// The start of the page text, cut at a word, for the `description` metadata.
fn summarise(plain: &str) -> Option<String> {
    const LIMIT: usize = 160;
    let text = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
    }
    if text.chars().count() <= LIMIT {
        return Some(text);
    }
    let cut: String = text.chars().take(LIMIT).collect();
    let end = cut.rfind(' ').unwrap_or(cut.len());
    Some(format!(
        "{}…",
        cut[..end].trim_end_matches([',', ';', ':', '.', ' '])
    ))
}

#[cfg(test)]
mod tests {
    use super::summarise;

    #[test]
    fn summary_is_cut_at_a_word() {
        let long = "word ".repeat(100);
        let s = summarise(&long).unwrap();
        assert!(s.ends_with('…') && s.chars().count() <= 161, "{s}");
        assert_eq!(summarise("  "), None);
        assert_eq!(summarise("short  text").as_deref(), Some("short text"));
    }
}
