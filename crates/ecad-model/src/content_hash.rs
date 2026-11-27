//! The stable content hash of a project's engineering and graphical state.
//!
//! SHA-256 over canonical JSON: the project as serialised, with every map's keys sorted, plus
//! its underlays. It is the same for the same content on every run, machine and program
//! version that keeps the same serialisation, unlike the folder fingerprint, which uses Rust's
//! `DefaultHasher` and is only for telling whether a folder changed during a session.
//!
//! It is the engineering-equality hash of docs/PLATFORM-ARCHITECTURE.md (F06): audit records,
//! approvals and transactional metadata will be kept out of it as they arrive.

use crate::project::{ModelError, Project, Result};
use sha2::{Digest, Sha256};

/// Bumped if what goes into the hash changes, so old and new hashes never compare equal by
/// accident.
const DOMAIN: &[u8] = b"electric-studio/content/1\n";

/// Hex SHA-256 of any value's canonical JSON. A value with a NaN or infinite number is
/// refused: JSON would write it as `null`, so different invalid states would hash alike.
pub fn sha256_json<T: serde::Serialize>(value: &T) -> Result<String> {
    crate::finite::check(value).map_err(|e| ModelError::NotFinite(e.0))?;
    // Through `Value`, whose maps are sorted (serde_json without `preserve_order`).
    let canonical = serde_json::to_value(value).map_err(|e| ModelError::Hash(e.to_string()))?;
    let bytes = serde_json::to_vec(&canonical).map_err(|e| ModelError::Hash(e.to_string()))?;
    Ok(hex(&Sha256::digest(bytes)))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Project {
    /// The project's content hash, as `sha256:<hex>`.
    pub fn content_hash(&self) -> Result<String> {
        let mut h = Sha256::new();
        h.update(DOMAIN);
        h.update(sha256_json(self)?.as_bytes());
        h.update(b"\n");
        // Underlays aren't part of the project's own serialisation (they are saved in files of
        // their own), but they are stored graphical state.
        h.update(sha256_json(&self.underlays)?.as_bytes());
        Ok(format!("sha256:{}", hex(&h.finalize())))
    }
}

#[cfg(test)]
mod tests {
    use crate::sample;
    use crate::*;

    #[test]
    fn the_hash_is_stable_and_follows_the_content() {
        let p = sample::with_power().unwrap();
        let h = p.content_hash().unwrap();
        assert!(h.starts_with("sha256:") && h.len() == 7 + 64);
        assert_eq!(h, p.clone().content_hash().unwrap());
        assert_eq!(h, sample::with_power().unwrap().content_hash().unwrap(), "rebuilt the same way");
        let mut q = p.clone();
        q.apply(Command::SetDescription { id: DeviceId::from_u128(0x103), description: "changed".into() }).unwrap();
        assert_ne!(q.content_hash().unwrap(), h);
        // Undo restores the content, and so the hash.
        let mut r = p.clone();
        let undo =
            r.apply(Command::SetDescription { id: DeviceId::from_u128(0x103), description: "x".into() }).unwrap();
        r.apply(undo).unwrap();
        assert_eq!(r.content_hash().unwrap(), h);
    }

    #[test]
    fn non_finite_numbers_are_refused_where_they_enter_and_by_the_hash() {
        let mut p = sample::start_circuit().unwrap();
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let to = Point::new(bad, 0.0);
            assert!(matches!(p.apply(Command::Move { id: sample::K1_COIL, to }), Err(ModelError::NotFinite(_))));
        }
        let mut f = p.function(sample::K1_COIL).unwrap().clone();
        f.id = FunctionId::new();
        f.position = Point::new(0.0, f64::NAN);
        assert!(matches!(p.apply(Command::Place(f)), Err(ModelError::NotFinite(_))));
        let b = LocationBox {
            id: BoxId::new(),
            page: sample::PAGE_1,
            min: Point::new(f64::NAN, 0.0),
            max: Point::new(10.0, 10.0),
            label: String::new(),
        };
        assert!(matches!(p.apply(Command::AddBox(b)), Err(ModelError::NotFinite(_))));
        let mut sheet = p.sheet;
        sheet.width = f64::INFINITY;
        assert!(matches!(p.apply(Command::SetSheet(sheet)), Err(ModelError::NotFinite(_))));
        let part = Part { width: Some(f64::NAN), ..p.parts.values().next().unwrap().clone() };
        assert!(matches!(p.apply(Command::UpdatePart(part)), Err(ModelError::NotFinite(_))));
        // Nothing got in, so the hash is still computed; state set behind the commands' back
        // with a NaN is refused by the hash rather than hashed as `null`.
        assert!(p.content_hash().is_ok());
        let mut q = p.clone();
        if let Some(f) = q.functions.get_mut(&sample::K1_COIL) {
            f.position.x = f64::NAN;
        }
        let e = q.content_hash().unwrap_err().to_string();
        assert!(e.contains("NaN at functions.") && e.contains("position.x"), "{e}");
    }

    #[test]
    fn the_hash_covers_underlays() {
        use crate::underlay::{Underlay, UnderlayShapes};
        let mut p = sample::start_circuit().unwrap();
        let h = p.content_hash().unwrap();
        let shapes = UnderlayShapes {
            lines: vec![vec![Point::new(0.0, 0.0), Point::new(100.0, 50.0)]],
            fills: Vec::new(),
            width: 100.0,
            height: 50.0,
        };
        let u = Underlay {
            source: "plan.dxf".into(),
            shapes: std::sync::Arc::new(shapes),
            at: Point::new(10.0, 10.0),
            scale: 1.0,
            visible: true,
            units: Default::default(),
            units_from: Default::default(),
        };
        p.apply(Command::SetUnderlay { page: sample::PAGE_1, underlay: Some(Box::new(u.clone())) }).unwrap();
        let with = p.content_hash().unwrap();
        assert_ne!(with, h);
        let moved = Underlay { at: Point::new(11.0, 10.0), ..u };
        p.apply(Command::SetUnderlay { page: sample::PAGE_1, underlay: Some(Box::new(moved)) }).unwrap();
        assert_ne!(p.content_hash().unwrap(), with, "moving the underlay changes the hash");
    }
}
