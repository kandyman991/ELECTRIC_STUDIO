//! Finding a number that isn't finite (NaN, ±infinity) anywhere in a serialisable value.
//!
//! The model's commands reject non-finite coordinates and sizes where they enter. This is the
//! guard behind them for the content hash: JSON has no NaN or infinity, so `serde_json` writes
//! them as `null`, and two different invalid states would hash alike. Hashing refuses instead.

use serde::ser::{self, Serialize};

#[derive(Debug)]
pub struct NotFinite(pub String);

impl std::fmt::Display for NotFinite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotFinite {}

impl ser::Error for NotFinite {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        NotFinite(msg.to_string())
    }
}

/// `Err` with the path of the first non-finite number in `value` (e.g. `functions.<id>.position.x`).
pub fn check<T: Serialize + ?Sized>(value: &T) -> Result<(), NotFinite> {
    value.serialize(&mut Walker { path: Vec::new() })
}

struct Walker {
    path: Vec<String>,
}

impl Walker {
    fn float(&self, v: f64) -> Result<(), NotFinite> {
        if v.is_finite() { Ok(()) } else { Err(NotFinite(format!("{v} at {}", self.path.join(".")))) }
    }
}

macro_rules! ok {
    ($($name:ident: $t:ty),*) => {
        $(fn $name(self, _: $t) -> Result<(), NotFinite> { Ok(()) })*
    };
}

impl ser::Serializer for &mut Walker {
    type Ok = ();
    type Error = NotFinite;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    ok!(serialize_bool: bool, serialize_i8: i8, serialize_i16: i16, serialize_i32: i32, serialize_i64: i64,
        serialize_u8: u8, serialize_u16: u16, serialize_u32: u32, serialize_u64: u64, serialize_char: char,
        serialize_str: &str, serialize_bytes: &[u8]);

    fn serialize_f32(self, v: f32) -> Result<(), NotFinite> {
        self.float(f64::from(v))
    }
    fn serialize_f64(self, v: f64) -> Result<(), NotFinite> {
        self.float(v)
    }
    fn serialize_none(self) -> Result<(), NotFinite> {
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, v: &T) -> Result<(), NotFinite> {
        v.serialize(self)
    }
    fn serialize_unit(self) -> Result<(), NotFinite> {
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<(), NotFinite> {
        Ok(())
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result<(), NotFinite> {
        Ok(())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T) -> Result<(), NotFinite> {
        v.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        v: &T,
    ) -> Result<(), NotFinite> {
        self.path.push(variant.to_owned());
        v.serialize(&mut *self)?;
        self.path.pop();
        Ok(())
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self, NotFinite> {
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self, NotFinite> {
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, NotFinite> {
        Ok(self)
    }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Result<Self, NotFinite> {
        Ok(self)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self, NotFinite> {
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, NotFinite> {
        Ok(self)
    }
    fn serialize_struct_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Result<Self, NotFinite> {
        Ok(self)
    }
}

macro_rules! elements {
    ($($tr:ident :: $m:ident),*) => {
        $(impl ser::$tr for &mut Walker {
            type Ok = ();
            type Error = NotFinite;
            fn $m<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), NotFinite> {
                v.serialize(&mut **self)
            }
            fn end(self) -> Result<(), NotFinite> {
                Ok(())
            }
        })*
    };
}

elements!(
    SerializeSeq::serialize_element,
    SerializeTuple::serialize_element,
    SerializeTupleStruct::serialize_field,
    SerializeTupleVariant::serialize_field
);

impl ser::SerializeMap for &mut Walker {
    type Ok = ();
    type Error = NotFinite;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, k: &T) -> Result<(), NotFinite> {
        // The key names the path; keys themselves are ids and names, but check them too.
        let name = serde_json::to_value(k).ok().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned));
        self.path.push(name.unwrap_or_default());
        k.serialize(&mut **self)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), NotFinite> {
        let r = v.serialize(&mut **self);
        self.path.pop();
        r
    }
    fn end(self) -> Result<(), NotFinite> {
        Ok(())
    }
}

macro_rules! fields {
    ($($tr:ident),*) => {
        $(impl ser::$tr for &mut Walker {
            type Ok = ();
            type Error = NotFinite;
            fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, v: &T) -> Result<(), NotFinite> {
                self.path.push(key.to_owned());
                v.serialize(&mut **self)?;
                self.path.pop();
                Ok(())
            }
            fn end(self) -> Result<(), NotFinite> {
                Ok(())
            }
        })*
    };
}

fields!(SerializeStruct, SerializeStructVariant);

#[cfg(test)]
mod tests {
    #[derive(serde::Serialize)]
    struct P {
        a: f64,
        list: Vec<(f64, f64)>,
        map: std::collections::BTreeMap<String, Option<f64>>,
    }

    #[test]
    fn the_first_non_finite_number_is_named() {
        let mut p = P { a: 1.0, list: vec![(1.0, 2.0)], map: [("k".to_owned(), Some(3.0))].into() };
        assert!(super::check(&p).is_ok());
        p.map.insert("bad".into(), Some(f64::INFINITY));
        assert_eq!(super::check(&p).unwrap_err().0, "inf at map.bad");
        p.a = f64::NAN;
        assert_eq!(super::check(&p).unwrap_err().0, "NaN at a");
    }
}
