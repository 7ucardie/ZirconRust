//! `.Zl` sprite library reader and writer (Zircon's `MirLibrary` / `Mir3Library`).
//!
//! Layout (all little-endian):
//! ```text
//! i32 header_size
//! header block:
//!     i32 value      count = version == 0 ? value : value & 0x01FF_FFFF
//!                    version = (value >> 25) & 0x7F
//!     count times:   u8 present; if present: 25-byte image header
//! payload:           per-image DXT blobs at absolute `position`
//! ```
//! Image header: `i32 position, i16 width, i16 height, i16 offset_x, i16 offset_y,
//! u8 shadow_type, i16 shadow_width, i16 shadow_height, i16 shadow_offset_x,
//! i16 shadow_offset_y, i16 overlay_width, i16 overlay_height`.
//!
//! Payload per image is three concatenated DXT surfaces: image, shadow, overlay.
//! Version 0 libraries are DXT1, everything else DXT5. Surface dimensions are
//! rounded up to multiples of 4 for sizing.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::{dxt, Cursor, FormatError, Result, Writer};

#[derive(Debug, Clone, Copy, Default)]
pub struct ImageInfo {
    pub position: u32,
    pub width: u16,
    pub height: u16,
    pub offset_x: i16,
    pub offset_y: i16,
    pub shadow_type: u8,
    pub shadow_width: u16,
    pub shadow_height: u16,
    pub shadow_offset_x: i16,
    pub shadow_offset_y: i16,
    pub overlay_width: u16,
    pub overlay_height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    Image,
    Shadow,
    Overlay,
}

/// Decoded RGBA8 pixels of one surface.
pub struct DecodedSurface {
    /// Padded (multiple of 4) width actually decoded.
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A lazily-read sprite library. The header is parsed once; pixel data is read
/// from disk on demand (the libraries total several GB).
pub struct ZlLibrary {
    path: PathBuf,
    version: u8,
    images: Vec<Option<ImageInfo>>,
    file: Mutex<File>,
    file_len: u64,
}

impl std::fmt::Debug for ZlLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZlLibrary")
            .field("path", &self.path)
            .field("version", &self.version)
            .field("images", &self.images.len())
            .finish()
    }
}

fn padded(v: u16) -> u32 {
    let v = v as u32;
    v + (4 - v % 4) % 4
}

impl ImageInfo {
    fn surface_size(&self, kind: SurfaceKind, version: u8) -> usize {
        let (w, h) = self.surface_dims(kind);
        if w == 0 || h == 0 {
            return 0;
        }
        let (w, h) = (padded(w) as usize, padded(h) as usize);
        if version > 0 {
            w * h
        } else {
            w * h / 2
        }
    }

    pub fn surface_dims(&self, kind: SurfaceKind) -> (u16, u16) {
        match kind {
            SurfaceKind::Image => (self.width, self.height),
            SurfaceKind::Shadow => (self.shadow_width, self.shadow_height),
            SurfaceKind::Overlay => (self.overlay_width, self.overlay_height),
        }
    }

    pub fn has_surface(&self, kind: SurfaceKind) -> bool {
        let (w, h) = self.surface_dims(kind);
        self.position != 0 && w > 0 && h > 0
    }
}

impl ZlLibrary {
    pub fn open(path: impl AsRef<Path>) -> Result<ZlLibrary> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path)?;
        let mut len_buf = [0u8; 4];
        file.read_exact(&mut len_buf)?;
        let header_size = i32::from_le_bytes(len_buf);
        let file_len = file.metadata()?.len();
        if header_size < 4 || header_size as u64 + 4 > file_len {
            return Err(FormatError::Invalid("zl header size"));
        }
        let mut header = vec![0u8; header_size as usize];
        file.read_exact(&mut header)?;
        let mut c = Cursor::new(&header);
        let value = c.i32()?;
        let version = ((value >> 25) & 0x7F) as u8;
        let count = if version == 0 {
            value
        } else {
            value & 0x01FF_FFFF
        };
        if count < 0 {
            return Err(FormatError::Invalid("zl image count"));
        }
        let mut images = Vec::with_capacity((count as usize).min(header_size as usize));
        for _ in 0..count {
            if !c.bool()? {
                images.push(None);
                continue;
            }
            let info = ImageInfo {
                position: c.i32()? as u32,
                width: c.i16()? as u16,
                height: c.i16()? as u16,
                offset_x: c.i16()?,
                offset_y: c.i16()?,
                shadow_type: c.u8()?,
                shadow_width: c.i16()? as u16,
                shadow_height: c.i16()? as u16,
                shadow_offset_x: c.i16()?,
                shadow_offset_y: c.i16()?,
                overlay_width: c.i16()? as u16,
                overlay_height: c.i16()? as u16,
            };
            images.push(Some(info));
        }
        Ok(ZlLibrary {
            path,
            version,
            images,
            file: Mutex::new(file),
            file_len,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn version(&self) -> u8 {
        self.version
    }

    pub fn is_dxt5(&self) -> bool {
        self.version > 0
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    pub fn info(&self, index: usize) -> Option<&ImageInfo> {
        self.images.get(index).and_then(|i| i.as_ref())
    }

    /// Read and decode one surface of one image to RGBA8. Returns `None` when
    /// the image or surface does not exist.
    pub fn decode(&self, index: usize, kind: SurfaceKind) -> Result<Option<DecodedSurface>> {
        let Some(info) = self.info(index) else {
            return Ok(None);
        };
        if !info.has_surface(kind) {
            return Ok(None);
        }
        let image_size = info.surface_size(SurfaceKind::Image, self.version);
        let shadow_size = info.surface_size(SurfaceKind::Shadow, self.version);
        let (offset, size) = match kind {
            SurfaceKind::Image => (0, image_size),
            SurfaceKind::Shadow => (image_size, shadow_size),
            SurfaceKind::Overlay => (
                image_size + shadow_size,
                info.surface_size(SurfaceKind::Overlay, self.version),
            ),
        };
        if info.position as u64 + offset as u64 + size as u64 > self.file_len {
            return Err(FormatError::Invalid("zl surface outside file"));
        }
        let mut raw = vec![0u8; size];
        {
            let mut file = self.file.lock().unwrap();
            file.seek(SeekFrom::Start(info.position as u64 + offset as u64))?;
            file.read_exact(&mut raw)?;
        }
        let (w, h) = info.surface_dims(kind);
        let (pw, ph) = (padded(w), padded(h));
        let rgba = dxt::decode(&raw, pw, ph, self.is_dxt5());
        Ok(Some(DecodedSurface {
            width: pw,
            height: ph,
            rgba,
        }))
    }
}

/// One image as stored: its header fields and the encoded payload (image,
/// shadow and overlay surfaces back to back). Copying these between
/// libraries is lossless.
#[derive(Debug, Clone)]
pub struct RawImage {
    /// `position` is ignored; the builder assigns it.
    pub info: ImageInfo,
    pub payload: Vec<u8>,
}

/// Assembles a `.Zl` file. Existing images can be carried over raw (no
/// re-encoding); new ones are encoded from RGBA.
#[derive(Debug, Clone)]
pub struct ZlBuilder {
    version: u8,
    images: Vec<Option<RawImage>>,
}

impl ZlBuilder {
    /// `version` 0 writes DXT1 libraries, anything else DXT5.
    pub fn new(version: u8) -> ZlBuilder {
        ZlBuilder {
            version,
            images: Vec::new(),
        }
    }

    pub fn version(&self) -> u8 {
        self.version
    }

    pub fn is_dxt5(&self) -> bool {
        self.version > 0
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    pub fn images(&self) -> &[Option<RawImage>] {
        &self.images
    }

    pub fn images_mut(&mut self) -> &mut Vec<Option<RawImage>> {
        &mut self.images
    }

    /// Append an image slot (an empty slot when `None`).
    pub fn push_raw(&mut self, image: Option<RawImage>) -> usize {
        self.images.push(image);
        self.images.len() - 1
    }

    /// Encode a surface of `width` x `height` RGBA pixels for this library.
    pub fn encode_surface(&self, rgba: &[u8], width: u16, height: u16) -> Vec<u8> {
        dxt::encode(rgba, width as u32, height as u32, self.is_dxt5())
    }

    /// Append a sprite from RGBA pixels with an optional shadow surface.
    /// Returns the new image index.
    pub fn push_rgba(&mut self, sprite: &Sprite<'_>) -> usize {
        let mut payload = self.encode_surface(sprite.rgba, sprite.width, sprite.height);
        let (shadow_width, shadow_height) = match sprite.shadow {
            Some((w, h, px)) => {
                payload.extend(self.encode_surface(px, w, h));
                (w, h)
            }
            None => (0, 0),
        };
        let info = ImageInfo {
            position: 0,
            width: sprite.width,
            height: sprite.height,
            offset_x: sprite.offset_x,
            offset_y: sprite.offset_y,
            shadow_type: sprite.shadow_type,
            shadow_width,
            shadow_height,
            shadow_offset_x: sprite.shadow_offset_x,
            shadow_offset_y: sprite.shadow_offset_y,
            overlay_width: 0,
            overlay_height: 0,
        };
        self.push_raw(Some(RawImage { info, payload }))
    }

    /// Serialise the library. Empty slots are written as absent entries;
    /// images with no pixels get position 0 like Zircon does.
    pub fn to_bytes(&self) -> Vec<u8> {
        let header_size: usize = 4 + self
            .images
            .iter()
            .map(|i| if i.is_some() { 26 } else { 1 })
            .sum::<usize>();
        let mut out = Writer::default();
        out.i32(header_size as i32);
        let count = self.images.len() as i32;
        let value = if self.version == 0 {
            count
        } else {
            ((self.version as i32) << 25) | (count & 0x01FF_FFFF)
        };
        out.i32(value);
        let mut position = 4 + header_size;
        let mut payloads = Writer::default();
        for image in &self.images {
            let Some(image) = image else {
                out.bool(false);
                continue;
            };
            out.bool(true);
            let info = &image.info;
            let pos = if image.payload.is_empty() {
                0
            } else {
                position
            };
            out.i32(pos as i32);
            out.i16(info.width as i16);
            out.i16(info.height as i16);
            out.i16(info.offset_x);
            out.i16(info.offset_y);
            out.u8(info.shadow_type);
            out.i16(info.shadow_width as i16);
            out.i16(info.shadow_height as i16);
            out.i16(info.shadow_offset_x);
            out.i16(info.shadow_offset_y);
            out.i16(info.overlay_width as i16);
            out.i16(info.overlay_height as i16);
            payloads.bytes(&image.payload);
            position += image.payload.len();
        }
        out.bytes(&payloads.data);
        out.data
    }

    /// Save with a timestamped backup of the previous file.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<Option<PathBuf>> {
        crate::save_with_backup(path, &self.to_bytes())
    }
}

/// RGBA input for [`ZlBuilder::push_rgba`].
pub struct Sprite<'a> {
    pub width: u16,
    pub height: u16,
    pub offset_x: i16,
    pub offset_y: i16,
    /// Tightly packed RGBA8, `width * height * 4` bytes.
    pub rgba: &'a [u8],
    pub shadow_type: u8,
    pub shadow_offset_x: i16,
    pub shadow_offset_y: i16,
    /// Optional shadow surface `(width, height, rgba)`.
    pub shadow: Option<(u16, u16, &'a [u8])>,
}

impl ZlLibrary {
    /// Total encoded size of an image's three surfaces.
    fn payload_size(&self, info: &ImageInfo) -> usize {
        info.surface_size(SurfaceKind::Image, self.version)
            + info.surface_size(SurfaceKind::Shadow, self.version)
            + info.surface_size(SurfaceKind::Overlay, self.version)
    }

    /// Read one image's header and encoded payload without decoding.
    pub fn raw(&self, index: usize) -> Result<Option<RawImage>> {
        let Some(info) = self.info(index) else {
            return Ok(None);
        };
        let size = if info.position == 0 {
            0
        } else {
            self.payload_size(info)
        };
        if info.position as u64 + size as u64 > self.file_len {
            return Err(FormatError::Invalid("zl surface outside file"));
        }
        let mut payload = vec![0u8; size];
        if size > 0 {
            let mut file = self.file.lock().unwrap();
            file.seek(SeekFrom::Start(info.position as u64))?;
            file.read_exact(&mut payload)?;
        }
        Ok(Some(RawImage {
            info: *info,
            payload,
        }))
    }

    /// Load every image raw into a builder, ready to edit and save.
    pub fn to_builder(&self) -> Result<ZlBuilder> {
        let mut b = ZlBuilder::new(self.version);
        for i in 0..self.len() {
            b.push_raw(self.raw(i)?);
        }
        Ok(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets() -> Option<PathBuf> {
        let p = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from)?;
        p.join("Data").exists().then_some(p)
    }

    #[test]
    fn reads_ground_library() {
        let Some(assets) = assets() else {
            eprintln!("ZIRCON_ASSETS not set; skipping");
            return;
        };
        let lib = ZlLibrary::open(assets.join("Data/Ground.Zl")).unwrap();
        assert!(!lib.is_empty());
        let first = (0..lib.len()).find(|&i| lib.info(i).is_some()).unwrap();
        let img = lib.decode(first, SurfaceKind::Image).unwrap().unwrap();
        assert_eq!(img.rgba.len(), (img.width * img.height * 4) as usize);
    }

    #[test]
    fn builder_round_trips_a_small_library() {
        let Some(assets) = assets() else {
            eprintln!("ZIRCON_ASSETS not set; skipping");
            return;
        };
        let path = assets.join("Data/Help.Zl");
        let data = std::fs::read(&path).unwrap();
        let lib = ZlLibrary::open(&path).unwrap();
        let builder = lib.to_builder().unwrap();
        let bytes = builder.to_bytes();
        assert_eq!(bytes.len(), data.len());
        // Byte-identical when the shipped payloads are packed in index order.
        assert!(bytes == data, "Help.Zl changed on round trip");
        // A sprite decoded, re-encoded and decoded again stays close.
        let first = (0..lib.len()).find(|&i| lib.info(i).is_some()).unwrap();
        let img = lib.decode(first, SurfaceKind::Image).unwrap().unwrap();
        let info = *lib.info(first).unwrap();
        let mut b = ZlBuilder::new(lib.version());
        let idx = b.push_rgba(&Sprite {
            width: info.width,
            height: info.height,
            offset_x: info.offset_x,
            offset_y: info.offset_y,
            rgba: &crop(&img, info.width as u32, info.height as u32),
            shadow_type: info.shadow_type,
            shadow_offset_x: 0,
            shadow_offset_y: 0,
            shadow: None,
        });
        let dir = std::env::temp_dir().join(format!("zl-build-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("new.Zl");
        b.save(&out).unwrap();
        let again = ZlLibrary::open(&out).unwrap();
        assert_eq!(again.version(), lib.version());
        let info2 = *again.info(idx).unwrap();
        assert_eq!((info2.width, info2.height), (info.width, info.height));
        assert_eq!(
            (info2.offset_x, info2.offset_y),
            (info.offset_x, info.offset_y)
        );
        let img2 = again.decode(idx, SurfaceKind::Image).unwrap().unwrap();
        let diff = dxt::mean_abs_diff(&img.rgba, &img2.rgba);
        assert!(diff < 4.0, "re-encoded sprite drifted by {diff}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn small_libraries_round_trip_byte_identical() {
        let Some(assets) = assets() else {
            eprintln!("ZIRCON_ASSETS not set; skipping");
            return;
        };
        let mut checked = 0;
        for entry in std::fs::read_dir(assets.join("Data")).unwrap() {
            let path = entry.unwrap().path();
            let is_zl = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("zl"))
                .unwrap_or(false);
            if !is_zl || path.metadata().unwrap().len() > 1_000_000 {
                continue;
            }
            let data = std::fs::read(&path).unwrap();
            let lib = ZlLibrary::open(&path).unwrap();
            let bytes = lib.to_builder().unwrap().to_bytes();
            assert!(bytes == data, "{} changed on round trip", path.display());
            checked += 1;
        }
        assert!(checked > 20, "libraries checked: {checked}");
    }

    /// Cut the unpadded `w` x `h` pixels out of a decoded (padded) surface.
    fn crop(s: &DecodedSurface, w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let o = ((y * s.width) * 4) as usize;
            out.extend_from_slice(&s.rgba[o..o + (w * 4) as usize]);
        }
        out
    }
}
