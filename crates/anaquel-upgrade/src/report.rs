//! What the person is told after an upgrade, in words. Kept apart from the procedure so that the
//! procedure returns facts and this decides how they read.

use anaquel_update::PlanKind;
use semver::Version;

use crate::procedure::{Report, Verdict};

/// The lines to print for a report, advisories first. `current` is the running version.
#[must_use]
pub fn describe(report: &Report, current: &Version) -> Vec<String> {
    let mut lines: Vec<String> = report
        .advisories
        .iter()
        .map(|advisory| {
            format!(
                "advisory {} ({}): {}",
                advisory.id, advisory.severity, advisory.message
            )
        })
        .collect();
    match &report.verdict {
        Verdict::UpToDate => lines.push(format!("anaquel {current} is up to date.")),
        Verdict::Refused(refusal) => lines.push(format!("Not upgrading: {refusal}.")),
        Verdict::WouldInstall { plan, blocker } => {
            lines.push(match plan.kind {
                PlanKind::Upgrade => format!(
                    "anaquel {} is available; you have {current}. Run the same command without `--check` to install it.",
                    plan.version
                ),
                PlanKind::Downgrade => format!(
                    "anaquel {} is older than the {current} you have. Run the same command without `--check` to install it.",
                    plan.version
                ),
                PlanKind::Reinstall => format!(
                    "anaquel {} is the version you have. Run the same command without `--check` to install it again.",
                    plan.version
                ),
            });
            if let Some(blocker) = blocker {
                lines.push(format!("It cannot be installed here: {blocker}."));
            }
        }
        Verdict::Installed { plan, kept } => {
            lines.push(match plan.kind {
                PlanKind::Upgrade => {
                    format!("Upgraded anaquel from {current} to {}.", plan.version)
                }
                PlanKind::Downgrade => {
                    format!("Installed anaquel {} in place of {current}.", plan.version)
                }
                PlanKind::Reinstall => format!("Installed anaquel {} again.", plan.version),
            });
            lines.push(format!(
                "The program it replaced is kept as {}; `anaquel upgrade --rollback` goes back to it.",
                kept.display()
            ));
        }
    }
    if !report.replay_protection {
        lines.push(
            "warning: this system did not say where to keep the time of the last manifest, so an \
             old manifest could not be told from a new one."
                .to_owned(),
        );
    }
    lines
}

#[cfg(test)]
mod tests;
