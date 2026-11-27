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

/// Hex SHA-256 of any value's canonical JSON.
pub fn sha256_json<T: serde::Serialize>(value: &T) -> Result<String> {
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
}
