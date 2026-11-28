//! Replacement, distinct from removal (S0, D23): a device keeps its identity (id, tag,
//! terminals, drawings, wires, layout position) and takes other products. Terminal
//! reconciliation is explicit: the caller gives each terminal whose designation changes;
//! every other terminal keeps its own, and nothing is dropped or reconnected. Nothing is
//! retired, since nothing is removed.

use crate::connectivity::TerminalId;
use crate::numbering::Identifier;
use crate::parts::PartRef;
use crate::project::{Command, DeviceId, ModelError, Project, Result};
use std::collections::{BTreeMap, BTreeSet};

impl Project {
    pub(crate) fn apply_replace(
        &mut self,
        id: DeviceId,
        parts: Vec<PartRef>,
        designations: BTreeMap<TerminalId, String>,
    ) -> Result<Command> {
        let tag = self.device(id)?.tag.clone();
        self.check_part_refs(&parts)?;
        let mut next: BTreeMap<TerminalId, String> = BTreeMap::new();
        for (t, d) in &designations {
            let entry = self.terminals.get(t).ok_or(ModelError::NoTerminal(t.0))?;
            if entry.device != id {
                return Err(ModelError::TerminalOfOtherDevice);
            }
            let d = d.trim();
            if d.is_empty() {
                return Err(ModelError::EmptyNumber);
            }
            if d != entry.designation {
                self.check_available(&Identifier::Terminal { device: id, designation: d.to_owned() })?;
                next.insert(*t, d.to_owned());
            }
        }
        // A physical group keeps one designation: renamed whole or not at all.
        let touched: BTreeSet<_> = next.keys().filter_map(|t| self.terminals.get(t).map(|e| e.group)).collect();
        let after = |t: &crate::TerminalEntry| next.get(&t.id).cloned().unwrap_or_else(|| t.designation.clone());
        for g in &touched {
            let members: BTreeSet<String> =
                self.terminals.values().filter(|t| t.device == id && t.group == *g).map(after).collect();
            if members.len() != 1 {
                return Err(ModelError::InvalidReconciliation(tag));
            }
        }
        // Strip terminals: one group per designation.
        let mut seen: BTreeMap<String, crate::connectivity::GroupId> = BTreeMap::new();
        for t in self.terminals.values().filter(|t| t.device == id && !t.point.is_empty()) {
            let d = after(t);
            if seen.insert(d.clone(), t.group).is_some_and(|g| g != t.group) {
                return Err(ModelError::DuplicateNumber(tag, d));
            }
        }
        let mut undo = BTreeMap::new();
        for (t, d) in next {
            if let Some(entry) = self.terminals.get_mut(&t) {
                undo.insert(t, std::mem::replace(&mut entry.designation, d));
            }
        }
        let d = self.devices.get_mut(&id).ok_or(ModelError::NoDevice(id))?;
        let old = std::mem::replace(&mut d.parts, parts);
        Ok(Command::ReplaceDevice { id, parts: old, designations: undo })
    }
}
