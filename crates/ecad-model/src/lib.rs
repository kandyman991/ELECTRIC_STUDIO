//! The electrical project model: the single source of truth for Electric Studio.
//!
//! Drawings, reports and exports are all derived from this model. This crate must never depend
//! on CADCraft or on any UI crate (see AGENTS.md).

pub mod geometry;
pub mod project;
pub mod sample;
pub mod symbol;

pub use geometry::{Direction, Point, Rotation};
pub use project::*;
pub use symbol::*;

#[cfg(test)]
mod tests {
    use super::*;
    use sample::{K1_COIL, PAGE_1, start_circuit};

    fn assert_routes_land_on_terminals(p: &Project) {
        for c in &p.connections {
            let route = p.route(c).unwrap();
            assert_eq!(
                route[0],
                p.connection_point(&c.from).unwrap().0,
                "{:?}",
                c.wire_number
            );
            assert_eq!(*route.last().unwrap(), p.connection_point(&c.to).unwrap().0);
            for seg in route.windows(2) {
                assert!(
                    seg[0].x == seg[1].x || seg[0].y == seg[1].y,
                    "segment not orthogonal: {seg:?}"
                );
            }
        }
    }

    #[test]
    fn wires_follow_a_moved_function() {
        let mut p = start_circuit().unwrap();
        assert_routes_land_on_terminals(&p);
        p.move_function(K1_COIL, Point::new(40.0, 15.0)).unwrap();
        assert_routes_land_on_terminals(&p);
        assert_eq!(
            p.connection_point(&ConnectionRef::new(K1_COIL, "A1"))
                .unwrap()
                .0,
            Point::new(40.0, 23.0)
        );
    }

    #[test]
    fn wires_follow_a_rotated_function() {
        let mut p = start_circuit().unwrap();
        p.functions
            .iter_mut()
            .find(|f| f.id == K1_COIL)
            .unwrap()
            .rotation = Rotation::R90;
        let (a1, dir) = p
            .connection_point(&ConnectionRef::new(K1_COIL, "A1"))
            .unwrap();
        assert_eq!((a1, dir), (Point::new(-8.0, 10.0), Direction::Left));
        assert_routes_land_on_terminals(&p);
    }

    #[test]
    fn routes_leave_terminals_outward_and_never_cross_their_symbol() {
        let mut p = start_circuit().unwrap();
        p.move_function(K1_COIL, Point::new(40.0, 15.0)).unwrap();
        p.functions
            .iter_mut()
            .find(|f| f.id == K1_COIL)
            .unwrap()
            .rotation = Rotation::R90;
        // The rotated coil's box spans x 37..43, y 10..20; A2 faces right at (48, 15).
        for c in &p.connections {
            let route = p.route(c).unwrap();
            for seg in route.windows(2) {
                let (x0, x1) = (seg[0].x.min(seg[1].x), seg[0].x.max(seg[1].x));
                let (y0, y1) = (seg[0].y.min(seg[1].y), seg[0].y.max(seg[1].y));
                let crosses_box = x0 < 43.0 && x1 > 37.0 && y0 < 20.0 && y1 > 10.0;
                assert!(
                    !crosses_box,
                    "{:?} crosses the coil: {route:?}",
                    c.wire_number
                );
            }
        }
        let w3 = p
            .connections
            .iter()
            .find(|c| c.wire_number.as_deref() == Some("W3"))
            .unwrap();
        let route = p.route(w3).unwrap();
        assert_eq!(
            route[1],
            Point::new(48.0 + project::ROUTE_STUB, 15.0),
            "leaves A2 to the right first"
        );
    }

    #[test]
    fn aligned_facing_points_get_a_straight_wire() {
        let p = start_circuit().unwrap();
        assert!(p.connections.iter().all(|c| p.route(c).unwrap().len() == 2));
    }

    #[test]
    fn a_connection_point_takes_one_wire() {
        let mut p = start_circuit().unwrap();
        let err = p
            .connect(Connection {
                id: ConnectionId::new(),
                from: ConnectionRef::new(K1_COIL, "A1"),
                to: ConnectionRef::new(sample::K1_COIL, "A2"),
                wire_number: None,
            })
            .unwrap_err();
        assert_eq!(
            err,
            ModelError::AlreadyConnected(("-K1".into(), "A1".into()), ("-S1".into(), "14".into()))
        );
    }

    #[test]
    fn unknown_points_and_tags_are_rejected() {
        let mut p = start_circuit().unwrap();
        let err = p
            .connect(Connection {
                id: ConnectionId::new(),
                from: ConnectionRef::new(K1_COIL, "A3"),
                to: ConnectionRef::new(K1_COIL, "A1"),
                wire_number: None,
            })
            .unwrap_err();
        assert!(matches!(err, ModelError::NoConnectionPoint { .. }));
        let dup = p.add_device(Device {
            id: DeviceId::new(),
            tag: "-K1".into(),
            description: String::new(),
        });
        assert_eq!(dup.unwrap_err(), ModelError::DuplicateTag("-K1".into()));
        assert_eq!(p.connections_on(PAGE_1).count(), 3);
    }
}
