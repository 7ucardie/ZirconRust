//! Reader and writer for Zircon's MirDB files (`System.db`).
//!
//! The format is self-describing:
//! ```text
//! i32 mapping_count
//! mapping_count × { string type_name; i32 prop_count; prop_count × { string name; string type } }
//! mapping_count × { i32 len; bytes[len] collection }
//! collection = i32 next_index; i32 count; count × { i32 raw_len; bytes[raw_len] object }
//! object     = properties in mapping order, no tags
//! ```
//! Enums are stored as their underlying integer type and object references as
//! `System.Int32` holding the target's `Index` (0 = null). `Index` itself is an
//! ordinary persisted property.

use std::collections::HashMap;
use std::path::Path;

use crate::{Cursor, FormatError, Result, Writer};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    /// .NET decimal: (lo, mid, hi, flags). Use [`Value::as_f64`] for a lossy read.
    Decimal(u32, u32, u32, u32),
    Str(String),
    Bytes(Vec<u8>),
    Point(i32, i32),
    Size(i32, i32),
    /// ARGB.
    Color(u32),
    /// `DateTime.ToBinary()`.
    DateTime(i64),
    /// Ticks.
    TimeSpan(i64),
    IntArray(Option<Vec<i32>>),
    PointArray(Option<Vec<(i32, i32)>>),
    /// LSB-first bits, as stored (`byte_len * 8` bits).
    BitArray(Option<Vec<u8>>),
    /// `(stat id, amount)` pairs.
    Stats(Option<Vec<(i32, i32)>>),
}

impl Value {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::UInt(v) => Some(*v as i64),
            Value::Bool(b) => Some(*b as i64),
            _ => None,
        }
    }
    pub fn as_i32(&self) -> Option<i32> {
        self.as_i64().map(|v| v as i32)
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Int(v) => Some(*v != 0),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(v) => Some(*v as f64),
            Value::UInt(v) => Some(*v as f64),
            Value::Decimal(lo, mid, hi, flags) => {
                let mag = (*lo as u128) | ((*mid as u128) << 32) | ((*hi as u128) << 64);
                let scale = ((flags >> 16) & 0xFF) as i32;
                let neg = flags & 0x8000_0000 != 0;
                let v = mag as f64 / 10f64.powi(scale);
                Some(if neg { -v } else { v })
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Property {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone)]
pub struct Mapping {
    /// Normalised type name (`Library.SystemModels.*`).
    pub type_name: String,
    /// The name exactly as stored in the file, written back on save.
    pub stored_type_name: String,
    pub properties: Vec<Property>,
}

/// One stored object: property values in mapping order.
#[derive(Debug, Clone)]
pub struct Record {
    pub values: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct Collection {
    pub mapping: Mapping,
    pub next_index: i32,
    pub records: Vec<Record>,
    index_of: HashMap<String, usize>,
}

impl Collection {
    /// Create an empty collection with an explicit ORM mapping.
    pub fn new(mapping: Mapping) -> Result<Self> {
        let mut index_of = HashMap::new();
        for (index, property) in mapping.properties.iter().enumerate() {
            if property.name.is_empty() || index_of.insert(property.name.clone(), index).is_some() {
                return Err(FormatError::Other(
                    "Collection properties must have unique nonempty names".into(),
                ));
            }
        }
        if !mapping
            .properties
            .iter()
            .any(|p| p.name == "Index" && p.type_name == "System.Int32")
        {
            return Err(FormatError::Other(
                "Collection requires a System.Int32 Index property".into(),
            ));
        }
        Ok(Self {
            mapping,
            next_index: 1,
            records: vec![],
            index_of,
        })
    }

    /// Short type name, e.g. `MapInfo` for `Library.SystemModels.MapInfo`.
    pub fn short_name(&self) -> &str {
        self.mapping
            .type_name
            .rsplit('.')
            .next()
            .unwrap_or(&self.mapping.type_name)
    }

    pub fn property_index(&self, name: &str) -> Option<usize> {
        self.index_of.get(name).copied()
    }

    pub fn has_property(&self, name: &str) -> bool {
        self.index_of.contains_key(name)
    }

    pub fn get<'a>(&self, record: &'a Record, name: &str) -> Option<&'a Value> {
        self.property_index(name).and_then(|i| record.values.get(i))
    }

    pub fn int(&self, record: &Record, name: &str) -> Option<i64> {
        self.get(record, name).and_then(Value::as_i64)
    }

    pub fn int_or(&self, record: &Record, name: &str, default: i64) -> i64 {
        self.int(record, name).unwrap_or(default)
    }

    pub fn bool_or(&self, record: &Record, name: &str, default: bool) -> bool {
        self.get(record, name)
            .and_then(Value::as_bool)
            .unwrap_or(default)
    }

    pub fn str_or<'a>(&self, record: &'a Record, name: &str, default: &'a str) -> &'a str {
        self.get(record, name)
            .and_then(Value::as_str)
            .unwrap_or(default)
    }

    pub fn float_or(&self, record: &Record, name: &str, default: f64) -> f64 {
        self.get(record, name)
            .and_then(Value::as_f64)
            .unwrap_or(default)
    }

    /// The object's `Index` (its primary key).
    pub fn index(&self, record: &Record) -> i32 {
        self.int_or(record, "Index", 0) as i32
    }
}

#[derive(Debug)]
pub struct MirDb {
    pub collections: Vec<Collection>,
}

impl MirDb {
    pub fn load(path: impl AsRef<Path>) -> Result<MirDb> {
        let data = std::fs::read(path)?;
        MirDb::parse(&data)
    }

    pub fn parse(data: &[u8]) -> Result<MirDb> {
        if data.len() > 21 {
            let sniff = String::from_utf8_lossy(&data[5..21]);
            let plain = sniff.starts_with("Plugin.")
                || sniff == "Server.DBModels."
                || sniff == "Library.SystemMo"
                || sniff == "Client.UserModel";
            if !plain {
                return Err(FormatError::Other(
                    "database appears to be encrypted; set EncryptionEnabled=false and re-save"
                        .into(),
                ));
            }
        }
        let mut c = Cursor::new(data);
        let mapping_count = c.i32()?;
        if mapping_count < 0 {
            return Err(FormatError::Invalid("mirdb mapping count"));
        }
        let mut mappings = Vec::new();
        for _ in 0..mapping_count {
            let stored_type_name = c.string()?;
            let mut type_name = stored_type_name.clone();
            if type_name.contains("Server.DBModels") {
                type_name = type_name.replace("Server.DBModels", "Library.SystemModels");
            }
            let prop_count = c.i32()?;
            let mut properties = Vec::new();
            for _ in 0..prop_count {
                let name = c.string()?;
                let type_name = c.string()?;
                properties.push(Property { name, type_name });
            }
            mappings.push(Mapping {
                type_name,
                stored_type_name,
                properties,
            });
        }
        let mut collections = Vec::with_capacity(mappings.len());
        for mapping in mappings {
            let len = c.i32()?;
            let blob = c.bytes(len.max(0) as usize)?;
            let mut b = Cursor::new(blob);
            let next_index = b.i32()?;
            let count = b.i32()?;
            let mut records = Vec::new();
            for _ in 0..count {
                let raw_len = b.i32()?;
                let raw = b.bytes(raw_len.max(0) as usize)?;
                let mut r = Cursor::new(raw);
                let mut values = Vec::with_capacity(mapping.properties.len());
                for p in &mapping.properties {
                    values.push(read_value(&mut r, &p.type_name).map_err(|e| {
                        FormatError::Other(format!(
                            "{}.{} ({}): {e}",
                            mapping.type_name, p.name, p.type_name
                        ))
                    })?);
                }
                records.push(Record { values });
            }
            let index_of = mapping
                .properties
                .iter()
                .enumerate()
                .map(|(i, p)| (p.name.clone(), i))
                .collect();
            collections.push(Collection {
                mapping,
                next_index,
                records,
                index_of,
            });
        }
        Ok(MirDb { collections })
    }

    /// Find a collection by short type name (e.g. `"MapInfo"`).
    pub fn collection(&self, short_name: &str) -> Option<&Collection> {
        self.collections
            .iter()
            .find(|c| c.short_name() == short_name)
    }

    pub fn collection_mut(&mut self, short_name: &str) -> Option<&mut Collection> {
        self.collections
            .iter_mut()
            .find(|c| c.short_name() == short_name)
    }

    /// Re-encode the database; `parse(to_bytes())` is byte-identical for a
    /// file the C# ORM wrote.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Writer::default();
        out.i32(self.collections.len() as i32);
        for c in &self.collections {
            out.string(&c.mapping.stored_type_name);
            out.i32(c.mapping.properties.len() as i32);
            for p in &c.mapping.properties {
                out.string(&p.name);
                out.string(&p.type_name);
            }
        }
        for c in &self.collections {
            let mut blob = Writer::default();
            blob.i32(c.next_index);
            blob.i32(c.records.len() as i32);
            for r in &c.records {
                if r.values.len() != c.mapping.properties.len() {
                    return Err(FormatError::Other(format!(
                        "{}: record has {} values for {} properties",
                        c.mapping.type_name,
                        r.values.len(),
                        c.mapping.properties.len()
                    )));
                }
                let mut raw = Writer::default();
                for (p, v) in c.mapping.properties.iter().zip(&r.values) {
                    write_value(&mut raw, &p.type_name, v).map_err(|e| {
                        FormatError::Other(format!(
                            "{}.{} ({}): {e}",
                            c.mapping.type_name, p.name, p.type_name
                        ))
                    })?;
                }
                blob.i32(raw.data.len() as i32);
                blob.bytes(&raw.data);
            }
            out.i32(blob.data.len() as i32);
            out.bytes(&blob.data);
        }
        Ok(out.data)
    }

    /// Save with a timestamped backup of the previous file.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<Option<std::path::PathBuf>> {
        crate::save_with_backup(path, &self.to_bytes()?)
    }
}

fn type_mismatch(type_name: &str, v: &Value) -> FormatError {
    FormatError::Other(format!("cannot store {v:?} as {type_name}"))
}

fn write_value(w: &mut Writer, type_name: &str, v: &Value) -> Result<()> {
    match (type_name, v) {
        ("System.Boolean", Value::Bool(b)) => w.bool(*b),
        ("System.Byte", _) => w.u8(v.as_i64().ok_or_else(|| type_mismatch(type_name, v))? as u8),
        ("System.SByte", _) => w.i8(v.as_i64().ok_or_else(|| type_mismatch(type_name, v))? as i8),
        ("System.Int16", _) => w.i16(v.as_i64().ok_or_else(|| type_mismatch(type_name, v))? as i16),
        ("System.UInt16", _) => {
            w.u16(v.as_i64().ok_or_else(|| type_mismatch(type_name, v))? as u16)
        }
        ("System.Int32", _) => w.i32(v.as_i64().ok_or_else(|| type_mismatch(type_name, v))? as i32),
        ("System.UInt32", _) => {
            w.u32(v.as_i64().ok_or_else(|| type_mismatch(type_name, v))? as u32)
        }
        ("System.Int64", _) => w.i64(v.as_i64().ok_or_else(|| type_mismatch(type_name, v))?),
        ("System.UInt64", _) => {
            w.u64(v.as_i64().ok_or_else(|| type_mismatch(type_name, v))? as u64)
        }
        ("System.Single", Value::Float(f)) => w.f32(*f as f32),
        ("System.Single", _) => {
            w.f32(v.as_f64().ok_or_else(|| type_mismatch(type_name, v))? as f32)
        }
        ("System.Double", Value::Float(f)) => w.f64(*f),
        ("System.Double", _) => w.f64(v.as_f64().ok_or_else(|| type_mismatch(type_name, v))?),
        ("System.Decimal", Value::Decimal(lo, mid, hi, flags)) => {
            w.u32(*lo);
            w.u32(*mid);
            w.u32(*hi);
            w.u32(*flags);
        }
        ("System.Char", Value::Str(s)) => {
            let ch = s.chars().next().unwrap_or('\0');
            let mut buf = [0u8; 4];
            w.bytes(ch.encode_utf8(&mut buf).as_bytes());
        }
        ("System.String", Value::Str(s)) => w.string(s),
        ("System.DateTime", Value::DateTime(t)) => w.i64(*t),
        ("System.TimeSpan", Value::TimeSpan(t)) => w.i64(*t),
        ("System.Drawing.Point", Value::Point(x, y)) => {
            w.i32(*x);
            w.i32(*y);
        }
        ("System.Drawing.Size", Value::Size(x, y)) => {
            w.i32(*x);
            w.i32(*y);
        }
        ("System.Drawing.Color", Value::Color(c)) => w.u32(*c),
        ("System.Byte[]", Value::Bytes(b)) => {
            w.i32(b.len() as i32);
            w.bytes(b);
        }
        ("System.Int32[]", Value::IntArray(a)) => match a {
            None => w.bool(false),
            Some(a) => {
                w.bool(true);
                w.i32(a.len() as i32);
                for v in a {
                    w.i32(*v);
                }
            }
        },
        ("System.Drawing.Point[]", Value::PointArray(a)) => match a {
            None => w.bool(false),
            Some(a) => {
                w.bool(true);
                w.i32(a.len() as i32);
                for (x, y) in a {
                    w.i32(*x);
                    w.i32(*y);
                }
            }
        },
        ("System.Collections.BitArray", Value::BitArray(a)) => match a {
            None => w.bool(false),
            Some(a) => {
                w.bool(true);
                w.i32(a.len() as i32);
                w.bytes(a);
            }
        },
        ("Library.Stats", Value::Stats(a)) => match a {
            None => w.bool(false),
            Some(a) => {
                w.bool(true);
                w.i32(a.len() as i32);
                for (k, v) in a {
                    w.i32(*k);
                    w.i32(*v);
                }
            }
        },
        _ => return Err(type_mismatch(type_name, v)),
    }
    Ok(())
}

impl Collection {
    /// Set a property on a record by name; false when the property is unknown.
    pub fn set(&self, record: &mut Record, name: &str, value: Value) -> bool {
        match self.property_index(name) {
            Some(i) if i < record.values.len() => {
                record.values[i] = value;
                true
            }
            _ => false,
        }
    }

    /// Shipped databases can store the last issued identity in next_index.
    /// Honour higher counters, but never reuse an identity present in the rows.
    pub fn allocate_index(&mut self) -> i32 {
        let first_unused = self
            .records
            .iter()
            .map(|r| self.index(r))
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .expect("collection identities exhausted");
        let index = self.next_index.max(first_unused);
        self.next_index = index
            .checked_add(1)
            .expect("collection identities exhausted");
        index
    }

    /// Append a record whose `Index` is the collection's next index (like the
    /// C# ORM); other values come from `values` (missing ones default).
    pub fn push_record(&mut self, values: Vec<(&str, Value)>) -> i32 {
        let index = self.allocate_index();
        let mut record = Record {
            values: self
                .mapping
                .properties
                .iter()
                .map(|p| default_value(&p.type_name))
                .collect(),
        };
        self.set(&mut record, "Index", Value::Int(index as i64));
        for (name, v) in values {
            self.set(&mut record, name, v);
        }
        self.records.push(record);
        index
    }
}

/// The value an unset property gets (mirrors .NET defaults).
pub fn default_value(type_name: &str) -> Value {
    match type_name {
        "System.Boolean" => Value::Bool(false),
        "System.Byte" | "System.UInt16" | "System.UInt32" | "System.UInt64" => Value::UInt(0),
        "System.SByte" | "System.Int16" | "System.Int32" | "System.Int64" => Value::Int(0),
        "System.Single" | "System.Double" => Value::Float(0.0),
        "System.Decimal" => Value::Decimal(0, 0, 0, 0),
        "System.Char" | "System.String" => Value::Str(String::new()),
        "System.DateTime" => Value::DateTime(0),
        "System.TimeSpan" => Value::TimeSpan(0),
        "System.Drawing.Point" => Value::Point(0, 0),
        "System.Drawing.Size" => Value::Size(0, 0),
        "System.Drawing.Color" => Value::Color(0),
        "System.Byte[]" => Value::Bytes(Vec::new()),
        "System.Int32[]" => Value::IntArray(None),
        "System.Drawing.Point[]" => Value::PointArray(None),
        "System.Collections.BitArray" => Value::BitArray(None),
        "Library.Stats" => Value::Stats(None),
        _ => Value::Int(0),
    }
}

fn read_value(c: &mut Cursor, type_name: &str) -> Result<Value> {
    Ok(match type_name {
        "System.Boolean" => Value::Bool(c.bool()?),
        "System.Byte" => Value::UInt(c.u8()? as u64),
        "System.SByte" => Value::Int(c.i8()? as i64),
        "System.Int16" => Value::Int(c.i16()? as i64),
        "System.UInt16" => Value::UInt(c.u16()? as u64),
        "System.Int32" => Value::Int(c.i32()? as i64),
        "System.UInt32" => Value::UInt(c.u32()? as u64),
        "System.Int64" => Value::Int(c.i64()?),
        "System.UInt64" => Value::UInt(c.u64()?),
        "System.Single" => Value::Float(c.f32()? as f64),
        "System.Double" => Value::Float(c.f64()?),
        "System.Decimal" => Value::Decimal(c.u32()?, c.u32()?, c.u32()?, c.u32()?),
        "System.Char" => {
            // UTF-8 encoded char, 1..=3 bytes.
            let first = c.u8()?;
            let extra = if first < 0x80 {
                0
            } else if first >> 5 == 0b110 {
                1
            } else {
                2
            };
            let mut buf = vec![first];
            buf.extend_from_slice(c.bytes(extra)?);
            Value::Str(String::from_utf8_lossy(&buf).into_owned())
        }
        "System.String" => Value::Str(c.string()?),
        "System.DateTime" => Value::DateTime(c.i64()?),
        "System.TimeSpan" => Value::TimeSpan(c.i64()?),
        "System.Drawing.Point" => Value::Point(c.i32()?, c.i32()?),
        "System.Drawing.Size" => Value::Size(c.i32()?, c.i32()?),
        "System.Drawing.Color" => Value::Color(c.u32()?),
        "System.Byte[]" => {
            let len = c.i32()?.max(0) as usize;
            Value::Bytes(c.bytes(len)?.to_vec())
        }
        "System.Int32[]" => {
            if !c.bool()? {
                Value::IntArray(None)
            } else {
                let len = c.i32()?.max(0) as usize;
                let mut v = Vec::new();
                for _ in 0..len {
                    v.push(c.i32()?);
                }
                Value::IntArray(Some(v))
            }
        }
        "System.Drawing.Point[]" => {
            if !c.bool()? {
                Value::PointArray(None)
            } else {
                let len = c.i32()?.max(0) as usize;
                let mut v = Vec::new();
                for _ in 0..len {
                    v.push((c.i32()?, c.i32()?));
                }
                Value::PointArray(Some(v))
            }
        }
        "System.Collections.BitArray" => {
            if !c.bool()? {
                Value::BitArray(None)
            } else {
                let len = c.i32()?.max(0) as usize;
                Value::BitArray(Some(c.bytes(len)?.to_vec()))
            }
        }
        "Library.Stats" => {
            if !c.bool()? {
                Value::Stats(None)
            } else {
                let len = c.i32()?.max(0) as usize;
                let mut v = Vec::new();
                for _ in 0..len {
                    v.push((c.i32()?, c.i32()?));
                }
                Value::Stats(Some(v))
            }
        }
        other => {
            return Err(FormatError::Other(format!(
                "unsupported MirDB type {other}"
            )));
        }
    })
}

/// Zircon `Stat` enum values used by the game data (`LibraryCore/Stat.cs`).
pub mod stat {
    pub const BASE_HEALTH: i32 = 0;
    pub const BASE_MANA: i32 = 1;
    pub const HEALTH: i32 = 2;
    pub const MANA: i32 = 3;
    pub const MIN_AC: i32 = 4;
    pub const MAX_AC: i32 = 5;
    pub const MIN_MR: i32 = 6;
    pub const MAX_MR: i32 = 7;
    pub const MIN_DC: i32 = 8;
    pub const MAX_DC: i32 = 9;
    pub const MIN_MC: i32 = 10;
    pub const MAX_MC: i32 = 11;
    pub const MIN_SC: i32 = 12;
    pub const MAX_SC: i32 = 13;
    pub const ACCURACY: i32 = 14;
    pub const AGILITY: i32 = 15;
    pub const ATTACK_SPEED: i32 = 16;
    /// Light radius.
    pub const LIGHT: i32 = 17;
    pub const LUCK: i32 = 19;
    pub const PICK_UP_RADIUS: i32 = 40;
    pub const MC_PERCENT: i32 = 67;
    pub const DC_PERCENT: i32 = 84;
    pub const SC_PERCENT: i32 = 85;
    /// Experience modifiers (`Stat.ExperienceRate` and `Stat.BaseExperienceRate`).
    pub const EXPERIENCE_RATE: i32 = 36;
    pub const BASE_EXPERIENCE_RATE: i32 = 106;
    /// Experience granted by a consumable.
    pub const EXPERIENCE: i32 = 116;
    pub const BLOCK_CHANCE: i32 = 125;
    pub const EVASION_CHANCE: i32 = 126;
    pub const POISON_RESISTANCE: i32 = 129;
    /// Focus point pool, granted by the discipline track.
    pub const FOCUS: i32 = 131;
    pub const GROWTH_LEVEL: i32 = 133;
}

/// `RegionType` values.
pub mod region_type {
    pub const NONE: i64 = 0;
    pub const AREA: i64 = 1;
    pub const CONNECTION: i64 = 2;
    pub const SPAWN: i64 = 3;
    pub const NPC: i64 = 4;
    pub const SPAWN_CONNECTION: i64 = 5;
}

/// Decode a `MapRegion` into cell coordinates given the map width, mirroring
/// `MapRegion.CreatePoints`: the bit array takes precedence over the point array.
pub fn region_points(
    bit_region: Option<&[u8]>,
    point_region: Option<&[(i32, i32)]>,
    map_width: i32,
) -> Vec<(i32, i32)> {
    if let Some(bits) = bit_region {
        let mut out = Vec::new();
        for (byte_i, byte) in bits.iter().enumerate() {
            if *byte == 0 {
                continue;
            }
            for bit in 0..8 {
                if byte & (1 << bit) != 0 {
                    let i = (byte_i * 8 + bit) as i32;
                    out.push((i % map_width, i / map_width));
                }
            }
        }
        return out;
    }
    point_region.map(|p| p.to_vec()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_collection_builds_field_lookup_and_roundtrips_references() {
        let mapping = Mapping {
            type_name: "Library.SystemModels.CastleGateInfo".into(),
            stored_type_name: "Library.SystemModels.CastleGateInfo".into(),
            properties: [("Castle", "System.Int32"), ("Index", "System.Int32")]
                .into_iter()
                .map(|(name, type_name)| Property {
                    name: name.into(),
                    type_name: type_name.into(),
                })
                .collect(),
        };
        let mut collection = Collection::new(mapping.clone()).unwrap();
        assert_eq!(collection.push_record(vec![("Castle", Value::Int(7))]), 1);
        let db = MirDb {
            collections: vec![collection],
        };
        let bytes = db.to_bytes().unwrap();
        let restored = MirDb::parse(&bytes).unwrap();
        let c = restored.collection("CastleGateInfo").unwrap();
        assert_eq!(c.int(&c.records[0], "Castle"), Some(7));
        assert_eq!(restored.to_bytes().unwrap(), bytes);
        let mut duplicate = mapping.clone();
        duplicate.properties.push(duplicate.properties[0].clone());
        assert!(Collection::new(duplicate).is_err());
        let mut no_index = mapping;
        no_index.properties.pop();
        assert!(Collection::new(no_index).is_err());
    }

    #[test]
    fn truncated_db_is_an_error_not_an_abort() {
        let mapping = Mapping {
            type_name: "Library.SystemModels.CastleGateInfo".into(),
            stored_type_name: "Library.SystemModels.CastleGateInfo".into(),
            properties: vec![Property {
                name: "Index".into(),
                type_name: "System.Int32".into(),
            }],
        };
        let mut collection = Collection::new(mapping).unwrap();
        collection.push_record(vec![("Index", Value::Int(1))]);
        let db = MirDb {
            collections: vec![collection],
        };
        let bytes = db.to_bytes().unwrap();
        for cut in [0, 10, 21, bytes.len() / 2, bytes.len() - 1] {
            assert!(MirDb::parse(&bytes[..cut]).is_err(), "cut {cut}");
        }
    }

    #[test]
    fn decimal_to_f64() {
        // 12.5 = 125 scaled by 1
        let v = Value::Decimal(125, 0, 0, 1 << 16);
        assert_eq!(v.as_f64(), Some(12.5));
        let n = Value::Decimal(7, 0, 0, 0x8000_0000);
        assert_eq!(n.as_f64(), Some(-7.0));
    }

    #[test]
    fn bit_region_points() {
        // width 4: bits 1 and 6 -> (1,0) and (2,1)
        let bits = [0b0100_0010u8];
        assert_eq!(region_points(Some(&bits), None, 4), vec![(1, 0), (2, 1)]);
    }

    #[test]
    fn system_db_round_trips_byte_identical() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
            eprintln!("ZIRCON_ASSETS not set; skipping");
            return;
        };
        let path = Path::new(&assets).join("../Database/System.db");
        let data = std::fs::read(path).unwrap();
        let db = MirDb::parse(&data).unwrap();
        let back = db.to_bytes().unwrap();
        assert_eq!(back.len(), data.len());
        assert!(back == data, "System.db changed on round trip");
    }

    #[test]
    fn edited_records_survive_a_save() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
            eprintln!("ZIRCON_ASSETS not set; skipping");
            return;
        };
        let path = Path::new(&assets).join("../Database/System.db");
        let mut db = MirDb::load(path).unwrap();
        let items = db.collection_mut("ItemInfo").unwrap();
        let mut first = items.records[0].clone();
        assert!(items.set(&mut first, "Price", Value::Int(12_345)));
        items.records[0] = first;
        let new_index = items.push_record(vec![
            ("ItemName", Value::Str("Test Blade".into())),
            ("Price", Value::Int(7)),
        ]);
        let bytes = db.to_bytes().unwrap();
        let again = MirDb::parse(&bytes).unwrap();
        let items = again.collection("ItemInfo").unwrap();
        assert_eq!(items.int_or(&items.records[0], "Price", 0), 12_345);
        let added = items.records.last().unwrap();
        assert_eq!(items.index(added), new_index);
        assert_eq!(items.str_or(added, "ItemName", ""), "Test Blade");
        assert_eq!(items.next_index, new_index + 1);
    }

    #[test]
    fn save_makes_a_backup() {
        let dir = std::env::temp_dir().join(format!("mirdb-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.map");
        let m = crate::MapFile::blank(4, 4);
        assert!(m.save(&path).unwrap().is_none());
        let bak = m.save(&path).unwrap().expect("a backup on overwrite");
        assert!(bak.exists());
        assert_eq!(std::fs::read(&bak).unwrap(), m.to_bytes());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn loads_system_db() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
            eprintln!("ZIRCON_ASSETS not set; skipping");
            return;
        };
        let path = Path::new(&assets).join("../Database/System.db");
        let db = MirDb::load(path).unwrap();
        let maps = db.collection("MapInfo").unwrap();
        assert!(!maps.records.is_empty());
        assert!(maps.property_index("FileName").is_some());
    }
}
