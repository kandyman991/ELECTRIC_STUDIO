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
}

/// One row per connection, sorted by wire number.
pub fn wire_list(project: &Project) -> Result<Vec<WireListRow>, ModelError> {
    let potentials = project.potentials();
    project
        .connections
        .values()
        .map(|c| {
            let (from_tag, from_point) = project.describe(&c.from)?;
            let (to_tag, to_point) = project.describe(&c.to)?;
            let potential =
                potentials.iter().find(|p| p.connections.contains(&c.id)).and_then(|p| p.name()).unwrap_or_default();
            Ok(WireListRow {
                wire: c.wire_number.clone().unwrap_or_default(),
                potential,
                from_tag,
                from_point,
                to_tag,
                to_point,
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
    let mut out = String::from("wire,potential,from_device,from_point,to_device,to_point\n");
    for r in rows {
        let fields = [&r.wire, &r.potential, &r.from_tag, &r.from_point, &r.to_tag, &r.to_point];
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
}

/// The bill of materials: every part used by a device, with the total quantity, sorted by part
/// number. Parts of a terminal strip count once per terminal.
pub fn bom(project: &Project) -> Vec<BomRow> {
    let mut rows: std::collections::BTreeMap<&str, BomRow> = std::collections::BTreeMap::new();
    for d in project.devices.values() {
        let terminals = project.terminals_of(d.id).len() as u32;
        let per = terminals.max(1);
        for r in &d.parts {
            let part = project.parts.get(&r.number);
            let row = rows.entry(&r.number).or_insert_with(|| BomRow {
                number: r.number.clone(),
                manufacturer: part.map(|p| p.manufacturer.clone()).unwrap_or_default(),
                description: part.map(|p| p.description.clone()).unwrap_or_default(),
                quantity: 0,
                devices: Vec::new(),
            });
            row.quantity += r.quantity * per;
            row.devices.push(d.tag.clone());
        }
    }
    rows.into_values()
        .map(|mut r| {
            r.devices.sort();
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
    let mut out = String::from("part_number,manufacturer,description,quantity,devices\n");
    for r in rows {
        let fields = [
            r.number.clone(),
            r.manufacturer.clone(),
            r.description.clone(),
            r.quantity.to_string(),
            r.devices.join(" "),
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

/// Report names, as `ecad report` takes them and as their file names (`<name>.csv`).
pub const REPORTS: [&str; 5] = ["wire-list", "bom", "terminal-diagram", "plc-io-list", "devices-without-parts"];

/// One report as CSV, by name (see [`REPORTS`]); `None` for an unknown name. A report with no
/// rows is just its header.
pub fn csv(project: &Project, name: &str) -> Result<Option<String>, ModelError> {
    Ok(Some(match name {
        "wire-list" => wire_list_csv(&wire_list(project)?),
        "bom" => bom_csv(&bom(project)),
        "terminal-diagram" => terminal_diagram_csv(&terminal_diagram(project)),
        "plc-io-list" => plc_io_list_csv(&plc_io_list(project)),
        "devices-without-parts" => devices_without_parts_csv(&devices_without_parts(project)),
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
    let missing = devices_without_parts(project);
    if !missing.is_empty() {
        files.push(("devices-without-parts.csv", devices_without_parts_csv(&missing)));
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
            "part_number,manufacturer,description,quantity,devices\n\
             EX-AUX-1NO1NC,Example,Auxiliary contact block 1 NO + 1 NC,2,-K1\n\
             EX-CT-24DC,Example,\"Contactor 3 kW, 24 V DC coil, 1 NO auxiliary\",2,-K1 -K2\n\
             EX-PB-GRN,Example,\"Pushbutton, green, 1 NO\",1,-S1\n"
        );
        // L+, M, I1 and J1 aren't components; everything else has a part now.
        assert!(devices_without_parts(&p).is_empty());
        p.apply(Command::SetDeviceParts { id: k2, parts: vec![] }).unwrap();
        assert_eq!(devices_without_parts(&p), [("-K2".to_owned(), String::new())]);
        let names: Vec<&str> = all_csv(&p).unwrap().iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["wire-list.csv", "bom.csv", "devices-without-parts.csv"]);
    }

    use super::*;
    use ecad_model::sample::start_circuit;

    #[test]
    fn wire_list_of_the_sample() {
        let csv = wire_list_csv(&wire_list(&start_circuit().unwrap()).unwrap());
        assert_eq!(
            csv,
            "wire,potential,from_device,from_point,to_device,to_point\n\
             W1,L+,L+,1,-S1,13\n\
             W2,,-S1,14,-K1,A1\n\
             W3,M,-K1,A2,M,1\n"
        );
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
        assert!(rows.iter().all(|r| r.location.starts_with("3.")), "{rows:?}");
        let names: Vec<&str> = all_csv(&p).unwrap().iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"plc-io-list.csv"), "{names:?}");
        assert!(
            plc_io_list_csv(&rows)
                .starts_with("card,channel,kind,address,symbol,comment,wired_to,location\n-KF1,0,DI,I0.0")
        );
    }

    #[test]
    fn csv_quotes_awkward_fields() {
        assert_eq!(csv_field("=A1+B2-K1"), "=A1+B2-K1");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }
}
