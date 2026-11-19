//! Fixed texts of the theme in the languages the engine ships.
//!
//! A theme reads them as `ui.NAME`. The language is the primary subtag of the
//! page language (`es-MX` reads Spanish); anything the engine has no table for
//! reads English.

use serde::Serialize;

/// The texts a template may print.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct Ui {
    /// Link that jumps over the header.
    pub skip: &'static str,
    /// Button that opens the sidebar on a narrow screen.
    pub menu: &'static str,
    /// Button that switches between light and dark.
    pub toggle_theme: &'static str,
    /// Label of the sidebar.
    pub volume: &'static str,
    /// Label of the trail of ancestors.
    pub breadcrumb: &'static str,
    /// Label of the previous and next links.
    pub pages: &'static str,
    /// Link to the previous page.
    pub previous: &'static str,
    /// Link to the next page.
    pub next: &'static str,
    /// Heading of the table of contents.
    pub on_this_page: &'static str,
    /// Label of the language switcher.
    pub languages: &'static str,
    /// Label of the search box.
    pub search: &'static str,
    /// Title of the 404 page.
    pub not_found: &'static str,
    /// Explanation on the 404 page.
    pub not_found_text: &'static str,
    /// Link text on the 404 page, followed by the library title.
    pub go_to_start: &'static str,
}

const EN: Ui = Ui {
    skip: "Skip to content",
    menu: "Menu",
    toggle_theme: "Toggle theme",
    volume: "Volume",
    breadcrumb: "Breadcrumb",
    pages: "Pages",
    previous: "Previous",
    next: "Next",
    on_this_page: "On this page",
    languages: "Languages",
    search: "Search",
    not_found: "Page not found",
    not_found_text: "There is nothing at this address. It may have been moved or removed.",
    go_to_start: "Go to the start of",
};

const ES: Ui = Ui {
    skip: "Saltar al contenido",
    menu: "Menú",
    toggle_theme: "Cambiar el tema",
    volume: "Volumen",
    breadcrumb: "Ruta de navegación",
    pages: "Páginas",
    previous: "Anterior",
    next: "Siguiente",
    on_this_page: "En esta página",
    languages: "Idiomas",
    search: "Buscar",
    not_found: "Página no encontrada",
    not_found_text: "No hay nada en esta dirección. Puede que se haya movido o eliminado.",
    go_to_start: "Ir al inicio de",
};

/// The texts for a BCP 47 language tag.
#[must_use]
pub fn ui_for(language: &str) -> Ui {
    let primary = language.split('-').next().unwrap_or("");
    if primary.eq_ignore_ascii_case("es") {
        ES
    } else {
        EN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_region_does_not_matter_and_unknown_languages_read_english() {
        assert_eq!(ui_for("es-MX"), ES);
        assert_eq!(ui_for("ES"), ES);
        assert_eq!(ui_for("fr"), EN);
        assert_eq!(ui_for(""), EN);
    }
}
