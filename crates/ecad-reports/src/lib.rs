//! Reports are queries on the model, so they can never disagree with the drawings.

use ecad_model::{ModelError, Project};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireListRow {
    pub wire: String,
    /// The potential's name (`L+`), empty when unnamed.
    pub potential: String,
    pub from_tag: String,
    pub from_point: String,
    pub to_tag: String,
    pub to_point: String,
    pub cross_section: String,
    pub colour: String,
    /// The cable's tag and the core, when the wire runs in a cable.
    pub cable: String,
    pub core: String,
    /// The estimated length in mm from the panel layout, empty when an end isn't on a plate.
    pub length: String,
    /// How the length was measured: `ducts`, `straight` (no ducts join the ends) or empty.
    pub route: String,
}

/// One row per connection, sorted by wire number.
pub fn wire_list(project: &Project) -> Result<Vec<WireListRow>, ModelError> {
    let potentials = project.potentials();
    let lengths = project.wire_lengths();
    project
        .connections
        .values()
        .map(|c| {
            let (from_tag, from_point) = project.describe(&c.from)?;
            let (to_tag, to_point) = project.describe(&c.to)?;
            let potential =
                potentials.iter().find(|p| p.connections.contains(&c.id)).and_then(|p| p.name()).unwrap_or_default();
            let (cable, core) = c
                .spec
                .cable
                .as_ref()
                .map(|k| (project.cables.get(&k.cable).map(|c| c.tag.clone()).unwrap_or_default(), k.core.clone()))
                .unwrap_or_default();
            let (length, route) = lengths
                .get(&c.id)
                .map(|l| (format!("{}", l.mm.round()), if l.direct { "straight" } else { "ducts" }.to_owned()))
                .unwrap_or_default();
            Ok(WireListRow {
                wire: c.wire_number.clone().unwrap_or_default(),
                potential,
                from_tag,
                from_point,
                to_tag,
                to_point,
                cross_section: c.spec.cross_section.clone(),
                colour: c.spec.colour.clone(),
                cable,
                core,
                length,
                route,
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|mut rows| {
            rows.sort_by(|a, b| a.wire.cmp(&b.wire));
            rows
        })
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_owned() }
}

pub fn wire_list_csv(rows: &[WireListRow]) -> String {
    let mut out = String::from(
        "wire,potential,from_device,from_point,to_device,to_point,cross_section,colour,cable,core,length_mm,route\n",
    );
    for r in rows {
        let fields = [
            &r.wire,
            &r.potential,
            &r.from_tag,
            &r.from_point,
            &r.to_tag,
            &r.to_point,
            &r.cross_section,
            &r.colour,
            &r.cable,
            &r.core,
            &r.length,
            &r.route,
        ];
        out += &fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",");
        out.push('\n');
    }
    out
}

/// One line of the bill of materials: a part, how many, and which devices use it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BomRow {
    pub number: String,
    pub manufacturer: String,
    pub description: String,
    pub quantity: u32,
    /// Tags of the devices using it, sorted.
    pub devices: Vec<String>,
    /// The company's internal code for the part.
    pub code: String,
    /// Pages where those devices are drawn, in page order.
    pub pages: Vec<String>,
}

/// The bill of materials: every part used by a device, with the total quantity, sorted by part
/// number. Parts of a terminal strip count once per terminal, once per terminal of one kind
/// (`only`), or once for the strip.
pub fn bom(project: &Project) -> Vec<BomRow> {
    let mut rows: std::collections::BTreeMap<&str, BomRow> = std::collections::BTreeMap::new();
    for d in project.devices.values() {
        let terminals = project.terminals_of(d.id);
        for r in &d.parts {
            let per = match (&r.only, r.per_strip) {
                (_, true) => 1,
                // Only the terminals drawn with that symbol (may be none).
                (Some(symbol), false) => {
                    terminals.iter().filter(|t| project.function(t.function).is_ok_and(|f| f.symbol == *symbol)).count()
                        as u32
                }
                (None, false) => (terminals.len() as u32).max(1),
            };
            let part = project.parts.get(&r.number);
            let row = rows.entry(&r.number).or_insert_with(|| BomRow {
                number: r.number.clone(),
                manufacturer: part.map(|p| p.manufacturer.clone()).unwrap_or_default(),
                description: part.map(|p| p.description.clone()).unwrap_or_default(),
                quantity: 0,
                devices: Vec::new(),
                code: part.map(|p| p.code.clone()).unwrap_or_default(),
                pages: Vec::new(),
            });
            row.quantity += r.quantity * per;
            row.devices.push(d.tag.clone());
            for f in project.functions_of(d.id) {
                if let Ok(page) = project.page(f.page)
                    && !row.pages.contains(&page.name)
                {
                    row.pages.push(page.name.clone());
                }
            }
        }
    }
    let order: Vec<String> = project.pages_in_order().iter().map(|p| p.name.clone()).collect();
    rows.into_values()
        .map(|mut r| {
            r.devices.sort();
            r.pages.sort_by_key(|p| order.iter().position(|o| o == p));
            r
        })
        .collect()
}

/// Devices that need a part but have none: everything except potential points, interruption
/// points and junctions, which are drawing aids rather than components. Sorted by tag.
pub fn devices_without_parts(project: &Project) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = project
        .devices
        .values()
        .filter(|d| d.parts.is_empty())
        .filter(|d| project.needs_part(d.id))
        .map(|d| (d.tag.clone(), d.description.clone()))
        .collect();
    out.sort();
    out
}

pub fn bom_csv(rows: &[BomRow]) -> String {
    let mut out = String::from("part_number,manufacturer,description,quantity,devices,code,pages\n");
    for r in rows {
        let fields = [
            r.number.clone(),
            r.manufacturer.clone(),
            r.description.clone(),
            r.quantity.to_string(),
            r.devices.join(" "),
            r.code.clone(),
            r.pages.join(" "),
        ];
        out += &fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",");
        out.push('\n');
    }
    out
}

pub fn devices_without_parts_csv(rows: &[(String, String)]) -> String {
    let mut out = String::from("device,description\n");
    for (tag, description) in rows {
        out += &format!("{},{}\n", csv_field(tag), csv_field(description));
    }
    out
}

/// One terminal in the terminal diagram.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalRow {
    pub strip: String,
    pub terminal: String,
    /// What the inside (top, cabinet) side is wired to, e.g. `-K1:A1`.
    pub inside: String,
    /// What the outside (bottom, field) side is wired to.
    pub outside: String,
    pub potential: String,
    /// Where the terminal is drawn, e.g. `1.6`.
    pub location: String,
}

/// Every terminal strip, sorted by tag, each in terminal order, with what both sides connect to.
pub fn terminal_diagram(project: &Project) -> Vec<TerminalRow> {
    let potentials = project.potentials();
    let mut strips: Vec<&ecad_model::Device> =
        project.devices.values().filter(|d| !project.terminals_of(d.id).is_empty()).collect();
    strips.sort_by(|a, b| a.tag.cmp(&b.tag));
    let mut rows = Vec::new();
    for strip in strips {
        for t in project.terminals_of(strip.id) {
            let Ok(f) = project.function(t.function) else { continue };
            let points: Vec<String> = project
                .symbol(&f.symbol)
                .map(|s| s.connection_points.iter().map(|cp| cp.name.clone()).collect())
                .unwrap_or_default();
            let far = |point: Option<&String>| -> String {
                let Some(point) = point else { return String::new() };
                let here = ecad_model::ConnectionRef::new(f.id, point);
                let Some(c) = project.connection_at(&here) else { return String::new() };
                let other = if c.from == here { &c.to } else { &c.from };
                project.describe(other).map(|(tag, p)| format!("{tag}:{p}")).unwrap_or_default()
            };
            let first = points.first().map(|p| ecad_model::ConnectionRef::new(f.id, p));
            let potential = first
                .and_then(|r| potentials.iter().find(|p| p.points.contains(&r)))
                .and_then(|p| p.name())
                .unwrap_or_default();
            rows.push(TerminalRow {
                strip: strip.tag.clone(),
                terminal: t.number.clone(),
                inside: far(points.first()),
                outside: far(points.get(1)),
                potential,
                location: t.location.clone(),
            });
        }
    }
    rows
}

pub fn terminal_diagram_csv(rows: &[TerminalRow]) -> String {
    let mut out = String::from("strip,terminal,inside,outside,potential,location\n");
    for r in rows {
        let fields = [&r.strip, &r.terminal, &r.inside, &r.outside, &r.potential, &r.location];
        out += &fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",");
        out.push('\n');
    }
    out
}

/// One channel in the PLC I/O list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlcIoRow {
    pub card: String,
    pub channel: String,
    /// `DI`, `DO`, `AI` or `AO`.
    pub kind: String,
    pub address: String,
    pub symbol: String,
    pub comment: String,
    /// What the channel is wired to, e.g. `-B1:14`.
    pub wired_to: String,
    /// Where the channel is drawn, e.g. `3.2`.
    pub location: String,
}

/// Every PLC card, sorted by tag, each in channel order.
pub fn plc_io_list(project: &Project) -> Vec<PlcIoRow> {
    let mut cards: Vec<&ecad_model::Device> =
        project.devices.values().filter(|d| !project.plc_channels_of(d.id).is_empty()).collect();
    cards.sort_by(|a, b| a.tag.cmp(&b.tag));
    let mut rows = Vec::new();
    for card in cards {
        for c in project.plc_channels_of(card.id) {
            let wired_to = project
                .function(c.function)
                .and_then(|f| project.symbol(&f.symbol))
                .ok()
                .and_then(|s| s.connection_points.first())
                .and_then(|cp| {
                    let here = ecad_model::ConnectionRef::new(c.function, &cp.name);
                    let conn = project.connection_at(&here)?;
                    let other = if conn.from == here { &conn.to } else { &conn.from };
                    project.describe(other).ok().map(|(tag, p)| format!("{tag}:{p}"))
                })
                .unwrap_or_default();
            rows.push(PlcIoRow {
                card: card.tag.clone(),
                channel: c.channel,
                kind: c.kind.short().to_owned(),
                address: c.address,
                symbol: c.symbol,
                comment: c.comment,
                wired_to,
                location: c.location,
            });
        }
    }
    rows
}

pub fn plc_io_list_csv(rows: &[PlcIoRow]) -> String {
    let mut out = String::from("card,channel,kind,address,symbol,comment,wired_to,location\n");
    for r in rows {
        let fields = [&r.card, &r.channel, &r.kind, &r.address, &r.symbol, &r.comment, &r.wired_to, &r.location];
        out += &fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",");
        out.push('\n');
    }
    out
}

/// One core of a cable in the cable list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CableRow {
    pub cable: String,
    pub kind: String,
    pub cores: u32,
    pub shielded: bool,
    pub core: String,
    pub wire: String,
    pub from: String,
    pub to: String,
}

/// Every cable, sorted by tag, with one row per used core (in core order); a cable with no
/// wires yet gets one row with an empty core.
pub fn cable_list(project: &Project) -> Vec<CableRow> {
    let mut cables: Vec<&ecad_model::Cable> = project.cables.values().collect();
    cables.sort_by(|a, b| a.tag.cmp(&b.tag));
    let end = |r| project.describe(r).map(|(t, p)| format!("{t}:{p}")).unwrap_or_default();
    let mut rows = Vec::new();
    for cable in cables {
        let mut cores: Vec<(&str, &ecad_model::Connection)> = project
            .connections
            .values()
            .filter_map(|c| c.spec.cable.as_ref().filter(|k| k.cable == cable.id).map(|k| (k.core.as_str(), c)))
            .collect();
        cores.sort_by(|a, b| natural(a.0, b.0));
        let row = |core: &str, wire: String, from: String, to: String| CableRow {
            cable: cable.tag.clone(),
            kind: cable.kind.clone(),
            cores: cable.cores,
            shielded: cable.shielded,
            core: core.to_owned(),
            wire,
            from,
            to,
        };
        if cores.is_empty() {
            rows.push(row("", String::new(), String::new(), String::new()));
        }
        for (core, c) in cores {
            rows.push(row(core, c.wire_number.clone().unwrap_or_default(), end(&c.from), end(&c.to)));
        }
    }
    rows
}

/// Numbers as numbers, then text.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        _ => a.cmp(b),
    }
}

pub fn cable_list_csv(rows: &[CableRow]) -> String {
    let mut out = String::from("cable,type,cores,shield,core,wire,from,to\n");
    for r in rows {
        let fields = [
            r.cable.clone(),
            r.kind.clone(),
            r.cores.to_string(),
            if r.shielded { "yes".into() } else { String::new() },
            r.core.clone(),
            r.wire.clone(),
            r.from.clone(),
            r.to.clone(),
        ];
        out += &fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",");
        out.push('\n');
    }
    out
}

/// The table of contents: every page in order with its title lines.
pub fn table_of_contents_csv(project: &Project) -> String {
    let mut out = String::from("page,description,subtitle\n");
    for p in project.pages_in_order() {
        let fields = [&p.name, &p.description, &p.subtitle];
        out += &fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",");
        out.push('\n');
    }
    out
}

/// One length to cut for a layout page: a rail or a duct.
#[derive(Clone, Debug, PartialEq)]
pub struct CutRow {
    pub page: String,
    /// `rail` or `duct 60 x 80`.
    pub item: String,
    pub part: String,
    pub length: f64,
}

/// The rails and ducts of every layout page, page by page from the top of each plate.
pub fn panel_cut_list(project: &Project) -> Vec<CutRow> {
    let mut out = Vec::new();
    for page in project.pages_in_order() {
        for item in project.panel_items_on(page.id) {
            let what = match &item.kind {
                ecad_model::PanelItemKind::Rail => "rail TS 35".to_owned(),
                ecad_model::PanelItemKind::Duct { width, depth } => format!("duct {width} x {depth}"),
                ecad_model::PanelItemKind::Device { .. } => continue,
            };
            out.push(CutRow { page: page.name.clone(), item: what, part: item.part.clone(), length: item.length });
        }
    }
    out
}

pub fn panel_cut_list_csv(rows: &[CutRow]) -> String {
    let mut out = String::from("page,item,part,length_mm\n");
    for r in rows {
        let length = format!("{}", (r.length * 10.0).round() / 10.0);
        let fields = [&r.page, &r.item, &r.part, &length];
        out += &fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",");
        out.push('\n');
    }
    out
}

/// Wire to buy: the estimated lengths added up by cross-section and colour.
#[derive(Clone, Debug, PartialEq)]
pub struct WireTotalRow {
    pub cross_section: String,
    pub colour: String,
    pub wires: usize,
    /// Metres, from the wires with an estimate.
    pub metres: f64,
    /// Wires of this kind without an estimate (an end isn't on a plate).
    pub not_estimated: usize,
}

/// Wire totals by cross-section and colour, from the estimated lengths. Wires in a cable are
/// left out: their cores come with the cable.
pub fn wire_totals(project: &Project) -> Vec<WireTotalRow> {
    let lengths = project.wire_lengths();
    let mut rows: std::collections::BTreeMap<(String, String), WireTotalRow> = Default::default();
    for c in project.connections.values().filter(|c| c.spec.cable.is_none()) {
        let key = (c.spec.cross_section.clone(), c.spec.colour.clone());
        let row = rows.entry(key.clone()).or_insert(WireTotalRow {
            cross_section: key.0,
            colour: key.1,
            wires: 0,
            metres: 0.0,
            not_estimated: 0,
        });
        row.wires += 1;
        match lengths.get(&c.id) {
            Some(l) => row.metres += l.mm / 1000.0,
            None => row.not_estimated += 1,
        }
    }
    rows.into_values().collect()
}

pub fn wire_totals_csv(rows: &[WireTotalRow]) -> String {
    let mut out = String::from("cross_section,colour,wires,metres,not_estimated\n");
    for r in rows {
        let fields = [
            r.cross_section.clone(),
            r.colour.clone(),
            r.wires.to_string(),
            format!("{:.1}", r.metres),
            r.not_estimated.to_string(),
        ];
        out += &fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",");
        out.push('\n');
    }
    out
}

/// Report names, as `ecad report` takes them and as their file names (`<name>.csv`).
pub const REPORTS: [&str; 9] = [
    "wire-list",
    "bom",
    "terminal-diagram",
    "plc-io-list",
    "cable-list",
    "table-of-contents",
    "devices-without-parts",
    "panel-cut-list",
    "wire-totals",
];

/// One report as CSV, by name (see [`REPORTS`]); `None` for an unknown name. A report with no
/// rows is just its header.
pub fn csv(project: &Project, name: &str) -> Result<Option<String>, ModelError> {
    Ok(Some(match name {
        "wire-list" => wire_list_csv(&wire_list(project)?),
        "bom" => bom_csv(&bom(project)),
        "terminal-diagram" => terminal_diagram_csv(&terminal_diagram(project)),
        "plc-io-list" => plc_io_list_csv(&plc_io_list(project)),
        "cable-list" => cable_list_csv(&cable_list(project)),
        "table-of-contents" => table_of_contents_csv(project),
        "devices-without-parts" => devices_without_parts_csv(&devices_without_parts(project)),
        "panel-cut-list" => panel_cut_list_csv(&panel_cut_list(project)),
        "wire-totals" => wire_totals_csv(&wire_totals(project)),
        _ => return Ok(None),
    }))
}

/// Every report, as (file name, contents), for writing into an output folder. The list of
/// devices without parts is only included when there are some.
pub fn all_csv(project: &Project) -> Result<Vec<(&'static str, String)>, ModelError> {
    let mut files = vec![("wire-list.csv", wire_list_csv(&wire_list(project)?)), ("bom.csv", bom_csv(&bom(project)))];
    let terminals = terminal_diagram(project);
    if !terminals.is_empty() {
        files.push(("terminal-diagram.csv", terminal_diagram_csv(&terminals)));
    }
    let channels = plc_io_list(project);
    if !channels.is_empty() {
        files.push(("plc-io-list.csv", plc_io_list_csv(&channels)));
    }
    let cables = cable_list(project);
    if !cables.is_empty() {
        files.push(("cable-list.csv", cable_list_csv(&cables)));
    }
    files.push(("table-of-contents.csv", table_of_contents_csv(project)));
    let missing = devices_without_parts(project);
    if !missing.is_empty() {
        files.push(("devices-without-parts.csv", devices_without_parts_csv(&missing)));
    }
    let cuts = panel_cut_list(project);
    if !cuts.is_empty() {
        files.push(("panel-cut-list.csv", panel_cut_list_csv(&cuts)));
    }
    // Totals only mean something once wires have lengths, i.e. with a panel layout.
    if !project.wire_lengths().is_empty() {
        files.push(("wire-totals.csv", wire_totals_csv(&wire_totals(project))));
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_bom_adds_up_quantities_and_lists_devices() {
        use ecad_model::{Command, DeviceId, PartRef, sample::with_potentials};
        let mut p = with_potentials().unwrap();
        // -K2 gets the same contactor; -K1 also gets two auxiliary blocks.
        let k1 = DeviceId::from_u128(0x103);
        let k2 = DeviceId::from_u128(0x108);
        p.apply(Command::SetDeviceParts { id: k2, parts: vec![PartRef::new("EX-CT-24DC", 1)] }).unwrap();
        let k1_parts = vec![PartRef::new("EX-CT-24DC", 1), PartRef::new("EX-AUX-1NO1NC", 2)];
        p.apply(Command::SetDeviceParts { id: k1, parts: k1_parts }).unwrap();
        assert_eq!(
            bom_csv(&bom(&p)),
            "part_number,manufacturer,description,quantity,devices,code,pages\n\
             EX-AUX-1NO1NC,Example,Auxiliary contact block 1 NO + 1 NC,2,-K1,,1 2\n\
             EX-CT-24DC,Example,\"Contactor 3 kW, 24 V DC coil, 1 NO auxiliary\",2,-K1 -K2,,1 2\n\
             EX-PB-GRN,Example,\"Pushbutton, green, 1 NO\",1,-S1,,1\n"
        );
        // L+, M, I1 and J1 aren't components; everything else has a part now.
        assert!(devices_without_parts(&p).is_empty());
        p.apply(Command::SetDeviceParts { id: k2, parts: vec![] }).unwrap();
        assert_eq!(devices_without_parts(&p), [("-K2".to_owned(), String::new())]);
        let names: Vec<&str> = all_csv(&p).unwrap().iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["wire-list.csv", "bom.csv", "table-of-contents.csv", "devices-without-parts.csv"]);
    }

    use super::*;
    use ecad_model::sample::start_circuit;

    #[test]
    fn wire_list_of_the_sample() {
        let csv = wire_list_csv(&wire_list(&start_circuit().unwrap()).unwrap());
        assert_eq!(
            csv,
            "wire,potential,from_device,from_point,to_device,to_point,cross_section,colour,cable,core,length_mm,route\n\
             W1,L+,L+,1,-S1,13,,,,,,\n\
             W2,,-S1,14,-K1,A1,,,,,,\n\
             W3,M,-K1,A2,M,1,,,,,,\n"
        );
    }

    #[test]
    fn the_reference_wires_get_lengths_and_add_up_to_totals() {
        let p = ecad_model::reference::reference_project().unwrap();
        let rows = wire_list(&p).unwrap();
        let p201 = rows.iter().find(|r| r.wire == "P201" && r.from_tag == "P2QS1").unwrap();
        assert_eq!(p201.route, "ducts");
        assert!(p201.length.parse::<f64>().unwrap() >= 300.0);
        // A motor's wires leave the cabinet: no estimate.
        assert!(rows.iter().filter(|r| r.to_tag == "M3").all(|r| r.length.is_empty()));
        // Totals are the wire list's lengths added up, in metres; wires in cables are left out.
        let totals = wire_totals(&p);
        let listed: f64 =
            rows.iter().filter(|r| r.cable.is_empty()).filter_map(|r| r.length.parse::<f64>().ok()).sum::<f64>()
                / 1000.0;
        let summed: f64 = totals.iter().map(|t| t.metres).sum();
        assert!((listed - summed).abs() < 0.01, "{listed} {summed}");
        let names: Vec<&str> = all_csv(&p).unwrap().iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"panel-cut-list.csv") && names.contains(&"wire-totals.csv"), "{names:?}");
    }

    #[test]
    fn the_terminal_diagram_lists_both_sides_and_the_bom_counts_terminals() {
        let p = ecad_model::sample::with_terminals().unwrap();
        let rows = terminal_diagram(&p);
        let csv = terminal_diagram_csv(&rows);
        let col = |f| p.sheet.column_of(p.function(f).unwrap().position);
        assert_eq!(
            csv,
            format!(
                "strip,terminal,inside,outside,potential,location\n\
                 -X2,1,L+:1,-S1:13,L+,1.{}\n\
                 -X2,2,-S1:14,-K1:A1,,1.{}\n",
                col(ecad_model::sample::X2_1),
                col(ecad_model::sample::X2_2)
            )
        );
        // One terminal part on the strip, two terminals: two in the BOM.
        let tb = bom(&p).into_iter().find(|r| r.number == "EX-TB-4").unwrap();
        assert_eq!((tb.quantity, tb.devices), (2, vec!["-X2".to_owned()]));
        let names: Vec<&str> = all_csv(&p).unwrap().iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"terminal-diagram.csv"), "{names:?}");

        // A PE terminal on the same strip: parts can count per kind, or once for the strip.
        let mut p = p;
        let strip = p.function(ecad_model::sample::X2_1).unwrap().device;
        let symbol = ecad_model::SymbolId::new("iec.terminal_pe");
        let designations = p.next_designations(strip, &symbol);
        let page = p.function(ecad_model::sample::X2_1).unwrap().page;
        p.place(ecad_model::Function {
            id: ecad_model::FunctionId::new(),
            device: strip,
            symbol,
            page,
            position: ecad_model::Point::new(80.0, 80.0),
            rotation: Default::default(),
            designations,
            properties: Default::default(),
            mirrored: false,
        })
        .unwrap();
        for number in ["EX-PE-4", "EX-END"] {
            p.apply(ecad_model::Command::AddPart(ecad_model::Part {
                number: number.into(),
                manufacturer: String::new(),
                description: String::new(),
                code: String::new(),
                contacts: Vec::new(),
                width: None,
                height: None,
                depth: None,
            }))
            .unwrap();
        }
        let parts = vec![
            ecad_model::PartRef::per_terminal_of("EX-TB-4", 1, "iec.terminal"),
            ecad_model::PartRef::per_terminal_of("EX-PE-4", 1, "iec.terminal_pe"),
            ecad_model::PartRef::per_strip("EX-END", 2),
        ];
        p.apply(ecad_model::Command::SetDeviceParts { id: strip, parts }).unwrap();
        let count = |n: &str| bom(&p).into_iter().find(|r| r.number == n).map(|r| r.quantity);
        assert_eq!((count("EX-TB-4"), count("EX-PE-4"), count("EX-END")), (Some(2), Some(1), Some(2)));
    }

    #[test]
    fn the_plc_io_list_shows_each_channel_with_address_symbol_and_wiring() {
        let p = ecad_model::sample::with_plc().unwrap();
        let rows = plc_io_list(&p);
        let summary: Vec<String> = rows
            .iter()
            .map(|r| {
                format!("{}:{} {} {} {} [{}] {}", r.card, r.channel, r.kind, r.address, r.symbol, r.comment, r.wired_to)
            })
            .collect();
        assert_eq!(
            summary,
            [
                "-KF1:0 DI I0.0 Door_Closed [Guard door closed] -B1:14",
                "-KF1:1 DI I0.1 Start_Spare [Start button, NC contact] -S1:22",
                "-KF1:2 DO Q0.0 Pump_On [] -K3:A1",
                "-KF1:3 AI IW256 Tank_Level [Tank level 4-20 mA] ",
            ]
        );
        assert!(rows.iter().all(|r| r.location.starts_with("3/")), "{rows:?}");
        let names: Vec<&str> = all_csv(&p).unwrap().iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"plc-io-list.csv"), "{names:?}");
        assert!(
            plc_io_list_csv(&rows)
                .starts_with("card,channel,kind,address,symbol,comment,wired_to,location\n-KF1,0,DI,I0.0")
        );
    }

    #[test]
    fn the_cable_list_has_a_row_per_core_and_the_bom_its_code_and_pages() {
        let p = ecad_model::sample::with_power().unwrap();
        let csv = cable_list_csv(&cable_list(&p));
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "cable,type,cores,shield,core,wire,from,to");
        assert_eq!(lines.len(), 4, "{csv}");
        assert_eq!(lines[1], "-WM1,\"4G2,5\",4,yes,1,407,-K1:2,-M1:U");
        let wires = wire_list(&p).unwrap();
        let u = wires.iter().find(|w| w.to_tag == "-M1" && w.to_point == "U").unwrap();
        assert_eq!(
            (u.cross_section.as_str(), u.colour.as_str(), u.cable.as_str(), u.core.as_str()),
            ("2,5", "BK", "-WM1", "1")
        );
        let toc = table_of_contents_csv(&p);
        assert!(toc.contains("\n4,Power distribution,Distribuzione potenza\n"), "{toc}");
        let names: Vec<&str> = all_csv(&p).unwrap().iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"cable-list.csv") && names.contains(&"table-of-contents.csv"));
    }

    #[test]
    fn csv_quotes_awkward_fields() {
        assert_eq!(csv_field("=A1+B2-K1"), "=A1+B2-K1");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }
}
