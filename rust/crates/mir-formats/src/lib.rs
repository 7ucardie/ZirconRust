//! Readers and writers for the Legend of Mir 3 "Zircon" asset formats.
//!
//! - [`zl`]: `.Zl` sprite libraries (DXT-compressed images with offsets and shadows)
//! - [`map`]: `.map` tile maps (48x32 cell grid, three tile layers, walk flags)
//! - [`mirdb`]: `System.db` static game content written by the C# MirDB ORM
//! - [`dxt`]: DXT1/DXT5 block decoders and encoders
//! - [`monster_table`]: `LibraryFile` paths and the monster image -> library table
//!
//! `.map` and MirDB files round-trip byte for byte through their `parse` /
//! `to_bytes` pairs; `save` writes a timestamped backup first.

pub mod dxt;
pub mod map;
pub mod mirdb;
pub mod monster_animation;
pub mod monster_table;
pub mod navigation;
pub mod room_door;
pub mod wearable_animation;
pub mod zl;

pub use map::MapFile;
pub use zl::ZlLibrary;

/// Width of one map cell in pixels.
pub const CELL_WIDTH: i32 = 48;
/// Height of one map cell in pixels.
pub const CELL_HEIGHT: i32 = 32;

/// Little-endian cursor over a byte slice, the only primitive the readers need.
#[derive(Clone, Copy)]
pub(crate) struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("unexpected end of data at offset {0}")]
    Eof(usize),
    #[error("invalid {0}")]
    Invalid(&'static str),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, FormatError>;

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    pub fn at(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.data.len() {
            return Err(FormatError::Eof(self.pos));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    pub fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }
    pub fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }
    pub fn u16(&mut self) -> Result<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    pub fn u32(&mut self) -> Result<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    pub fn u64(&mut self) -> Result<u64> {
        let b = self.bytes(8)?;
        Ok(u64::from_le_bytes(b.try_into().unwrap()))
    }
    pub fn i64(&mut self) -> Result<i64> {
        Ok(self.u64()? as i64)
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
    pub fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.u64()?))
    }
    /// .NET BinaryReader 7-bit encoded int (LEB128-style).
    pub fn varint(&mut self) -> Result<u32> {
        let mut result = 0u32;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            result |= ((b & 0x7F) as u32) << shift;
            if b & 0x80 == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift > 35 {
                return Err(FormatError::Invalid("7-bit encoded int"));
            }
        }
    }
    /// .NET BinaryReader string: 7-bit length prefix, UTF-8 bytes.
    pub fn string(&mut self) -> Result<String> {
        let len = self.varint()? as usize;
        let b = self.bytes(len)?;
        Ok(String::from_utf8_lossy(b).into_owned())
    }
}

/// Little-endian byte builder mirroring [`Cursor`].
#[derive(Default)]
pub(crate) struct Writer {
    pub data: Vec<u8>,
}

impl Writer {
    pub fn u8(&mut self, v: u8) {
        self.data.push(v);
    }
    pub fn i8(&mut self, v: i8) {
        self.u8(v as u8);
    }
    pub fn bool(&mut self, v: bool) {
        self.u8(v as u8);
    }
    pub fn u16(&mut self, v: u16) {
        self.data.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i16(&mut self, v: i16) {
        self.u16(v as u16);
    }
    pub fn u32(&mut self, v: u32) {
        self.data.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.u32(v as u32);
    }
    pub fn u64(&mut self, v: u64) {
        self.data.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i64(&mut self, v: i64) {
        self.u64(v as u64);
    }
    pub fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    pub fn f64(&mut self, v: f64) {
        self.u64(v.to_bits());
    }
    pub fn bytes(&mut self, b: &[u8]) {
        self.data.extend_from_slice(b);
    }
    /// .NET BinaryWriter 7-bit encoded int.
    pub fn varint(&mut self, mut v: u32) {
        while v >= 0x80 {
            self.u8((v as u8) | 0x80);
            v >>= 7;
        }
        self.u8(v as u8);
    }
    /// .NET BinaryWriter string: 7-bit byte length, UTF-8 bytes.
    pub fn string(&mut self, s: &str) {
        self.varint(s.len() as u32);
        self.bytes(s.as_bytes());
    }
}

/// Write `bytes` to `path`, first copying an existing file to
/// `<path>.bak-<unix seconds>` so a bad save is one rename away from undone.
/// Returns the backup path when one was made.
pub fn save_with_backup(
    path: impl AsRef<std::path::Path>,
    bytes: &[u8],
) -> Result<Option<std::path::PathBuf>> {
    let path = path.as_ref();
    let backup = if path.exists() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let name = format!(
            "{}.bak-{stamp}",
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
        let bak = path.with_file_name(name);
        std::fs::copy(path, &bak)?;
        Some(bak)
    } else {
        None
    };
    let tmp = path.with_extension("tmp-save");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(backup)
}

#[cfg(test)]
mod writer_tests {
    use super::*;

    #[test]
    fn varint_and_string_round_trip() {
        let mut w = Writer::default();
        for v in [0u32, 1, 127, 128, 300, 16_383, 16_384, 1 << 30] {
            w.varint(v);
        }
        w.string("héllo");
        let mut c = Cursor::new(&w.data);
        for v in [0u32, 1, 127, 128, 300, 16_383, 16_384, 1 << 30] {
            assert_eq!(c.varint().unwrap(), v);
        }
        assert_eq!(c.string().unwrap(), "héllo");
    }
}
