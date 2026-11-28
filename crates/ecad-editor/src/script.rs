//! Editing by name instead of by pointer, for scripts and the MCP server: a page by its name,
//! a connection point as `-K1:A1`, a PLC channel as card and channel. Every change goes through
//! the same commands and undo history as the canvas, one undo step per call.

use crate::{Editor, GRID, snap};
use ecad_model::geometry::Point;
use ecad_model::{
    ADDRESS, COMMENT, Command, ConnectionId, ConnectionRef, DeviceId, FunctionId, PageId, Project, SYMBOL, SymbolId,
};

/// A page by its name (`2`, `A3`) or, if no page has that name, its place in the order (`#2`
/// or a bare number past the names).
pub fn find_page(project: &Project, page: &str) -> Result<PageId, String> {
    let pages = project.pages_in_order();
    if let Some(p) = pages.iter().find(|p| p.name == page) {
        return Ok(p.id);
    }
    let n = page.trim_start_matches('#').parse::<usize>().ok().filter(|n| *n >= 1 && *n <= pages.len());
    match n {
        Some(n) => Ok(pages[n - 1].id),
        None => {
            let names: Vec<&str> = pages.iter().map(|p| p.name.as_str()).collect();
            Err(format!("no page `{page}` (pages: {})", names.join(", ")))
        }
    }
}

/// A device by its tag.
pub fn find_device(project: &Project, tag: &str) -> Result<DeviceId, String> {
    project.device_by_tag(tag).map(|d| d.id).ok_or_else(|| format!("no device `{tag}`"))
}

/// A connection point written `TAG:POINT` as drawn (`-K1:A1`, `-X1:3`, `-Q1:2`): the point is
/// the designation printed beside it. The tag may itself contain `:` only before the last one.
pub fn find_point(project: &Project, spec: &str) -> Result<ConnectionRef, String> {
    let (tag, point) = spec.rsplit_once(':').ok_or_else(|| format!("`{spec}` isn't TAG:POINT (like -K1:A1)"))?;
    // A device by its tag, or a node (a potential, interruption or junction) by its name.
    let drawings: Vec<&ecad_model::Function> = match project.device_by_tag(tag) {
        Some(d) => project.functions_of(d.id).collect(),
        None => project.functions.values().filter(|f| f.node.is_some() && project.label_of(f) == tag).collect(),
    };
    if drawings.is_empty() {
        return Err(format!("no device `{tag}`"));
    }
    let mut found: Vec<ConnectionRef> = Vec::new();
    let mut all: Vec<String> = Vec::new();
    for f in drawings {
        let Ok(symbol) = project.symbol(&f.symbol) else { continue };
        for cp in &symbol.connection_points {
            let shown = project.designation(f, &cp.name);
            all.push(shown.to_owned());
            if shown == point {
                found.push(ConnectionRef::new(f.id, &cp.name));
            }
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => {
            all.sort();
            all.dedup();
            Err(format!("{tag} has no point `{point}` (points: {})", all.join(", ")))
        }
        _ => Err(format!("{tag} has {} points drawn `{point}`: can't tell which", found.len())),
    }
}

impl Editor {
    /// Place a symbol at `x`, `y` mm on a page (snapped to the 4 mm grid): as a new device with
    /// the project's next tag, or as another drawing of an existing one (a relay's contact).
    pub fn script_place(
        &mut self,
        symbol: &str,
        page: PageId,
        x: f64,
        y: f64,
        device: Option<&str>,
    ) -> Result<FunctionId, String> {
        let symbol = SymbolId(symbol.to_owned());
        self.project.symbol(&symbol).map_err(|e| e.to_string())?;
        if !(x.is_finite() && y.is_finite()) {
            return Err("x and y must be numbers".into());
        }
        let at = snap(Point::new(x, y), GRID);
        let placed = match device {
            // A node symbol names its node: `L+` is a potential, found by name (D33).
            Some(name) if self.project.node_kind_of(&symbol).is_some() => {
                let kind = self.project.node_kind_of(&symbol).ok_or("not a node symbol")?;
                let node =
                    self.project.node_by_name(kind, name, ecad_model::PROJECT_SCOPE).map_err(|e| e.to_string())?.id;
                self.place_node_drawing(page, &symbol, node, at)
            }
            Some(tag) => {
                let device = find_device(&self.project, tag)?;
                self.place_another(page, &symbol, device, at)
            }
            None => self.place_new(page, &symbol, at),
        };
        placed.map_err(|e| e.to_string())
    }

    /// Add a wire between the terminals or nodes two drawn points show, without drawing it
    /// (D1): undoable, like any edit.
    pub fn script_add_wire(&mut self, from: &str, to: &str, number: Option<&str>) -> Result<ConnectionId, String> {
        let a = find_point(&self.project, from)?;
        let b = find_point(&self.project, to)?;
        let c = self.project.connectivity();
        let (Some(from_end), Some(to_end)) = (c.endpoint(&a), c.endpoint(&b)) else {
            return Err("those points show no terminal or node".into());
        };
        let id = ConnectionId::new();
        let wire = ecad_model::Wire {
            id,
            from: from_end,
            to: to_end,
            wire_number: number.map(str::to_owned),
            spec: Default::default(),
        };
        self.execute(&format!("Add wire {from} to {to} (not drawn)"), Command::AddWire(wire))
            .map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// Draw a wire between two free points (`-S1:14`, `-K1:A1`).
    pub fn script_connect(&mut self, from: &str, to: &str) -> Result<ConnectionId, String> {
        let a = find_point(&self.project, from)?;
        let b = find_point(&self.project, to)?;
        if a == b {
            return Err("a wire needs two different points".into());
        }
        self.draw_wire(a, b).map_err(|e| e.to_string())
    }

    /// Delete the wire at a point (`-K1:A1`).
    pub fn script_disconnect(&mut self, at: &str) -> Result<String, String> {
        let r = find_point(&self.project, at)?;
        let c = self.project.connection_at(&r).ok_or_else(|| format!("no wire at {at}"))?;
        let (id, number) = (c.id, c.wire_number.clone().unwrap_or_default());
        self.delete_wire(id).map_err(|e| e.to_string())?;
        Ok(number)
    }

    /// Delete a device: all its drawings and their wires, and its footprint on a layout, in
    /// one step.
    pub fn script_delete_device(&mut self, tag: &str) -> Result<(), String> {
        let device = find_device(&self.project, tag)?;
        let mut cmds: Vec<Command> = self.project.functions_of(device).map(|f| Command::Unplace(f.id)).collect();
        if let Some(item) = self.project.panel_item_of(device) {
            cmds.push(Command::RemovePanelItem(item.id));
        }
        cmds.push(Command::RemoveDevice(device));
        self.execute_removal(&format!("Delete {tag}"), cmds).map_err(|e| e.to_string())
    }

    /// Give a device a new tag and/or description, in one step.
    pub fn script_edit_device(
        &mut self,
        tag: &str,
        new_tag: Option<&str>,
        description: Option<&str>,
    ) -> Result<(), String> {
        let id = find_device(&self.project, tag)?;
        let mut cmds = Vec::new();
        if let Some(t) = new_tag.filter(|t| *t != tag) {
            cmds.push(Command::RenameDevice { id, tag: t.to_owned() });
        }
        if let Some(d) = description {
            cmds.push(Command::SetDescription { id, description: d.to_owned() });
        }
        if cmds.is_empty() {
            return Ok(());
        }
        self.execute(&format!("Edit {tag}"), Command::Batch(cmds)).map_err(|e| e.to_string())
    }

    /// Set a PLC channel's address, symbol and comment (`None` keeps it, `""` clears it), in
    /// one step. The channel is the number drawn on the card (`-KF1`, `3`).
    pub fn script_set_plc_channel(
        &mut self,
        card: &str,
        channel: &str,
        address: Option<&str>,
        symbol: Option<&str>,
        comment: Option<&str>,
    ) -> Result<(), String> {
        let device = find_device(&self.project, card)?;
        let channels = self.project.plc_channels_of(device);
        let c = channels.iter().find(|c| c.channel == channel).ok_or_else(|| {
            let list: Vec<&str> = channels.iter().map(|c| c.channel.as_str()).collect();
            format!("{card} has no channel `{channel}` (channels: {})", list.join(", "))
        })?;
        let mut properties = self.project.properties_of(self.project.function(c.function).map_err(|e| e.to_string())?);
        for (key, value) in [(ADDRESS, address), (SYMBOL, symbol), (COMMENT, comment)] {
            match value {
                Some("") => {
                    properties.remove(key);
                }
                Some(v) => {
                    properties.insert(key.to_owned(), v.to_owned());
                }
                None => {}
            }
        }
        let label = format!("Edit {card}:{channel}");
        self.execute(&label, Command::SetProperties { id: c.function, properties }).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ecad_model::sample;

    fn editor(p: Project) -> Editor {
        let page = p.pages_in_order()[0].id;
        Editor::new(p, page).unwrap()
    }

    #[test]
    fn a_small_circuit_is_drawn_by_name_and_undone_step_by_step() {
        let mut e = editor(sample::start_circuit().unwrap());
        let page = find_page(e.project(), "#1").unwrap();
        let before = e.project().clone();
        let f = e.script_place("iec.coil", page, 201.0, 99.0, None).unwrap();
        let tag = e.project().device(e.project().function(f).unwrap().device).unwrap().tag.clone();
        assert_eq!(e.project().function(f).unwrap().position, Point::new(200.0, 100.0), "on the grid");
        let g = e.script_place("iec.coil", page, 240.0, 100.0, None).unwrap();
        let other = e.project().device(e.project().function(g).unwrap().device).unwrap().tag.clone();
        e.script_connect(&format!("{tag}:A2"), &format!("{other}:A1")).unwrap();
        // The point is taken now.
        let err = e.script_connect(&format!("{tag}:A2"), &format!("{other}:A2")).unwrap_err();
        assert!(err.contains("already"), "{err}");
        e.script_edit_device(&tag, Some("-K9"), Some("Test relay")).unwrap();
        assert_eq!(e.project().device_by_tag("-K9").unwrap().description, "Test relay");
        assert_eq!(e.undo_label(), Some(format!("Edit {tag}").as_str()));
        for _ in 0..4 {
            assert!(e.undo().unwrap().is_some());
        }
        assert_eq!(e.project(), &before);
        assert!(e.redo().unwrap().is_some());
    }

    #[test]
    fn names_that_dont_exist_say_what_does() {
        let mut e = editor(sample::start_circuit().unwrap());
        let page = e.page();
        assert!(find_page(e.project(), "Z9").unwrap_err().starts_with("no page `Z9` (pages: "));
        assert_eq!(find_device(e.project(), "-Q99").unwrap_err(), "no device `-Q99`");
        let err = find_point(e.project(), "-S1:99").unwrap_err();
        assert!(err.starts_with("-S1 has no point `99` (points: 13, 14"), "{err}");
        assert!(find_point(e.project(), "S1").unwrap_err().contains("TAG:POINT"));
        assert!(e.script_place("iec.nothing", page, 0.0, 0.0, None).is_err());
        assert!(e.script_place("iec.coil", page, f64::NAN, 0.0, None).is_err());
        assert_eq!(e.script_disconnect("-K1:A1").map(|_| ()).err(), None, "the start circuit has a wire there");
        assert!(e.script_disconnect("-K1:A1").unwrap_err().starts_with("no wire at"));
    }

    #[test]
    fn plc_channels_and_devices_are_edited_by_name() {
        let mut e = editor(sample::with_plc().unwrap());
        e.script_set_plc_channel("-KF1", "0", None, Some("Door_Shut"), Some("")).unwrap();
        let c = &e.project().plc_channels_of(find_device(e.project(), "-KF1").unwrap())[0];
        assert_eq!((c.address.as_str(), c.symbol.as_str(), c.comment.as_str()), ("I0.0", "Door_Shut", ""));
        // The model's checks still apply: an address another channel has is refused.
        let err = e.script_set_plc_channel("-KF1", "1", Some("%I0.0"), None, None).unwrap_err();
        assert!(err.contains("I0.0"), "{err}");
        assert!(e.script_set_plc_channel("-KF1", "77", None, None, None).unwrap_err().contains("channels: 0"));
        let wires = e.project().connections.len();
        e.script_delete_device("-KF1").unwrap();
        assert!(e.project().device_by_tag("-KF1").is_none());
        assert!(e.project().connections.len() < wires);
        e.undo().unwrap();
        assert_eq!(e.project().connections.len(), wires);
    }
}
