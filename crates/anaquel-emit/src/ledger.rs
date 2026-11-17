use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anaquel_diagnostics::{Code, Diagnostic, Report};
use serde::{Deserialize, Serialize};

/// Where the ledger lives inside the output folder.
pub(crate) const LEDGER_PATH: &str = ".anaquel/ledger.json";

/// What one build produced.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    /// Format number of this file.
    pub schema: u32,
    /// Output path to short content hash, for every file written.
    pub files: BTreeMap<String, String>,
    /// URLs of every published page.
    pub urls: BTreeSet<String>,
    /// Former URLs that redirect to a page.
    pub redirects: BTreeSet<String>,
}

impl Ledger {
    /// Current format number.
    pub const SCHEMA: u32 = 1;

    /// Reads the ledger of a previous build, if there is a usable one.
    pub fn load(dist: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(dist.join(LEDGER_PATH)).ok()?;
        let ledger: Self = serde_json::from_str(&text).ok()?;
        (ledger.schema == Self::SCHEMA).then_some(ledger)
    }

    /// Serialises the ledger as stable, readable JSON.
    ///
    /// # Errors
    /// Returns a diagnostic if serialisation fails, which cannot happen for
    /// this type; the error exists so callers do not need a panic.
    pub fn to_json(&self) -> Result<String, Diagnostic> {
        serde_json::to_string_pretty(self)
            .map(|mut text| {
                text.push('\n');
                text
            })
            .map_err(|e| Diagnostic::error(Code::IO, format!("cannot serialise the ledger: {e}")))
    }
}

/// Reports every URL the previous build served that this build neither
/// serves nor redirects. A moved page without a redirect breaks every link
/// to it from outside, so it is an error, not a warning.
pub fn lost_urls(
    previous: &Ledger,
    urls: &BTreeSet<String>,
    redirects: &BTreeSet<String>,
) -> Report {
    let mut report = Report::new();
    for url in previous.urls.iter().chain(&previous.redirects) {
        if !urls.contains(url) && !redirects.contains(url) {
            report.push(
                Diagnostic::error(
                    Code::LOST_URL,
                    format!("{url} was published before and now leads nowhere"),
                )
                .with_help(
                    "add the old URL to the new page's `aliases`, or use `anaquel relocate`",
                ),
            );
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_vanished_url_is_an_error_unless_it_redirects() {
        let previous = Ledger {
            schema: 1,
            urls: set(&["/a/", "/b/"]),
            ..Ledger::default()
        };
        let report = lost_urls(&previous, &set(&["/a/"]), &set(&[]));
        assert_eq!(report.items().len(), 1);
        assert!(report.items()[0].message.starts_with("/b/"));
        assert!(!lost_urls(&previous, &set(&["/a/"]), &set(&["/b/"])).has_errors());
    }

    #[test]
    fn old_redirects_must_keep_working_too() {
        let previous = Ledger {
            schema: 1,
            redirects: set(&["/old/"]),
            ..Ledger::default()
        };
        assert!(lost_urls(&previous, &set(&[]), &set(&[])).has_errors());
    }

    #[test]
    fn json_round_trips() {
        let ledger = Ledger {
            schema: 1,
            files: BTreeMap::from([("a.html".to_owned(), "ff".to_owned())]),
            ..Ledger::default()
        };
        let back: Ledger = serde_json::from_str(&ledger.to_json().unwrap()).unwrap();
        assert_eq!(back, ledger);
    }
}
