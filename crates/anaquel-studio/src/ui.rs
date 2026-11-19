//! The interface: plain files embedded in the program.
//!
//! No build step, no framework and no inline script or style: the page is served under a policy
//! that allows only the program's own files, so text from a page can never be made to run.

/// The folder of the interface in the address space of the server.
pub(crate) const PREFIX: &str = "/__studio/";

/// One embedded file.
pub(crate) struct UiFile {
    /// The address that serves it.
    pub(crate) path: &'static str,
    pub(crate) content_type: &'static str,
    pub(crate) body: &'static str,
}

/// Every file of the interface. The texts of the two languages are separate files so that a
/// translator edits one file and a test compares their keys.
pub(crate) static FILES: &[UiFile] = &[
    UiFile {
        path: "/__studio/",
        content_type: "text/html; charset=utf-8",
        body: include_str!("ui/index.html"),
    },
    UiFile {
        path: "/__studio/app.js",
        content_type: "text/javascript; charset=utf-8",
        body: include_str!("ui/app.js"),
    },
    UiFile {
        path: "/__studio/app.css",
        content_type: "text/css; charset=utf-8",
        body: include_str!("ui/app.css"),
    },
    UiFile {
        path: "/__studio/texts.en.json",
        content_type: "application/json; charset=utf-8",
        body: include_str!("ui/texts.en.json"),
    },
    UiFile {
        path: "/__studio/texts.es.json",
        content_type: "application/json; charset=utf-8",
        body: include_str!("ui/texts.es.json"),
    },
];

/// The file served at `path`, if there is one.
pub(crate) fn find(path: &str) -> Option<&'static UiFile> {
    FILES.iter().find(|file| file.path == path)
}

#[cfg(test)]
mod tests;
