//! One release-validation policy (AI0, F08): the same blocking reasons for every release path —
//! the app's release check, `ecad check --release`, `ecad export --release` and, later,
//! baseline release creation and issued documents (S1). Draft exports stay available and are
//! labelled unreleased.
//!
//! Today it blocks on what the model and the checks can tell: check errors (integrity and
//! electrical rules), devices whose product is not in the project's catalogue, and
//! safety-related devices whose classification is dangling. Later milestones add their reasons
//! through [`EXTENSIONS`] (unresolved product placeholders and terminal mappings in M9, missing
//! verifications in S1) without another code path.

use crate::Severity;
use ecad_model::Project;

/// What passing means, said wherever the result is shown.
pub const NOTE: &str = "Release validation checks software consistency and the configured engineering checks; it is not electrical or functional safety certification.";

/// One reason a release is blocked.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Blocker {
    pub rule: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Release {
    pub blocking: Vec<Blocker>,
}

impl Release {
    pub fn releasable(&self) -> bool {
        self.blocking.is_empty()
    }
    pub fn summary(&self) -> String {
        if self.releasable() {
            return format!("releasable. {NOTE}");
        }
        let list: Vec<String> = self.blocking.iter().map(|b| format!("{}: {}", b.rule, b.message)).collect();
        format!("not releasable ({} blocking): {}", self.blocking.len(), list.join("; "))
    }
}

/// A release rule: the blocking reasons it finds.
pub type Rule = fn(&Project) -> Vec<Blocker>;

fn check_errors(p: &Project) -> Vec<Blocker> {
    crate::check(p)
        .into_iter()
        .filter(|c| c.severity == Severity::Error)
        .map(|c| Blocker { rule: c.rule, message: format!("{} ({})", c.message, c.location) })
        .collect()
}

fn unresolved_products(p: &Project) -> Vec<Blocker> {
    p.devices
        .values()
        .flat_map(|d| d.parts.iter().filter(|r| !p.parts.contains_key(&r.product)).map(move |r| (d, r)))
        .map(|(d, r)| Blocker {
            rule: "unresolved-product",
            message: format!("{} refers to product {} that the catalogue doesn't have", d.tag, r.product.0),
        })
        .collect()
}

fn safety_classification(p: &Project) -> Vec<Blocker> {
    p.safety_related
        .iter()
        .filter(|d| p.device(**d).is_err())
        .map(|d| Blocker {
            rule: "safety-classification",
            message: format!("a safety-related marking names no device ({})", d.0),
        })
        .collect()
}

/// The rules every release path runs, in order. Later milestones add theirs here.
pub const EXTENSIONS: &[Rule] = &[check_errors, unresolved_products, safety_classification];

/// Validate `project` for release.
pub fn validate(project: &Project) -> Release {
    Release { blocking: EXTENSIONS.iter().flat_map(|rule| rule(project)).collect() }
}
