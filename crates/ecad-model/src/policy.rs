//! Project policy for the engineering interface (AI0): which devices are safety-related, and
//! which operation classes trusted automation may apply (D19, D42).
//!
//! Both are engineering settings, part of the project's content and its content hash: changing
//! them advances the committed revision, so an approval made under another policy goes stale.
//! A person sets them, in the app.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Classes of ordinary operations trusted automation may be allowed to apply with `--apply`
/// (D42). Global operations (renumbering, regeneration of existing content) and safety-related
/// changes are never in any automation scope: they go through a reviewed change set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AutomationClass {
    /// Ordinary edits: placing, connecting, deleting, editing devices and channels.
    Edit,
    /// Reserving, unreserving, retiring and reclaiming identifiers.
    Numbering,
    /// Imports: PLC tag tables, drawings as underlays.
    Import,
}

impl AutomationClass {
    pub const ALL: [AutomationClass; 3] = [AutomationClass::Edit, AutomationClass::Numbering, AutomationClass::Import];
    pub fn label(self) -> &'static str {
        match self {
            AutomationClass::Edit => "edit",
            AutomationClass::Numbering => "numbering",
            AutomationClass::Import => "import",
        }
    }
}

/// The project's automation scope: none by default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Automation {
    pub allowed: BTreeSet<AutomationClass>,
}

impl Automation {
    pub fn is_empty(&self) -> bool {
        self.allowed.is_empty()
    }
}
