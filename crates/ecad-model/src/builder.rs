//! Building a project in code, page by page, refering to devices by tag and to points by
//! designation: used by the reference project and the conveyor acceptance project.

use crate::geometry::{Point, Rotation};
use crate::library::{BlackBox, black_box};
use crate::project::*;
use crate::symbol::SymbolId;
use std::collections::BTreeMap;

/// Builds the project page by page, refering to devices by tag and to points by designation.
pub(crate) struct Builder {
    pub(crate) p: Project,
    pub(crate) page: PageId,
    pub(crate) next: u128,
}

pub(crate) type F = FunctionId;

impl Builder {
    pub(crate) fn id(&mut self) -> u128 {
        self.next += 1;
        self.next
    }

    pub(crate) fn page(&mut self, name: &str, description: &str, subtitle: &str) -> Result<()> {
        let order = self.p.pages.len() as u32;
        let id = PageId::from_u128(0x100 + u128::from(order));
        self.p.apply(Command::AddPage(Page {
            id,
            name: name.into(),
            description: description.into(),
            subtitle: subtitle.into(),
            // Sparse keys, as format 2 stores them, so a page can be inserted between two
            // without touching either (D25).
            order: crate::PAGE_STEP * (u64::from(order) + 1),
        }))?;
        self.page = id;
        Ok(())
    }

    pub(crate) fn device(&mut self, tag: &str) -> Result<DeviceId> {
        if let Some(d) = self.p.device_by_tag(tag) {
            return Ok(d.id);
        }
        let id = DeviceId::from_u128(self.id());
        self.p.add_device(Device::new(id, tag, ""))?;
        Ok(id)
    }

    pub(crate) fn describe(&mut self, tag: &str, description: &str) -> Result<()> {
        // A potential is a node, described as one.
        if let Ok(n) = self.p.node_by_name(crate::NodeKind::Potential, tag, crate::PROJECT_SCOPE) {
            let node = crate::NodeEntry { description: description.into(), ..n.clone() };
            return self.p.apply(Command::SetNode(node)).map(drop);
        }
        let id = self.device(tag)?;
        self.p.apply(Command::SetDescription { id, description: description.into() }).map(drop)
    }

    /// A drawing of `tag` with `symbol` at (x, y), numbered as the model numbers it.
    pub(crate) fn put_r(&mut self, tag: &str, symbol: &str, x: f64, y: f64, rotation: Rotation) -> Result<F> {
        let symbol = SymbolId::new(symbol);
        if let Some(kind) = crate::connectivity::node_kind(self.p.symbol(&symbol)?) {
            return self.put_node(kind, tag, symbol, x, y, rotation);
        }
        let device = self.device(tag)?;
        let designations = self.p.next_designations(device, &symbol);
        let properties = self.p.new_drawing_properties(&symbol);
        self.place(device, symbol, x, y, rotation, designations, properties)
    }

    /// A drawing of a node: a potential or interruption found by its name (in the project
    /// scope) or new, or a new junction. Ids are drawn from the counter as format 1's devices
    /// took them, so the nodes get the ids a format 1 conversion gives them.
    fn put_node(
        &mut self,
        kind: crate::NodeKind,
        name: &str,
        symbol: SymbolId,
        x: f64,
        y: f64,
        rotation: Rotation,
    ) -> Result<F> {
        let node = match kind {
            // The junction keeps the name its device had (`J<n>`).
            crate::NodeKind::Junction => {
                self.id();
                None
            }
            _ => match self.p.node_by_name(kind, name, crate::PROJECT_SCOPE) {
                Ok(n) => Some(n.id),
                Err(_) => {
                    let n = crate::NodeId::for_device(DeviceId::from_u128(self.id()));
                    self.p.apply(Command::AddNode(crate::NodeEntry::new(n, kind, name)))?;
                    Some(n)
                }
            },
        };
        let id = F::from_u128(self.id());
        let node = match node {
            Some(n) => n,
            None => {
                let n = crate::NodeId::for_junction(id);
                self.p.apply(Command::AddNode(crate::NodeEntry::new(n, kind, name)))?;
                n
            }
        };
        self.p.place(Function {
            node: Some(node),
            terminals: Default::default(),
            id,
            device: DeviceId::NONE,
            symbol,
            page: self.page,
            position: Point::new(x, y),
            rotation,
            designations: BTreeMap::new(),
            properties: BTreeMap::new(),
            mirrored: false,
        })?;
        Ok(id)
    }

    pub(crate) fn put(&mut self, tag: &str, symbol: &str, x: f64, y: f64) -> Result<F> {
        self.put_r(tag, symbol, x, y, Rotation::R0)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn place(
        &mut self,
        device: DeviceId,
        symbol: SymbolId,
        x: f64,
        y: f64,
        rotation: Rotation,
        designations: BTreeMap<String, String>,
        properties: BTreeMap<String, String>,
    ) -> Result<F> {
        let id = F::from_u128(self.id());
        self.p.place(Function {
            node: None,
            terminals: Default::default(),
            id,
            device,
            symbol,
            page: self.page,
            position: Point::new(x, y),
            rotation,
            designations,
            properties,
            mirrored: false,
        })?;
        Ok(id)
    }

    pub(crate) fn terminal(&mut self, strip: &str, symbol: &str, number: &str, x: f64, y: f64) -> Result<F> {
        let device = self.device(strip)?;
        let symbol = SymbolId::new(symbol);
        let designations = [("1".to_owned(), number.to_owned()), ("2".to_owned(), number.to_owned())].into();
        self.place(device, symbol, x, y, Rotation::R0, designations, BTreeMap::new())
    }

    /// A PLC channel with its address, symbol and comment.
    pub(crate) fn channel(&mut self, card: &str, symbol: &str, x: f64, y: f64, address: &str, name: &str) -> Result<F> {
        let f = self.put(card, symbol, x, y)?;
        let properties = [(ADDRESS.to_owned(), address.to_owned()), (SYMBOL.to_owned(), name.to_owned())]
            .into_iter()
            .filter(|(_, v)| !v.is_empty())
            .collect();
        self.p.apply(Command::SetProperties { id: f, properties })?;
        Ok(f)
    }

    pub(crate) fn junction(&mut self, x: f64, y: f64) -> Result<F> {
        let tag = format!("J{}", self.next + 1);
        self.put(&tag, "iec.junction", x, y)
    }

    /// An off-page arrow: `facing` is the direction its wire leaves in.
    pub(crate) fn arrow(&mut self, name: &str, x: f64, y: f64, facing: crate::Direction) -> Result<F> {
        let rotation = match facing {
            crate::Direction::Up => Rotation::R0,
            crate::Direction::Left => Rotation::R90,
            crate::Direction::Down => Rotation::R180,
            crate::Direction::Right => Rotation::R270,
        };
        self.put_r(name, "iec.interruption", x, y, rotation)
    }

    /// The connection point of `f` named or designated `d`.
    pub(crate) fn pin(&self, f: F, d: &str) -> Result<ConnectionRef> {
        let func = self.p.function(f)?;
        let sym = self.p.symbol(&func.symbol)?;
        // A point's own name first (a terminal's two sides share one number), then its
        // designation (a contact numbered 21-22 on a symbol whose points are 11 and 12).
        sym.connection_point(d)
            .or_else(|| sym.connection_points.iter().find(|cp| self.p.designation(func, &cp.name) == d))
            .map(|cp| ConnectionRef::new(f, &cp.name))
            .ok_or_else(|| ModelError::NoConnectionPoint { symbol: func.symbol.0.clone(), point: d.into() })
    }

    pub(crate) fn wire_spec(
        &mut self,
        a: (F, &str),
        b: (F, &str),
        number: Option<&str>,
        spec: WireSpec,
    ) -> Result<ConnectionId> {
        let id = ConnectionId::from_u128(self.id());
        let (from, to) = (self.pin(a.0, a.1)?, self.pin(b.0, b.1)?);
        self.p.connect(Connection { bend: None, id, from, to, wire_number: number.map(str::to_owned), spec })?;
        Ok(id)
    }

    /// Flips a drawing left to right.
    pub(crate) fn mirror(&mut self, f: F) -> Result<()> {
        self.p.apply(Command::Mirror { id: f, to: true }).map(drop)
    }

    /// Puts a wire's middle run at `at` (its y when horizontal, its x when vertical).
    pub(crate) fn bend(&mut self, id: ConnectionId, at: f64) -> Result<()> {
        self.p.apply(Command::SetWireBend { id, bend: Some(at) }).map(drop)
    }

    pub(crate) fn w(&mut self, a: (F, &str), b: (F, &str)) -> Result<ConnectionId> {
        self.wire_spec(a, b, None, WireSpec::default())
    }

    pub(crate) fn wn(&mut self, a: (F, &str), b: (F, &str), number: &str) -> Result<ConnectionId> {
        self.wire_spec(a, b, Some(number), WireSpec::default())
    }

    pub(crate) fn ws(
        &mut self,
        a: (F, &str),
        b: (F, &str),
        number: &str,
        cross_section: &str,
        colour: &str,
    ) -> Result<ConnectionId> {
        let spec = WireSpec { cross_section: cross_section.into(), colour: colour.into(), cable: None };
        self.wire_spec(a, b, Some(number), spec)
    }

    pub(crate) fn cable(&mut self, tag: &str, kind: &str, cores: u32, shielded: bool) -> Result<CableId> {
        let id = CableId::from_u128(self.id());
        self.p.apply(Command::AddCable(Cable { id, tag: tag.into(), kind: kind.into(), cores, shielded }))?;
        Ok(id)
    }

    pub(crate) fn core(&mut self, wire: ConnectionId, cable: CableId, core: &str, cross_section: &str) -> Result<()> {
        let spec = WireSpec {
            cross_section: cross_section.into(),
            colour: String::new(),
            cable: Some(CableCore { cable, core: core.into() }),
        };
        self.p.apply(Command::SetWireSpec { id: wire, spec }).map(drop)
    }

    pub(crate) fn boxed(&mut self, label: &str, min: (f64, f64), max: (f64, f64)) -> Result<()> {
        let id = BoxId::from_u128(self.id());
        let page = self.page;
        let b =
            LocationBox { id, page, min: Point::new(min.0, min.1), max: Point::new(max.0, max.1), label: label.into() };
        self.p.apply(Command::AddBox(b)).map(drop)
    }

    /// A black box device: its own symbol, then one drawing of it.
    pub(crate) fn black(&mut self, tag: &str, spec: BlackBox, x: f64, y: f64) -> Result<F> {
        let id = format!("box.{tag}");
        self.p.apply(Command::AddSymbol(black_box(&id, &spec)))?;
        let f = self.put(tag, &id, x, y)?;
        self.describe(tag, &spec.description)?;
        Ok(f)
    }
}
