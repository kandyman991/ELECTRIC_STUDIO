//! Undo and redo for a project, built on the inverse commands returned by [`Project::apply`].

use crate::project::{Command, Project, Result};

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    label: String,
    /// The command that reverses the recorded change.
    inverse: Command,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

impl History {
    pub fn new() -> Self {
        History::default()
    }

    /// Apply `cmd` to the project and record it for undo.
    pub fn execute(&mut self, project: &mut Project, label: &str, cmd: Command) -> Result<()> {
        let inverse = project.apply(cmd)?;
        self.record(label, inverse);
        Ok(())
    }

    /// Record a change that was already applied, given the command that reverses it (used by
    /// live drags, which move the project on every pointer event and record once at the end).
    pub fn record(&mut self, label: &str, inverse: Command) {
        self.undo.push(Entry { label: label.to_owned(), inverse });
        self.redo.clear();
    }

    /// Undo the last change; returns its label, or `None` when there is nothing to undo.
    pub fn undo(&mut self, project: &mut Project) -> Result<Option<String>> {
        Self::shift(project, &mut self.undo, &mut self.redo)
    }

    /// Redo the last undone change; returns its label.
    pub fn redo(&mut self, project: &mut Project) -> Result<Option<String>> {
        Self::shift(project, &mut self.redo, &mut self.undo)
    }

    fn shift(project: &mut Project, from: &mut Vec<Entry>, to: &mut Vec<Entry>) -> Result<Option<String>> {
        let Some(entry) = from.pop() else { return Ok(None) };
        match project.apply(entry.inverse.clone()) {
            Ok(inverse) => {
                let label = entry.label.clone();
                to.push(Entry { label: entry.label, inverse });
                Ok(Some(label))
            }
            Err(e) => {
                from.push(entry);
                Err(e)
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }
}
