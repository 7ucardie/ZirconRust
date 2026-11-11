//! Editable PNG projects with explicit slots and lossless retention of untouched raw art.
use anyhow::{ensure, Context, Result};
use mir_formats::zl::{ImageInfo, RawImage, SurfaceKind, ZlBuilder, ZlLibrary};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{BufReader, Read, Write},
    path::{Component, Path, PathBuf},
};

const MAX_SOURCE: u64 = 512 * 1024 * 1024;
pub(crate) const MAX_SLOTS: usize = 131_072;
const MAX_DIM: u16 = 4096;
const MAX_PIXELS: u64 = 512 * 1024 * 1024;
const MAX_EDITED_PIXELS: usize = 64 * 1024 * 1024;
const MAX_PNG: u64 = 96 * 1024 * 1024;
const KINDS: [(SurfaceKind, &str); 3] = [
    (SurfaceKind::Image, "image"),
    (SurfaceKind::Shadow, "shadow"),
    (SurfaceKind::Overlay, "overlay"),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u16,
    pub library_version: u8,
    pub source_path: String,
    pub source_checksum: String,
    pub source_start: usize,
    pub source_slots: usize,
    pub original_checksum: String,
    pub slots: Vec<Slot>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    pub index: usize,
    pub present: bool,
    pub offset_x: i16,
    pub offset_y: i16,
    pub shadow_offset_x: i16,
    pub shadow_offset_y: i16,
    pub shadow_type: u8,
    pub image: Option<Surface>,
    pub shadow: Option<Surface>,
    pub overlay: Option<Surface>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    pub path: String,
    pub width: u16,
    pub height: u16,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub slots: usize,
    pub present: usize,
    pub surfaces: usize,
    pub changed_slots: usize,
    pub library_version: u8,
    pub source_checksum: String,
    pub output: PathBuf,
    pub library: Option<PathBuf>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetCell {
    pub slot: usize,
    pub layer: String,
    pub column: u32,
    pub row: u32,
}

/// Polygon coordinates use the original image's pixel grid. Negative points
/// can draw into explicitly requested canvas padding.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaintPolygon {
    pub slot: usize,
    pub points: Vec<[i16; 2]>,
    pub color: [u8; 4],
    #[serde(default)]
    pub under: bool,
}

/// A pose for one slot in an explicitly ordered animation run.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionKeyframe {
    pub slot: usize,
    pub points: Vec<[i16; 2]>,
    pub color: [u8; 4],
    #[serde(default)]
    pub under: bool,
}

impl Slot {
    pub fn surface(&self, layer: &str) -> Result<Option<&Surface>> {
        Ok(match layer {
            "image" => self.image.as_ref(),
            "shadow" => self.shadow.as_ref(),
            "overlay" => self.overlay.as_ref(),
            _ => anyhow::bail!("Layer must be image, shadow or overlay"),
        })
    }
    fn info(&self) -> ImageInfo {
        let dimensions = |surface: &Option<Surface>| {
            surface
                .as_ref()
                .map_or((0, 0), |surface| (surface.width, surface.height))
        };
        let (width, height) = dimensions(&self.image);
        let (shadow_width, shadow_height) = dimensions(&self.shadow);
        let (overlay_width, overlay_height) = dimensions(&self.overlay);
        ImageInfo {
            position: 0,
            width,
            height,
            offset_x: self.offset_x,
            offset_y: self.offset_y,
            shadow_type: self.shadow_type,
            shadow_width,
            shadow_height,
            shadow_offset_x: self.shadow_offset_x,
            shadow_offset_y: self.shadow_offset_y,
            overlay_width,
            overlay_height,
        }
    }
}

/// FNV-1a detects accidental source changes; this is not an authentication hash.
pub fn file_checksum(path: &Path) -> Result<String> {
    ensure!(
        std::fs::metadata(path)?.is_file(),
        "Checksum input must be a regular file"
    );
    let mut file = File::open(path)?;
    ensure!(file.metadata()?.len() <= MAX_SOURCE, "File exceeds 512 MiB");
    let mut checksum = 0xcbf29ce484222325u64;
    let mut bytes = [0; 65536];
    let mut total = 0;
    loop {
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        ensure!(total <= MAX_SOURCE, "File exceeds 512 MiB");
        for byte in &bytes[..count] {
            checksum = (checksum ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    Ok(format!("fnv1a64:{checksum:016x}"))
}
fn confined(project: &Path, relative: &str) -> Result<PathBuf> {
    ensure!(
        !relative.is_empty()
            && relative.len() <= 160
            && relative
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
            && Path::new(relative)
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "Project path must be relative without traversal"
    );
    let root = project.canonicalize()?;
    let path = root.join(relative).canonicalize()?;
    ensure!(
        path.starts_with(root) && path.is_file(),
        "Project file escapes its directory or is not a file"
    );
    Ok(path)
}
fn open_library(path: &Path) -> Result<ZlLibrary> {
    ensure!(
        std::fs::metadata(path)?.is_file(),
        "Library input must be a regular file"
    );
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    ensure!(length <= MAX_SOURCE, "Library exceeds 512 MiB");
    let mut prefix = [0; 8];
    file.read_exact(&mut prefix)?;
    let header = i32::from_le_bytes(prefix[..4].try_into()?);
    ensure!(
        header >= 4 && (header as u64) + 4 <= length && header as usize <= 4 + 26 * MAX_SLOTS,
        "Invalid or oversized library header"
    );
    let value = u32::from_le_bytes(prefix[4..].try_into()?);
    let count = if value >> 25 == 0 {
        value
    } else {
        value & 0x01ff_ffff
    };
    ensure!(count as usize <= MAX_SLOTS, "Library exceeds 131072 slots");
    let library = ZlLibrary::open(path)?;
    for index in 0..library.len() {
        if let Some(info) = library.info(index) {
            for (kind, _) in KINDS {
                let (w, h) = info.surface_dims(kind);
                ensure!(
                    w <= MAX_DIM && h <= MAX_DIM,
                    "Source slot {index} exceeds 4096 pixels"
                );
            }
        }
    }
    Ok(library)
}
fn fresh(output: &Path) -> Result<()> {
    ensure!(
        !output.exists(),
        "Output already exists; choose a new directory"
    );
    std::fs::create_dir(output)?;
    Ok(())
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = File::options().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn save_manifest(project: &Path, manifest: &Manifest) -> Result<()> {
    write_new(
        &project.join("project.json"),
        &serde_json::to_vec_pretty(manifest)?,
    )
}
pub fn load_manifest(project: &Path) -> Result<Manifest> {
    let path = confined(project, "project.json")?;
    let file = File::open(path)?;
    let mut bytes = vec![];
    file.take(32 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 32 * 1024 * 1024, "Manifest exceeds 32 MiB");
    Ok(serde_json::from_slice(&bytes)?)
}

pub(crate) fn write_png(path: &Path, width: u16, height: u16, rgba: &[u8]) -> Result<()> {
    ensure!(
        width > 0 && height > 0 && rgba.len() == usize::from(width) * usize::from(height) * 4,
        "Invalid RGBA dimensions"
    );
    let file = File::options().write(true).create_new(true).open(path)?;
    let mut encoder = png::Encoder::new(file, u32::from(width), u32::from(height));
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}
pub(crate) fn read_png(path: &Path) -> Result<(u16, u16, Vec<u8>)> {
    ensure!(
        std::fs::metadata(path)?.is_file(),
        "PNG input must be a regular file"
    );
    let file = File::open(path)?;
    ensure!(file.metadata()?.len() <= MAX_PNG, "PNG exceeds 96 MiB");
    let mut decoder = png::Decoder::new(BufReader::new(file));
    decoder.set_limits(png::Limits {
        bytes: MAX_PNG as usize,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let info = reader.info();
    ensure!(
        info.width > 0
            && info.height > 0
            && info.width <= u32::from(MAX_DIM)
            && info.height <= u32::from(MAX_DIM),
        "PNG dimensions must be 1–4096"
    );
    ensure!(
        info.animation_control.is_none(),
        "Import a static PNG, not APNG"
    );
    let size = reader
        .output_buffer_size()
        .context("PNG allocation overflow")?;
    ensure!(size <= MAX_PNG as usize, "PNG decoded size exceeds limit");
    let mut bytes = vec![0; size];
    let info = reader.next_frame(&mut bytes)?;
    bytes.truncate(info.buffer_size());
    let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    match info.color_type {
        png::ColorType::Rgba => rgba = bytes,
        png::ColorType::Rgb => {
            for pixel in bytes.as_chunks::<3>().0.iter() {
                rgba.extend_from_slice(pixel);
                rgba.push(255);
            }
        }
        png::ColorType::Grayscale => {
            for value in bytes {
                rgba.extend_from_slice(&[value, value, value, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for pixel in bytes.as_chunks::<2>().0.iter() {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
        }
        _ => anyhow::bail!("PNG conversion did not produce eight-bit pixels"),
    }
    Ok((info.width as u16, info.height as u16, rgba))
}
pub(crate) fn pixels(
    library: &ZlLibrary,
    index: usize,
    kind: SurfaceKind,
) -> Result<Option<Vec<u8>>> {
    let Some(surface) = library.decode(index, kind)? else {
        return Ok(None);
    };
    let (w, h) = library.info(index).unwrap().surface_dims(kind);
    let mut rgba = Vec::with_capacity(usize::from(w) * usize::from(h) * 4);
    for y in 0..usize::from(h) {
        let start = y * surface.width as usize * 4;
        rgba.extend_from_slice(&surface.rgba[start..start + usize::from(w) * 4]);
    }
    Ok(Some(rgba))
}
fn slot_from(library: &ZlLibrary, index: usize) -> Slot {
    let info = library.info(index).copied().unwrap_or_default();
    let surface = |kind, name| {
        info.has_surface(kind).then(|| {
            let (width, height) = info.surface_dims(kind);
            Surface {
                path: format!("frames/{index:06}-{name}.png"),
                width,
                height,
            }
        })
    };
    Slot {
        index,
        present: library.info(index).is_some(),
        offset_x: info.offset_x,
        offset_y: info.offset_y,
        shadow_offset_x: info.shadow_offset_x,
        shadow_offset_y: info.shadow_offset_y,
        shadow_type: info.shadow_type,
        image: surface(SurfaceKind::Image, "image"),
        shadow: surface(SurfaceKind::Shadow, "shadow"),
        overlay: surface(SurfaceKind::Overlay, "overlay"),
    }
}
fn report(project: &Path, manifest: &Manifest) -> Report {
    Report {
        slots: manifest.slots.len(),
        present: manifest.slots.iter().filter(|slot| slot.present).count(),
        surfaces: manifest
            .slots
            .iter()
            .map(|slot| {
                usize::from(slot.image.is_some())
                    + usize::from(slot.shadow.is_some())
                    + usize::from(slot.overlay.is_some())
            })
            .sum(),
        changed_slots: 0,
        library_version: manifest.library_version,
        source_checksum: manifest.source_checksum.clone(),
        output: project.into(),
        library: None,
    }
}
#[cfg(test)]
pub fn export(library: &Path, output: &Path) -> Result<Report> {
    let source = open_library(library)?;
    export_range(library, 0, source.len(), output)
}
pub fn export_range(library: &Path, start: usize, count: usize, output: &Path) -> Result<Report> {
    let source = open_library(library)?;
    ensure!(
        count > 0
            && count <= MAX_SLOTS
            && start
                .checked_add(count)
                .is_some_and(|end| end <= source.len()),
        "Invalid source slot range"
    );
    let mut total = 0u64;
    for index in start..start + count {
        if let Some(info) = source.info(index) {
            for (kind, _) in KINDS {
                if info.has_surface(kind) {
                    let (w, h) = info.surface_dims(kind);
                    total += u64::from(w) * u64::from(h);
                }
            }
        }
    }
    ensure!(
        total <= MAX_PIXELS,
        "Export exceeds 512 million surface pixels; select a smaller range"
    );
    let checksum = file_checksum(library)?;
    let mut original = ZlBuilder::new(source.version());
    for index in start..start + count {
        original.push_raw(source.raw(index)?);
    }
    fresh(output)?;
    std::fs::create_dir(output.join("frames"))?;
    write_new(&output.join("source.zl"), &original.to_bytes())?;
    let selected = open_library(&output.join("source.zl"))?;
    let slots: Vec<_> = (0..selected.len())
        .map(|index| slot_from(&selected, index))
        .collect();
    for slot in &slots {
        for (kind, name) in KINDS {
            if let Some(surface) = slot.surface(name)? {
                write_png(
                    &output.join(&surface.path),
                    surface.width,
                    surface.height,
                    &pixels(&selected, slot.index, kind)?.context("Missing source surface")?,
                )?;
            }
        }
    }
    let manifest = Manifest {
        version: 1,
        library_version: source.version(),
        source_path: library
            .canonicalize()?
            .to_str()
            .context("Source path must be UTF-8")?
            .into(),
        source_checksum: checksum,
        source_start: start,
        source_slots: source.len(),
        original_checksum: file_checksum(&output.join("source.zl"))?,
        slots,
    };
    save_manifest(output, &manifest)?;
    Ok(report(output, &manifest))
}
fn checked(project: &Path) -> Result<(Manifest, ZlLibrary)> {
    let manifest = load_manifest(project)?;
    ensure!(manifest.version == 1, "Unsupported sprite project version");
    ensure!(
        !manifest.slots.is_empty()
            && manifest.slots.len() <= MAX_SLOTS
            && manifest.source_slots <= MAX_SLOTS
            && manifest
                .source_start
                .checked_add(manifest.slots.len())
                .is_some_and(|end| end <= manifest.source_slots),
        "Invalid project slot range"
    );
    let source_path = confined(project, "source.zl")?;
    ensure!(
        file_checksum(&source_path)? == manifest.original_checksum,
        "Original raw library checksum changed"
    );
    let source = open_library(&source_path)?;
    ensure!(
        source.len() == manifest.slots.len() && source.version() == manifest.library_version,
        "Original library version or slot count changed"
    );
    let mut paths = BTreeSet::new();
    let mut pixels = 0u64;
    for (index, slot) in manifest.slots.iter().enumerate() {
        ensure!(
            slot.index == index,
            "Slot indices must be complete, ordered and unique"
        );
        ensure!(
            slot.present
                || (slot.offset_x == 0
                    && slot.offset_y == 0
                    && slot.shadow_offset_x == 0
                    && slot.shadow_offset_y == 0
                    && slot.shadow_type == 0),
            "Absent slots cannot carry offsets or shadow metadata"
        );
        for (_, name) in KINDS {
            if let Some(surface) = slot.surface(name)? {
                ensure!(slot.present, "Absent slot {index} cannot contain a surface");
                ensure!(
                    surface.width > 0
                        && surface.height > 0
                        && surface.width <= MAX_DIM
                        && surface.height <= MAX_DIM,
                    "Invalid surface dimensions in slot {index}"
                );
                ensure!(
                    surface.path.starts_with("frames/")
                        && surface.path.ends_with(".png")
                        && surface
                            .path
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"/_-.".contains(&byte)),
                    "PNG paths must use safe names under frames/"
                );
                ensure!(
                    paths.insert(surface.path.clone()),
                    "Duplicate PNG path in manifest"
                );
                pixels += u64::from(surface.width) * u64::from(surface.height);
            }
        }
    }
    ensure!(pixels <= MAX_PIXELS, "Project exceeds 512 million pixels");
    Ok((manifest, source))
}
fn load_surface(project: &Path, surface: &Surface, version: u8) -> Result<Vec<u8>> {
    let (w, h, rgba) = read_png(&confined(project, &surface.path)?)?;
    ensure!(
        (w, h) == (surface.width, surface.height),
        "PNG dimensions differ from manifest: {}",
        surface.path
    );
    ensure!(
        version > 0
            || rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 0 || pixel[3] == 255),
        "DXT1 projects require binary alpha; use a compatible DXT5 source for partial transparency"
    );
    Ok(rgba)
}
pub fn validate(project: &Path) -> Result<Report> {
    let (manifest, source) = checked(project)?;
    let mut result = report(project, &manifest);
    for slot in &manifest.slots {
        let baseline = slot_from(&source, slot.index);
        let mut same = same_metadata(slot, &baseline);
        for (kind, name) in KINDS {
            match (slot.surface(name)?, baseline.surface(name)?) {
                (Some(surface), original) => {
                    let rgba = load_surface(project, surface, manifest.library_version)?;
                    same &= original.is_some_and(|original| {
                        (surface.width, surface.height) == (original.width, original.height)
                    }) && pixels(&source, slot.index, kind)?.as_deref() == Some(&rgba);
                }
                (None, Some(_)) => same = false,
                (None, None) => {}
            }
        }
        result.changed_slots += usize::from(!same);
    }
    Ok(result)
}

fn same_metadata(slot: &Slot, baseline: &Slot) -> bool {
    slot.present == baseline.present
        && slot.offset_x == baseline.offset_x
        && slot.offset_y == baseline.offset_y
        && slot.shadow_offset_x == baseline.shadow_offset_x
        && slot.shadow_offset_y == baseline.shadow_offset_y
        && slot.shadow_type == baseline.shadow_type
}

struct Expected {
    index: usize,
    kind: SurfaceKind,
    width: u16,
    height: u16,
    rgba: Vec<u8>,
}
fn raw_surface(raw: &RawImage, wanted: SurfaceKind, version: u8) -> Result<&[u8]> {
    let mut offset = 0;
    for (kind, _) in KINDS {
        let (width, height) = raw.info.surface_dims(kind);
        let size = usize::from(width).div_ceil(4)
            * usize::from(height).div_ceil(4)
            * if version > 0 { 16 } else { 8 };
        if kind == wanted {
            return raw
                .payload
                .get(offset..offset + size)
                .context("Original surface payload is truncated");
        }
        offset += size;
    }
    unreachable!("All surface kinds are enumerated")
}
pub fn build(project: &Path, output: &Path) -> Result<Report> {
    let (manifest, source) = checked(project)?;
    let mut builder = ZlBuilder::new(manifest.library_version);
    let mut expected = vec![];
    let mut edited_pixels = 0;
    let mut changed = 0;
    for slot in &manifest.slots {
        let original = source.raw(slot.index)?;
        let baseline = slot_from(&source, slot.index);
        let mut unchanged = same_metadata(slot, &baseline);
        let mut loaded = vec![];
        for (kind, name) in KINDS {
            match (slot.surface(name)?, baseline.surface(name)?) {
                (Some(surface), original_surface) => {
                    let rgba = load_surface(project, surface, manifest.library_version)?;
                    let same_pixels = original_surface.is_some_and(|original| {
                        (surface.width, surface.height) == (original.width, original.height)
                    }) && pixels(&source, slot.index, kind)?.as_deref()
                        == Some(&rgba);
                    unchanged &= same_pixels;
                    loaded.push((kind, surface, rgba, same_pixels));
                }
                (None, Some(_)) => unchanged = false,
                (None, None) => {}
            }
        }
        if unchanged {
            builder.push_raw(original);
            continue;
        }
        changed += 1;
        if !slot.present {
            builder.push_raw(None);
            continue;
        }
        let mut raw = RawImage {
            info: slot.info(),
            payload: vec![],
        };
        for (kind, surface, rgba, same_pixels) in loaded {
            if same_pixels {
                raw.payload.extend_from_slice(raw_surface(
                    original.as_ref().unwrap(),
                    kind,
                    manifest.library_version,
                )?);
                continue;
            }
            edited_pixels += rgba.len() / 4;
            ensure!(
                edited_pixels <= MAX_EDITED_PIXELS,
                "Build exceeds 64 million edited pixels; split the project range"
            );
            raw.payload
                .extend(builder.encode_surface(&rgba, surface.width, surface.height));
            expected.push(Expected {
                index: slot.index,
                kind,
                width: surface.width,
                height: surface.height,
                rgba,
            });
        }
        builder.push_raw(Some(raw));
    }
    let payload_bytes: u64 = builder
        .images()
        .iter()
        .flatten()
        .map(|raw| raw.payload.len() as u64)
        .sum();
    ensure!(
        payload_bytes + 8 + builder.len() as u64 * 26 <= MAX_SOURCE,
        "Built library exceeds 512 MiB"
    );
    fresh(output)?;
    let library = output.join("Library.Zl");
    write_new(&library, &builder.to_bytes())?;
    let decoded = open_library(&library)?;
    ensure!(
        decoded.len() == builder.len() && decoded.version() == builder.version(),
        "Built library identity mismatch"
    );
    for (index, wanted) in builder.images().iter().enumerate() {
        let actual = decoded.raw(index)?;
        match (wanted, &actual) {
            (None, None) => {}
            (Some(wanted), Some(actual)) => {
                ensure!(
                    actual.payload == wanted.payload,
                    "Built raw slot {index} changed"
                );
                let a = actual.info;
                let b = wanted.info;
                ensure!(
                    (
                        a.width,
                        a.height,
                        a.offset_x,
                        a.offset_y,
                        a.shadow_width,
                        a.shadow_height,
                        a.shadow_offset_x,
                        a.shadow_offset_y,
                        a.shadow_type,
                        a.overlay_width,
                        a.overlay_height
                    ) == (
                        b.width,
                        b.height,
                        b.offset_x,
                        b.offset_y,
                        b.shadow_width,
                        b.shadow_height,
                        b.shadow_offset_x,
                        b.shadow_offset_y,
                        b.shadow_type,
                        b.overlay_width,
                        b.overlay_height
                    ),
                    "Built slot metadata mismatch"
                );
            }
            _ => anyhow::bail!("Built slot presence changed"),
        }
    }
    for frame in expected {
        let actual = pixels(&decoded, frame.index, frame.kind)?.context("Built surface missing")?;
        ensure!(
            actual.len() == frame.rgba.len()
                && actual.len() == usize::from(frame.width) * usize::from(frame.height) * 4,
            "Built surface dimensions changed"
        );
        let mut colour_error = 0u64;
        let mut colours = 0u64;
        for (want, got) in frame
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(actual.as_chunks::<4>().0.iter())
        {
            ensure!(
                want[3].abs_diff(got[3]) <= 40 && (want[3] != 0 || got[3] == 0),
                "Built alpha/shape exceeds compression tolerance at slot {}",
                frame.index
            );
            if want[3] > 0 {
                for c in 0..3 {
                    colour_error += u64::from(want[c].abs_diff(got[c]));
                    colours += 1;
                }
            }
        }
        ensure!(
            colours == 0 || colour_error <= 48 * colours,
            "Built colours exceed compression tolerance at slot {}",
            frame.index
        );
    }
    let mut result = report(output, &manifest);
    result.changed_slots = changed;
    result.library = Some(library);
    write_new(
        &output.join("build.json"),
        &serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}

fn clone_project(project: &Path, output: &Path, manifest: &Manifest) -> Result<()> {
    let mut extras = vec![];
    for name in [
        "animation.json",
        "contract.json",
        "identity.json",
        "reference.json",
        "reference.png",
    ] {
        if project.join(name).symlink_metadata().is_ok() {
            let path = confined(project, name)?;
            ensure!(
                std::fs::metadata(&path)?.len() <= 32 * 1024 * 1024,
                "Reference metadata exceeds 32 MiB"
            );
            extras.push((name, path));
        }
    }
    fresh(output)?;
    std::fs::create_dir(output.join("frames"))?;
    std::fs::copy(confined(project, "source.zl")?, output.join("source.zl"))?;
    for slot in &manifest.slots {
        for (_, name) in KINDS {
            if let Some(surface) = slot.surface(name)? {
                let target = output.join(&surface.path);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(confined(project, &surface.path)?, target)?;
            }
        }
    }
    for (name, path) in extras {
        std::fs::copy(path, output.join(name))?;
    }
    Ok(())
}
/// Apply one consistent colour treatment across every frame of one layer.
/// Geometry, alpha, other layers and the reference source remain unchanged.
pub fn tint(project: &Path, output: &Path, layer: &str, rgb: [u8; 3]) -> Result<Report> {
    ensure!(
        matches!(layer, "image" | "overlay"),
        "Tint layer must be image or overlay"
    );
    validate(project)?;
    let manifest = load_manifest(project)?;
    ensure!(
        std::fs::symlink_metadata(output).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "Tint requires a new revision"
    );
    let result = (|| -> Result<Report> {
        clone_project(project, output, &manifest)?;
        let mut changed = 0;
        for slot in &manifest.slots {
            let Some(surface) = slot.surface(layer)? else {
                continue;
            };
            if rgb == [255; 3] {
                continue;
            }
            let path = confined(output, &surface.path)?;
            let (w, h, mut rgba) = read_png(&path)?;
            let mut different = false;
            for pixel in rgba.as_chunks_mut::<4>().0 {
                if pixel[3] == 0 {
                    continue;
                }
                for channel in 0..3 {
                    let value =
                        ((u16::from(pixel[channel]) * u16::from(rgb[channel]) + 127) / 255) as u8;
                    different |= value != pixel[channel];
                    pixel[channel] = value;
                }
            }
            if different {
                std::fs::remove_file(&path)?;
                write_png(&path, w, h, &rgba)?;
                changed += 1;
            }
        }
        save_manifest(output, &manifest)?;
        let mut result = validate(output)?;
        result.changed_slots = changed;
        Ok(result)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(output);
    }
    result
}

/// Draw editable polygons across selected image frames in one new revision.
/// Padding grows the canvas while preserving the existing ground anchor.
pub fn paint(
    project: &Path,
    output: &Path,
    padding: [u8; 4],
    polygons: &[PaintPolygon],
) -> Result<Report> {
    ensure!(
        std::fs::symlink_metadata(output).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "Paint requires a new revision"
    );
    ensure!(
        !polygons.is_empty() && polygons.len() <= 4096,
        "Paint requires 1–4096 polygons"
    );
    ensure!(
        padding.into_iter().all(|value| value <= 64),
        "Paint padding must be at most 64 pixels per side"
    );
    validate(project)?;
    let mut manifest = load_manifest(project)?;
    let mut grouped = std::collections::BTreeMap::<usize, Vec<&PaintPolygon>>::new();
    for polygon in polygons {
        ensure!(
            (3..=64).contains(&polygon.points.len()),
            "Paint polygon needs 3–64 points"
        );
        ensure!(
            matches!(polygon.color[3], 0 | 255),
            "Paint uses opaque colour or transparent erase"
        );
        let slot = manifest
            .slots
            .get(polygon.slot)
            .context("Paint slot outside project")?;
        ensure!(slot.image.is_some(), "Paint slot has no image canvas");
        grouped.entry(polygon.slot).or_default().push(polygon);
    }
    let [left, top, right, bottom] = padding.map(i32::from);
    let mut pixel_budget = 0u64;
    let mut draw_work = 0u64;
    for (&index, shapes) in &grouped {
        let slot = &manifest.slots[index];
        let image = slot.image.as_ref().unwrap();
        let width = i32::from(image.width) + left + right;
        let height = i32::from(image.height) + top + bottom;
        ensure!(
            width <= 4096 && height <= 4096,
            "Paint canvas exceeds 4096 pixels"
        );
        ensure!(
            i32::from(slot.offset_x) - left >= i32::from(i16::MIN)
                && i32::from(slot.offset_y) - top >= i32::from(i16::MIN),
            "Paint anchor exceeds signed 16-bit range"
        );
        pixel_budget += (width * height) as u64;
        for shape in shapes {
            let min_x = shape
                .points
                .iter()
                .map(|point| i32::from(point[0]) + left)
                .min()
                .unwrap()
                .clamp(0, width);
            let max_x = shape
                .points
                .iter()
                .map(|point| i32::from(point[0]) + left)
                .max()
                .unwrap()
                .clamp(0, width);
            let min_y = shape
                .points
                .iter()
                .map(|point| i32::from(point[1]) + top)
                .min()
                .unwrap()
                .clamp(0, height);
            let max_y = shape
                .points
                .iter()
                .map(|point| i32::from(point[1]) + top)
                .max()
                .unwrap()
                .clamp(0, height);
            draw_work += u64::from((max_x - min_x).max(0) as u32)
                * u64::from((max_y - min_y).max(0) as u32)
                * shape.points.len() as u64;
        }
    }
    ensure!(
        pixel_budget <= 64 * 1024 * 1024,
        "Paint exceeds 64 million pixels"
    );
    ensure!(
        draw_work <= 128 * 1024 * 1024,
        "Paint exceeds 128 million edge checks"
    );
    let result = (|| -> Result<Report> {
        clone_project(project, output, &manifest)?;
        for (index, shapes) in grouped {
            let slot = &mut manifest.slots[index];
            let image = slot.image.as_mut().unwrap();
            let path = confined(output, &image.path)?;
            let (w, h, source) = read_png(&path)?;
            let width = usize::from(w) + (left + right) as usize;
            let height = usize::from(h) + (top + bottom) as usize;
            let mut rgba = vec![0; width * height * 4];
            for y in 0..usize::from(h) {
                let src = y * usize::from(w) * 4;
                let dst = ((y + top as usize) * width + left as usize) * 4;
                rgba[dst..dst + usize::from(w) * 4]
                    .copy_from_slice(&source[src..src + usize::from(w) * 4]);
            }
            for shape in shapes {
                fill_polygon(&mut rgba, width, height, shape, left, top);
            }
            std::fs::remove_file(&path)?;
            write_png(&path, width as u16, height as u16, &rgba)?;
            image.width = width as u16;
            image.height = height as u16;
            slot.offset_x -= left as i16;
            slot.offset_y -= top as i16;
        }
        save_manifest(output, &manifest)?;
        validate(output)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(output);
    }
    result
}

/// Fill an animation run from a few polygon poses, then use the normal paint
/// revision path so source art, anchors and untouched layers stay intact.
pub fn paint_motion(
    project: &Path,
    output: &Path,
    padding: [u8; 4],
    slots: &[usize],
    keyframes: &[MotionKeyframe],
) -> Result<Report> {
    ensure!(
        (2..=4096).contains(&slots.len()),
        "Motion needs 2–4096 slots"
    );
    ensure!(
        (2..=slots.len()).contains(&keyframes.len()),
        "Motion needs at least two keyframes"
    );
    validate(project)?;
    let manifest = load_manifest(project)?;
    let contract_path = confined(project, "contract.json")?;
    ensure!(
        std::fs::metadata(&contract_path)?.len() <= 16 * 1024 * 1024,
        "Animation contract exceeds 16 MiB"
    );
    let contract: crate::sprite_contract::Contract =
        serde_json::from_slice(&std::fs::read(contract_path)?)?;
    ensure!(
        contract.source_checksum == manifest.source_checksum
            && contract.source_start == manifest.source_start
            && contract.source_slots == manifest.slots.len(),
        "Animation contract does not match project source"
    );
    ensure!(
        contract.frames.windows(slots.len()).any(|run| {
            let first = &run[0];
            run.iter().zip(slots).all(|(frame, &slot)| {
                frame.action == first.action
                    && frame.direction == first.direction
                    && frame.index.checked_sub(manifest.source_start) == Some(slot)
            }) && run
                .windows(2)
                .all(|pair| pair[0].frame.checked_add(1) == Some(pair[1].frame))
        }),
        "Motion slots must be a contiguous run in one animation and direction"
    );
    let mut positions = Vec::with_capacity(keyframes.len());
    let point_count = keyframes[0].points.len();
    for key in keyframes {
        ensure!(
            key.points.len() == point_count && (3..=64).contains(&point_count),
            "Motion keyframes need matching polygons with 3–64 points"
        );
        ensure!(
            matches!(key.color[3], 0 | 255),
            "Motion keyframe alpha must be transparent or opaque"
        );
        let position = slots
            .iter()
            .position(|&slot| slot == key.slot)
            .context("Motion keyframe slot is outside the animation run")?;
        ensure!(
            positions.last().is_none_or(|&last| position > last),
            "Motion keyframes must follow slot order without duplicates"
        );
        positions.push(position);
    }
    ensure!(
        positions[0] == 0 && positions.last() == Some(&(slots.len() - 1)),
        "Motion keyframes must include first and last slots"
    );
    let mut polygons = Vec::with_capacity(slots.len());
    for (segment, pair) in keyframes.windows(2).enumerate() {
        let start = positions[segment];
        let end = positions[segment + 1];
        let span = (end - start) as u32;
        for (index, &slot) in slots.iter().enumerate().take(end).skip(start) {
            let step = (index - start) as u32;
            let points = pair[0]
                .points
                .iter()
                .zip(&pair[1].points)
                .map(|(a, b)| {
                    [
                        (f64::from(a[0])
                            + (f64::from(b[0]) - f64::from(a[0])) * f64::from(step)
                                / f64::from(span))
                        .round() as i16,
                        (f64::from(a[1])
                            + (f64::from(b[1]) - f64::from(a[1])) * f64::from(step)
                                / f64::from(span))
                        .round() as i16,
                    ]
                })
                .collect();
            let mut color = [0; 4];
            for (channel, value) in color.iter_mut().enumerate() {
                *value = ((u32::from(pair[0].color[channel]) * (span - step)
                    + u32::from(pair[1].color[channel]) * step
                    + span / 2)
                    / span) as u8;
            }
            // The sprite library stores binary alpha. Threshold the interpolated
            // value; under is a discrete compositing mode, switched at midpoint.
            color[3] = if color[3] >= 128 { 255 } else { 0 };
            polygons.push(PaintPolygon {
                slot,
                points,
                color,
                under: if step * 2 < span {
                    pair[0].under
                } else {
                    pair[1].under
                },
            });
        }
    }
    let last = keyframes.last().unwrap();
    polygons.push(PaintPolygon {
        slot: last.slot,
        points: last.points.clone(),
        color: last.color,
        under: last.under,
    });
    paint(project, output, padding, &polygons)
}

/// Design cloth motion from one drawn pose and the selected runtime action.
/// The top edge stays attached; lower vertices sway or recoil. All safety and
/// revision checks remain in `paint_motion`.
pub fn paint_auto_motion(
    project: &Path,
    output: &Path,
    padding: [u8; 4],
    slots: &[usize],
    seed: &MotionKeyframe,
    strength: u8,
) -> Result<Report> {
    ensure!(
        (2..=4096).contains(&slots.len()),
        "Motion needs 2–4096 slots"
    );
    ensure!((1..=12).contains(&strength), "Motion strength must be 1–12");
    ensure!(
        slots.contains(&seed.slot),
        "Motion seed must belong to selected run"
    );
    ensure!(
        (3..=64).contains(&seed.points.len()) && matches!(seed.color[3], 0 | 255),
        "Motion seed needs 3–64 points and binary alpha"
    );
    let manifest = load_manifest(project)?;
    ensure!(
        slots.iter().all(|&slot| slot < manifest.slots.len()),
        "Motion slot outside project"
    );
    let seed_anchor = &manifest.slots[seed.slot];
    let contract_path = confined(project, "contract.json")?;
    ensure!(
        std::fs::metadata(&contract_path)?.len() <= 16 * 1024 * 1024,
        "Animation contract exceeds 16 MiB"
    );
    let contract: crate::sprite_contract::Contract =
        serde_json::from_slice(&std::fs::read(contract_path)?)?;
    let run = contract
        .frames
        .windows(slots.len())
        .find(|run| {
            run.iter()
                .zip(slots)
                .all(|(frame, &slot)| frame.index.checked_sub(manifest.source_start) == Some(slot))
        })
        .context("Motion slots are not in the animation contract")?;
    let action = &run[0].action;
    let direction = run[0].direction;
    let top = seed.points.iter().map(|point| point[1]).min().unwrap_or(0);
    let bottom = seed
        .points
        .iter()
        .map(|point| point[1])
        .max()
        .unwrap_or(top);
    let height = f64::from((i32::from(bottom) - i32::from(top)).max(1));
    let action = action.to_ascii_lowercase();
    let sign = if (5..=7).contains(&direction) {
        -1.0
    } else {
        1.0
    };
    let count = slots.len() as f64;
    let keyframes: Vec<_> = slots
        .iter()
        .enumerate()
        .map(|(index, &slot)| {
            let frame_anchor = &manifest.slots[slot];
            let anchor_x =
                f64::from(i32::from(seed_anchor.offset_x) - i32::from(frame_anchor.offset_x));
            let anchor_y =
                f64::from(i32::from(seed_anchor.offset_y) - i32::from(frame_anchor.offset_y));
            let phase = std::f64::consts::TAU * index as f64 / count;
            let progress = index as f64 / (count - 1.0);
            let power = f64::from(strength);
            let (sway, drop) = if action.contains("running") || action.contains("dash") {
                (power * 1.5 * phase.sin(), power * 0.35 * phase.cos())
            } else if action.contains("walking") {
                (power * phase.sin(), power * 0.2 * phase.cos())
            } else if action.contains("struck") {
                (
                    -power * (std::f64::consts::PI * progress).sin(),
                    power * 0.3 * progress,
                )
            } else if action.contains("die") || action.contains("dead") {
                (power * 0.25 * progress, power * progress)
            } else if action.contains("attack")
                || action.contains("cast")
                || action.contains("mining")
                || action.contains("harvest")
                || action.contains("fishingreel")
            {
                (
                    power * (std::f64::consts::PI * progress).sin(),
                    -power * 0.25 * (std::f64::consts::PI * progress).sin(),
                )
            } else {
                ((power / 3.0).max(1.0) * phase.sin(), 0.0)
            };
            let points = seed
                .points
                .iter()
                .map(|point| {
                    let loose = f64::from(i32::from(point[1]) - i32::from(top)) / height;
                    [
                        (f64::from(point[0]) + anchor_x + sway * loose * sign)
                            .round()
                            .clamp(f64::from(i16::MIN), f64::from(i16::MAX))
                            as i16,
                        (f64::from(point[1]) + anchor_y + drop * loose)
                            .round()
                            .clamp(f64::from(i16::MIN), f64::from(i16::MAX))
                            as i16,
                    ]
                })
                .collect();
            MotionKeyframe {
                slot,
                points,
                color: seed.color,
                under: seed.under,
            }
        })
        .collect();
    paint_motion(project, output, padding, slots, &keyframes)
}

pub(crate) fn fill_polygon(
    rgba: &mut [u8],
    width: usize,
    height: usize,
    shape: &PaintPolygon,
    left: i32,
    top: i32,
) {
    let min_x = shape
        .points
        .iter()
        .map(|point| i32::from(point[0]) + left)
        .min()
        .unwrap()
        .max(0);
    let max_x = shape
        .points
        .iter()
        .map(|point| i32::from(point[0]) + left)
        .max()
        .unwrap()
        .min(width as i32);
    let min_y = shape
        .points
        .iter()
        .map(|point| i32::from(point[1]) + top)
        .min()
        .unwrap()
        .max(0);
    let max_y = shape
        .points
        .iter()
        .map(|point| i32::from(point[1]) + top)
        .max()
        .unwrap()
        .min(height as i32);
    for y in min_y..max_y {
        for x in min_x..max_x {
            let px = f64::from(x - left) + 0.5;
            let py = f64::from(y - top) + 0.5;
            let mut inside = false;
            for edge in 0..shape.points.len() {
                let a = shape.points[edge];
                let b = shape.points[(edge + 1) % shape.points.len()];
                let (ax, ay) = (f64::from(a[0]), f64::from(a[1]));
                let (bx, by) = (f64::from(b[0]), f64::from(b[1]));
                if (ay > py) != (by > py) && px < ax + (py - ay) * (bx - ax) / (by - ay) {
                    inside = !inside;
                }
            }
            if inside {
                let pixel = &mut rgba[(y as usize * width + x as usize) * 4..][..4];
                if !shape.under || pixel[3] == 0 {
                    pixel.copy_from_slice(&shape.color);
                }
            }
        }
    }
}

fn fitted(rgba: &[u8], w: u16, h: u16, tw: u16, th: u16) -> Vec<u8> {
    let ratio = (f64::from(tw) / f64::from(w)).min(f64::from(th) / f64::from(h));
    let nw = (f64::from(w) * ratio).round().max(1.0) as usize;
    let nh = (f64::from(h) * ratio).round().max(1.0) as usize;
    let mut out = vec![0; usize::from(tw) * usize::from(th) * 4];
    let left = (usize::from(tw) - nw) / 2;
    let top = usize::from(th) - nh;
    for y in 0..nh {
        for x in 0..nw {
            let from = ((y * usize::from(h) / nh) * usize::from(w) + x * usize::from(w) / nw) * 4;
            let to = ((y + top) * usize::from(tw) + x + left) * 4;
            out[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
        }
    }
    out
}
fn prepared(
    mut rgba: Vec<u8>,
    w: u16,
    h: u16,
    tw: u16,
    th: u16,
    options: ImportOptions,
) -> Result<Vec<u8>> {
    ensure!(
        !options.trim_transparent || options.fit,
        "Trimming transparent margins requires explicit fit"
    );
    if options.binary_alpha {
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel[3] = if pixel[3] >= 128 { 255 } else { 0 };
        }
    }
    let (mut width, mut height) = (w, h);
    if options.trim_transparent {
        let (mut left, mut top, mut right, mut bottom) = (w, h, 0, 0);
        for y in 0..h {
            for x in 0..w {
                if rgba[(usize::from(y) * usize::from(w) + usize::from(x)) * 4 + 3] > 0 {
                    left = left.min(x);
                    top = top.min(y);
                    right = right.max(x + 1);
                    bottom = bottom.max(y + 1);
                }
            }
        }
        ensure!(
            right > left && bottom > top,
            "Visible-art fitting requires nontransparent pixels"
        );
        width = right - left;
        height = bottom - top;
        let mut cropped = Vec::with_capacity(usize::from(width) * usize::from(height) * 4);
        for y in top..bottom {
            let start = (usize::from(y) * usize::from(w) + usize::from(left)) * 4;
            cropped.extend_from_slice(&rgba[start..start + usize::from(width) * 4]);
        }
        rgba = cropped;
    }
    Ok(if options.fit {
        fitted(&rgba, width, height, tw, th)
    } else {
        rgba
    })
}
#[cfg(test)]
pub fn import_image(
    project: &Path,
    slot: usize,
    layer: &str,
    input: &Path,
    output: &Path,
) -> Result<Report> {
    import_image_with_fit(project, slot, layer, input, output, false)
}
#[cfg(test)]
pub fn import_image_with_fit(
    project: &Path,
    slot: usize,
    layer: &str,
    input: &Path,
    output: &Path,
    fit: bool,
) -> Result<Report> {
    import_image_options(
        project,
        slot,
        layer,
        input,
        output,
        ImportOptions {
            fit,
            binary_alpha: false,
            trim_transparent: false,
        },
    )
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ImportOptions {
    pub fit: bool,
    pub binary_alpha: bool,
    pub trim_transparent: bool,
}
pub fn import_image_options(
    project: &Path,
    slot: usize,
    layer: &str,
    input: &Path,
    output: &Path,
    options: ImportOptions,
) -> Result<Report> {
    let fit = options.fit;
    let (manifest, _) = checked(project)?;
    let surface = manifest
        .slots
        .get(slot)
        .context("Slot outside project")?
        .surface(layer)?
        .context("Surface absent; declare its canvas in project.json first")?;
    let (w, h, rgba) = read_png(input)?;
    ensure!(
        fit || (w, h) == (surface.width, surface.height),
        "Input dimensions differ; explicitly choose fit or edit the canvas dimensions first"
    );
    let rgba = prepared(rgba, w, h, surface.width, surface.height, options)?;
    ensure!(
        manifest.library_version > 0
            || rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 0 || pixel[3] == 255),
        "DXT1 projects require binary alpha"
    );
    validate(project)?;
    clone_project(project, output, &manifest)?;
    std::fs::remove_file(output.join(&surface.path))?;
    write_png(
        &output.join(&surface.path),
        surface.width,
        surface.height,
        &rgba,
    )?;
    save_manifest(output, &manifest)?;
    validate(output)
}
#[cfg(test)]
pub fn import_sheet(
    project: &Path,
    input: &Path,
    cell_width: u16,
    cell_height: u16,
    mapping: &[SheetCell],
    output: &Path,
) -> Result<Report> {
    import_sheet_options(
        project,
        input,
        cell_width,
        cell_height,
        mapping,
        output,
        ImportOptions::default(),
    )
}
pub fn import_sheet_options(
    project: &Path,
    input: &Path,
    cell_width: u16,
    cell_height: u16,
    mapping: &[SheetCell],
    output: &Path,
    options: ImportOptions,
) -> Result<Report> {
    ensure!(
        cell_width > 0 && cell_height > 0 && !mapping.is_empty() && mapping.len() <= 4096,
        "Sheet needs nonzero cells and 1–4096 explicit mappings"
    );
    let (manifest, _) = checked(project)?;
    let (w, h, rgba) = read_png(input)?;
    ensure!(
        cell_width <= MAX_DIM && cell_height <= MAX_DIM,
        "Sheet cells exceed4096 pixels"
    );
    let mut destinations = BTreeSet::new();
    let mut frames = vec![];
    let mut imported_pixels = 0u64;
    for cell in mapping {
        ensure!(
            destinations.insert((cell.slot, cell.layer.clone())),
            "Duplicate destination slot/layer in sheet mapping"
        );
        let surface = manifest
            .slots
            .get(cell.slot)
            .context("Sheet slot outside project")?
            .surface(&cell.layer)?
            .context("Sheet destination surface is absent")?;
        imported_pixels += u64::from(surface.width) * u64::from(surface.height);
        ensure!(
            imported_pixels <= 64 * 1024 * 1024,
            "Sheet import exceeds 64 million destination pixels"
        );
        ensure!(
            options.fit || (surface.width, surface.height) == (cell_width, cell_height),
            "Sheet cell must match the declared destination canvas"
        );
        ensure!(
            cell.column < u32::from(w.div_ceil(cell_width))
                && cell.row < u32::from(h.div_ceil(cell_height)),
            "Sheet cell outside image"
        );
        // Partial edge cells are transparently padded; no source pixel is cropped.
        let mut pixels = vec![0; usize::from(cell_width) * usize::from(cell_height) * 4];
        for y in 0..usize::from(cell_height) {
            let sy = cell.row as usize * usize::from(cell_height) + y;
            let sx = cell.column as usize * usize::from(cell_width);
            if sy >= usize::from(h) {
                continue;
            }
            let count = usize::from(cell_width).min(usize::from(w) - sx) * 4;
            let start = (sy * usize::from(w) + sx) * 4;
            let target = y * usize::from(cell_width) * 4;
            pixels[target..target + count].copy_from_slice(&rgba[start..start + count]);
        }
        pixels = prepared(
            pixels,
            cell_width,
            cell_height,
            surface.width,
            surface.height,
            options,
        )?;
        ensure!(
            manifest.library_version > 0
                || pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| pixel[3] == 0 || pixel[3] == 255),
            "DXT1 projects require binary alpha"
        );
        frames.push((surface, pixels));
    }
    validate(project)?;
    clone_project(project, output, &manifest)?;
    for (surface, rgba) in frames {
        std::fs::remove_file(output.join(&surface.path))?;
        write_png(
            &output.join(&surface.path),
            surface.width,
            surface.height,
            &rgba,
        )?;
    }
    save_manifest(output, &manifest)?;
    validate(output)
}

#[derive(Clone, Copy, Debug)]
pub struct CanvasSheetOptions {
    pub options: ImportOptions,
    pub canvas: [u16; 2],
    pub offsets: [i16; 2],
    pub replace_layers: bool,
}
/// Declare body canvases (including absent frames), then import an explicitly mapped atlas.
pub fn import_sheet_canvas(
    project: &Path,
    input: &Path,
    cell_width: u16,
    cell_height: u16,
    mapping: &[SheetCell],
    output: &Path,
    request: CanvasSheetOptions,
) -> Result<Report> {
    validate(project)?;
    let original = load_manifest(project)?;
    let [width, height] = request.canvas;
    ensure!(
        (1..=4096).contains(&width) && (1..=4096).contains(&height),
        "Canvas dimensions must be 1–4096"
    );
    ensure!(
        !mapping.is_empty() && mapping.len() <= 4096,
        "Canvas atlas needs 1–4096 mappings"
    );
    ensure!(
        u64::from(width) * u64::from(height) * mapping.len() as u64 <= 64 * 1024 * 1024,
        "Canvas atlas exceeds 64 million pixels"
    );
    let mut seen = BTreeSet::new();
    for cell in mapping {
        ensure!(
            cell.layer == "image",
            "Canvas declaration only supports body image mappings"
        );
        ensure!(
            cell.slot < original.slots.len() && seen.insert(cell.slot),
            "Invalid or duplicate canvas slot"
        );
    }
    let parent = output.parent().context("Output has no parent")?;
    let temporary = parent.join(format!(
        ".sprite-canvas-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    clone_project(project, &temporary, &original)?;
    let result = (|| {
        let mut manifest = original;
        let blank = vec![0; usize::from(width) * usize::from(height) * 4];
        for cell in mapping {
            let slot = &mut manifest.slots[cell.slot];
            if request.replace_layers {
                slot.shadow = None;
                slot.overlay = None;
                slot.shadow_offset_x = 0;
                slot.shadow_offset_y = 0;
                slot.shadow_type = 0;
            }
            slot.present = true;
            slot.offset_x = request.offsets[0];
            slot.offset_y = request.offsets[1];
            let path = format!("frames/{:06}-image.png", cell.slot);
            let destination = temporary.join(&path);
            if destination.exists() {
                std::fs::remove_file(&destination)?;
            }
            write_png(&destination, width, height, &blank)?;
            slot.image = Some(Surface {
                path,
                width,
                height,
            });
        }
        save_manifest(&temporary, &manifest)?;
        import_sheet_options(
            &temporary,
            input,
            cell_width,
            cell_height,
            mapping,
            output,
            request.options,
        )
    })();
    let _ = std::fs::remove_dir_all(&temporary);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new(version: u8) -> Self {
            let path = std::env::temp_dir().join(format!(
                "zircon-sprite-project-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            let mut builder = ZlBuilder::new(version);
            builder.push_raw(None);
            let mut payload = builder.encode_surface(&[220, 100, 40, 255].repeat(16), 4, 4);
            payload.extend(builder.encode_surface(&[16, 16, 16, 255].repeat(16), 4, 4));
            payload.extend(builder.encode_surface(&[40, 100, 220, 255].repeat(16), 4, 4));
            builder.push_raw(Some(RawImage {
                info: ImageInfo {
                    width: 4,
                    height: 4,
                    offset_x: -3,
                    offset_y: 7,
                    shadow_width: 4,
                    shadow_height: 4,
                    shadow_offset_x: -2,
                    shadow_offset_y: 4,
                    shadow_type: 177,
                    overlay_width: 4,
                    overlay_height: 4,
                    ..ImageInfo::default()
                },
                payload,
            }));
            builder.push_raw(Some(RawImage {
                info: ImageInfo {
                    width: 12,
                    height: 8,
                    offset_x: -8,
                    ..ImageInfo::default()
                },
                payload: vec![],
            }));
            builder.push_raw(None);
            write_new(&path.join("original.zl"), &builder.to_bytes()).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn batch_tint_preserves_alpha_anchors_and_other_layers() {
        let fixture = Fixture::new(2);
        let project = fixture.0.join("project");
        export(&fixture.0.join("original.zl"), &project).unwrap();
        let before = load_manifest(&project).unwrap();
        let output = fixture.0.join("tinted");
        assert_eq!(
            tint(&project, &output, "image", [0, 255, 255])
                .unwrap()
                .changed_slots,
            1
        );
        let after = load_manifest(&output).unwrap();
        assert_eq!(
            serde_json::to_value(before.clone()).unwrap(),
            serde_json::to_value(after).unwrap()
        );
        for layer in ["shadow", "overlay"] {
            let path = &before.slots[1].surface(layer).unwrap().unwrap().path;
            assert_eq!(
                std::fs::read(project.join(path)).unwrap(),
                std::fs::read(output.join(path)).unwrap()
            );
        }
        let path = &before.slots[1].image.as_ref().unwrap().path;
        let (_, _, old) = read_png(&project.join(path)).unwrap();
        let (_, _, new) = read_png(&output.join(path)).unwrap();
        for (old, new) in old.as_chunks::<4>().0.iter().zip(new.as_chunks::<4>().0) {
            assert_eq!(new[0], 0);
            assert_eq!(&old[1..], &new[1..]);
        }
        build(&output, &fixture.0.join("built")).unwrap();
        let neutral = fixture.0.join("neutral");
        assert_eq!(
            tint(&project, &neutral, "image", [255; 3])
                .unwrap()
                .changed_slots,
            0
        );
        assert!(tint(&project, &neutral, "image", [0; 3]).is_err());
        assert!(tint(&project, &fixture.0.join("invalid"), "shadow", [0; 3]).is_err());
    }

    #[test]
    fn paint_extends_silhouette_without_moving_body_or_other_layers() {
        let fixture = Fixture::new(1);
        let project = fixture.0.join("project");
        export(&fixture.0.join("original.zl"), &project).unwrap();
        let before = load_manifest(&project).unwrap();
        let output = fixture.0.join("painted");
        let polygons = [
            PaintPolygon {
                slot: 1,
                points: vec![[-2, -1], [6, -1], [6, 5], [-2, 5]],
                color: [200, 80, 40, 255],
                under: true,
            },
            PaintPolygon {
                slot: 1,
                points: vec![[0, 0], [2, 0], [2, 2], [0, 2]],
                color: [240, 140, 20, 255],
                under: false,
            },
        ];
        assert_eq!(
            paint(&project, &output, [2, 1, 2, 1], &polygons)
                .unwrap()
                .changed_slots,
            1
        );
        let after = load_manifest(&output).unwrap();
        let slot = &after.slots[1];
        assert_eq!((slot.offset_x, slot.offset_y), (-5, 6));
        assert_eq!(
            (
                slot.image.as_ref().unwrap().width,
                slot.image.as_ref().unwrap().height
            ),
            (8, 6)
        );
        let (_, _, pixels) = read_png(&output.join(&slot.image.as_ref().unwrap().path)).unwrap();
        let pixel = |x: usize, y: usize| &pixels[(y * 8 + x) * 4..(y * 8 + x + 1) * 4];
        assert_eq!(pixel(0, 0), [200, 80, 40, 255]);
        assert_eq!(pixel(2, 1), [240, 140, 20, 255]);
        let (_, _, original) =
            read_png(&project.join(&before.slots[1].image.as_ref().unwrap().path)).unwrap();
        assert_eq!(pixel(4, 3), &original[(2 * 4 + 2) * 4..(2 * 4 + 3) * 4]);
        for layer in ["shadow", "overlay"] {
            let path = &before.slots[1].surface(layer).unwrap().unwrap().path;
            assert_eq!(
                std::fs::read(project.join(path)).unwrap(),
                std::fs::read(output.join(path)).unwrap()
            );
        }
        build(&output, &fixture.0.join("built-paint")).unwrap();
        assert!(paint(&project, &output, [0; 4], &polygons).is_err());
        assert!(output.exists());
        let invalid = fixture.0.join("invalid-paint");
        assert!(paint(&project, &invalid, [0; 4], &[]).is_err());
        assert!(!invalid.exists());
    }

    #[test]
    fn motion_keyframes_interpolate_only_one_contract_animation() {
        let fixture = Fixture::new(1);
        let source = fixture.0.join("motion-source.zl");
        let mut builder = ZlBuilder::new(1);
        for _ in 0..5 {
            let mut image = vec![0; 6 * 8 * 4];
            image[(3 * 6 + 3) * 4..(3 * 6 + 4) * 4].copy_from_slice(&[30, 40, 50, 255]);
            let mut payload = builder.encode_surface(&image, 6, 8);
            payload.extend(builder.encode_surface(&[10, 10, 10, 255].repeat(6 * 8), 6, 8));
            payload.extend(builder.encode_surface(&[20, 20, 20, 255].repeat(6 * 8), 6, 8));
            builder.push_raw(Some(RawImage {
                info: ImageInfo {
                    width: 6,
                    height: 8,
                    offset_x: -3,
                    offset_y: -7,
                    shadow_width: 6,
                    shadow_height: 8,
                    overlay_width: 6,
                    overlay_height: 8,
                    ..ImageInfo::default()
                },
                payload,
            }));
        }
        write_new(&source, &builder.to_bytes()).unwrap();
        let project = fixture.0.join("motion-project");
        export(&source, &project).unwrap();
        let original = std::fs::read(&source).unwrap();
        let before = load_manifest(&project).unwrap();
        let contract = crate::sprite_contract::Contract {
            version: 1,
            kind: "armour-male".into(),
            appearance: 0,
            library: 31,
            library_path: String::new(),
            source_checksum: before.source_checksum.clone(),
            source_start: before.source_start,
            source_slots: before.source_slots,
            directions: vec!["Down".into()],
            frames: (0..5)
                .map(|frame| crate::sprite_contract::RequiredFrame {
                    action: "Walking".into(),
                    direction: 0,
                    frame,
                    index: frame as usize,
                    interval_ms: 100,
                })
                .collect(),
            slots: vec![],
            missing_required: vec![],
        };
        std::fs::write(
            project.join("contract.json"),
            serde_json::to_vec(&contract).unwrap(),
        )
        .unwrap();
        let keyframes = [
            MotionKeyframe {
                slot: 0,
                points: vec![[-1, 2], [1, 2], [1, 7], [-1, 7]],
                color: [200, 0, 0, 255],
                under: true,
            },
            MotionKeyframe {
                slot: 2,
                points: vec![[3, 2], [5, 2], [5, 7], [3, 7]],
                color: [0, 0, 200, 255],
                under: true,
            },
            MotionKeyframe {
                slot: 4,
                points: vec![[-1, 2], [1, 2], [1, 7], [-1, 7]],
                color: [200, 0, 0, 255],
                under: true,
            },
        ];
        let output = fixture.0.join("motion-painted");
        assert_eq!(
            paint_motion(
                &project,
                &output,
                [2, 0, 2, 2],
                &[0, 1, 2, 3, 4],
                &keyframes
            )
            .unwrap()
            .changed_slots,
            5
        );
        let after = load_manifest(&output).unwrap();
        assert_eq!((after.slots[1].offset_x, after.slots[1].offset_y), (-5, -7));
        let (_, _, frame) = read_png(&output.join("frames/000001-image.png")).unwrap();
        let at = |x: usize, y: usize| &frame[(y * 10 + x) * 4..(y * 10 + x + 1) * 4];
        assert_eq!(at(3, 3), [100, 0, 100, 255]);
        let (_, _, source_frame) = read_png(&project.join("frames/000001-image.png")).unwrap();
        assert_eq!(at(5, 3), &source_frame[(3 * 6 + 3) * 4..(3 * 6 + 4) * 4]);
        for slot in 0..5 {
            for layer in ["shadow", "overlay"] {
                let path = &before.slots[slot].surface(layer).unwrap().unwrap().path;
                assert_eq!(
                    std::fs::read(project.join(path)).unwrap(),
                    std::fs::read(output.join(path)).unwrap()
                );
            }
        }
        build(&output, &fixture.0.join("motion-built")).unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), original);
        let toggles = [
            MotionKeyframe {
                slot: 0,
                points: vec![[2, 2], [5, 2], [5, 7], [2, 7]],
                color: [0, 200, 0, 255],
                under: true,
            },
            MotionKeyframe {
                slot: 4,
                points: vec![[2, 2], [5, 2], [5, 7], [2, 7]],
                color: [0, 200, 0, 255],
                under: false,
            },
        ];
        let toggled = fixture.0.join("motion-under-switch");
        paint_motion(&project, &toggled, [2, 0, 2, 2], &[0, 1, 2, 3, 4], &toggles).unwrap();
        let (_, _, early) = read_png(&toggled.join("frames/000001-image.png")).unwrap();
        let (_, _, late) = read_png(&toggled.join("frames/000002-image.png")).unwrap();
        assert_eq!(
            &early[(3 * 10 + 5) * 4..(3 * 10 + 6) * 4],
            &source_frame[(3 * 6 + 3) * 4..(3 * 6 + 4) * 4]
        );
        assert_eq!(&late[(3 * 10 + 5) * 4..(3 * 10 + 6) * 4], &[0, 200, 0, 255]);
        let auto = fixture.0.join("motion-designed");
        let designed = paint_auto_motion(
            &project,
            &auto,
            [2, 0, 2, 2],
            &[0, 1, 2, 3, 4],
            &keyframes[0],
            3,
        )
        .unwrap();
        assert_eq!(designed.changed_slots, 5);
        let (_, _, forward) = read_png(&auto.join("frames/000001-image.png")).unwrap();
        let (_, _, backward) = read_png(&auto.join("frames/000003-image.png")).unwrap();
        assert_ne!(forward, backward, "walking pose must sway across frames");
        assert_eq!(
            load_manifest(&auto).unwrap().slots[0].offset_x,
            after.slots[0].offset_x
        );
        build(&auto, &fixture.0.join("motion-designed-built")).unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), original);
        assert!(paint_auto_motion(
            &project,
            &fixture.0.join("invalid-auto-strength"),
            [0; 4],
            &[0, 1, 2, 3, 4],
            &keyframes[0],
            0,
        )
        .is_err());
        let mut outside = keyframes[0].clone();
        outside.slot = 5;
        let rejected = fixture.0.join("invalid-auto-seed");
        assert!(
            paint_auto_motion(&project, &rejected, [0; 4], &[0, 1, 2, 3, 4], &outside, 3,).is_err()
        );
        assert!(!rejected.exists());
        if let Some(directory) = std::env::var_os("ZIRCON_MOTION_QA_DIR") {
            let directory = PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            let editor_project = directory.join("editor-project");
            let editor_manifest = load_manifest(&auto).unwrap();
            clone_project(&auto, &editor_project, &editor_manifest).unwrap();
            save_manifest(&editor_project, &editor_manifest).unwrap();
            let (width, height, scale) = (10usize, 10usize, 8usize);
            let strip_width = width * 5 * scale;
            for (source, name) in [
                (&output, "motion-strip.png"),
                (&auto, "auto-motion-strip.png"),
            ] {
                let mut strip = [45, 45, 45, 255].repeat(strip_width * height * scale);
                for slot in 0..5 {
                    let (_, _, pixels) =
                        read_png(&source.join(format!("frames/{slot:06}-image.png"))).unwrap();
                    for y in 0..height {
                        for x in 0..width {
                            let pixel = &pixels[(y * width + x) * 4..(y * width + x + 1) * 4];
                            if pixel[3] == 0 {
                                continue;
                            }
                            for dy in 0..scale {
                                for dx in 0..scale {
                                    let at = ((y * scale + dy) * strip_width
                                        + slot * width * scale
                                        + x * scale
                                        + dx)
                                        * 4;
                                    strip[at..at + 4].copy_from_slice(pixel);
                                }
                            }
                        }
                    }
                }
                write_png(
                    &directory.join(name),
                    strip_width as u16,
                    (height * scale) as u16,
                    &strip,
                )
                .unwrap();
            }
        }
        let mut attack = contract;
        for frame in &mut attack.frames {
            frame.action = "Attack".into();
        }
        std::fs::write(
            project.join("contract.json"),
            serde_json::to_vec(&attack).unwrap(),
        )
        .unwrap();
        let attack_output = fixture.0.join("motion-designed-attack");
        paint_auto_motion(
            &project,
            &attack_output,
            [2, 0, 2, 2],
            &[0, 1, 2, 3, 4],
            &keyframes[0],
            3,
        )
        .unwrap();
        let (_, _, lunge) = read_png(&attack_output.join("frames/000003-image.png")).unwrap();
        assert_ne!(lunge, backward, "attack must differ from walking sway");
        assert!(paint_motion(
            &project,
            &fixture.0.join("bad-motion"),
            [0; 4],
            &[0, 2, 4],
            &keyframes
        )
        .is_err());
    }

    #[test]
    fn visible_art_fit_uses_the_canvas_instead_of_transparent_margins() {
        let fixture = Fixture::new(1);
        let project = fixture.0.join("project");
        export(&fixture.0.join("original.zl"), &project).unwrap();
        let input = fixture.0.join("padded.png");
        let mut rgba = vec![0; 12 * 12 * 4];
        for y in 4..8 {
            for x in 5..7 {
                rgba[(y * 12 + x) * 4..(y * 12 + x + 1) * 4].copy_from_slice(&[40, 200, 80, 255]);
            }
        }
        write_png(&input, 12, 12, &rgba).unwrap();
        let original = std::fs::read(&input).unwrap();
        let output = fixture.0.join("visible");
        import_image_options(
            &project,
            1,
            "image",
            &input,
            &output,
            ImportOptions {
                fit: true,
                binary_alpha: false,
                trim_transparent: true,
            },
        )
        .unwrap();
        let (w, h, pixels) = read_png(&output.join("frames/000001-image.png")).unwrap();
        assert_eq!((w, h), (4, 4));
        assert_eq!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] > 0)
                .count(),
            8
        );
        assert_eq!(std::fs::read(&input).unwrap(), original);
        let blank = fixture.0.join("blank.png");
        write_png(&blank, 12, 12, &vec![0; 12 * 12 * 4]).unwrap();
        let refused = fixture.0.join("refused");
        assert!(import_image_options(
            &project,
            1,
            "image",
            &blank,
            &refused,
            ImportOptions {
                fit: true,
                binary_alpha: false,
                trim_transparent: true
            }
        )
        .is_err());
        assert!(!refused.exists());
    }
    #[test]
    fn partial_sheet_edges_preserve_pixels_and_pad_transparently() {
        let fixture = Fixture::new(1);
        let project = fixture.0.join("project");
        export(&fixture.0.join("original.zl"), &project).unwrap();
        let input = fixture.0.join("partial.png");
        write_png(&input, 3, 3, &[10, 20, 30, 255].repeat(9)).unwrap();
        let output = fixture.0.join("padded");
        import_sheet(
            &project,
            &input,
            4,
            4,
            &[SheetCell {
                slot: 1,
                layer: "image".into(),
                column: 0,
                row: 0,
            }],
            &output,
        )
        .unwrap();
        let (_, _, pixels) = read_png(&output.join("frames/000001-image.png")).unwrap();
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(
                    &pixels[(y * 4 + x) * 4..(y * 4 + x + 1) * 4],
                    if x < 3 && y < 3 {
                        &[10, 20, 30, 255]
                    } else {
                        &[0, 0, 0, 0]
                    }
                );
            }
        }
    }
    #[test]
    fn untouched_project_preserves_all_raw_slots_surfaces_offsets_and_explicit_blanks() {
        let fixture = Fixture::new(1);
        let source = fixture.0.join("original.zl");
        let before = std::fs::read(&source).unwrap();
        let project = fixture.0.join("project");
        let exported = export(&source, &project).unwrap();
        assert_eq!(exported.slots, 4);
        assert_eq!(exported.present, 2);
        assert_eq!(exported.surfaces, 3);
        let manifest = load_manifest(&project).unwrap();
        assert!(!manifest.slots[0].present);
        assert!(manifest.slots[2].present);
        assert!(manifest.slots[2].image.is_none());
        assert_eq!(manifest.slots[1].offset_x, -3);
        assert_eq!(manifest.slots[1].shadow_type, 177);
        let built = build(&project, &fixture.0.join("built")).unwrap();
        assert_eq!(built.changed_slots, 0);
        assert_eq!(std::fs::read(built.library.unwrap()).unwrap(), before);
        assert_eq!(std::fs::read(&source).unwrap(), before);
        assert!(export(&source, &project).is_err());
        assert!(build(&project, &fixture.0.join("built")).is_err());
    }
    #[test]
    fn explicit_fit_and_sheet_imports_create_new_revisions_and_rebuild_changed_slots() {
        let fixture = Fixture::new(1);
        let project = fixture.0.join("project");
        export(&fixture.0.join("original.zl"), &project).unwrap();
        let before = file_checksum(&project.join("frames/000001-image.png")).unwrap();
        let input = fixture.0.join("new.png");
        write_png(&input, 8, 8, &[20, 220, 80, 255].repeat(64)).unwrap();
        assert!(import_image(&project, 1, "image", &input, &fixture.0.join("refused")).is_err());
        assert!(!fixture.0.join("refused").exists());
        let edited = fixture.0.join("edited");
        import_image_with_fit(&project, 1, "image", &input, &edited, true).unwrap();
        assert_eq!(validate(&edited).unwrap().changed_slots, 1);
        assert_eq!(
            file_checksum(&project.join("frames/000001-image.png")).unwrap(),
            before
        );
        let result = build(&edited, &fixture.0.join("built")).unwrap();
        assert_eq!(result.changed_slots, 1);
        let built = open_library(&result.library.unwrap()).unwrap();
        assert_eq!(built.info(1).unwrap().offset_x, -3);
        assert_eq!(built.info(1).unwrap().shadow_type, 177);
        let body = pixels(&built, 1, SurfaceKind::Image).unwrap().unwrap();
        assert!(body[1] > 200);
        assert!(body[0] < 40);
        let original = open_library(&project.join("source.zl")).unwrap();
        assert_eq!(
            built.raw(2).unwrap().unwrap().payload,
            original.raw(2).unwrap().unwrap().payload
        );
        assert_eq!(
            pixels(&built, 1, SurfaceKind::Shadow).unwrap(),
            pixels(&original, 1, SurfaceKind::Shadow).unwrap()
        );
        assert_eq!(
            raw_surface(&built.raw(1).unwrap().unwrap(), SurfaceKind::Shadow, 1).unwrap(),
            raw_surface(&original.raw(1).unwrap().unwrap(), SurfaceKind::Shadow, 1).unwrap()
        );
        let sheet = fixture.0.join("sheet.png");
        let mut sheet_pixels = vec![];
        for _ in 0..4 {
            sheet_pixels.extend([255, 0, 0, 255].repeat(4));
            sheet_pixels.extend([0, 0, 255, 255].repeat(4));
        }
        write_png(&sheet, 8, 4, &sheet_pixels).unwrap();
        let mapping = vec![SheetCell {
            slot: 1,
            layer: "image".into(),
            column: 1,
            row: 0,
        }];
        let imported = fixture.0.join("sheet-project");
        import_sheet(&project, &sheet, 4, 4, &mapping, &imported).unwrap();
        assert_eq!(
            &read_png(&imported.join("frames/000001-image.png"))
                .unwrap()
                .2[..4],
            &[0, 0, 255, 255]
        );
        assert!(import_sheet(
            &project,
            &sheet,
            4,
            4,
            &[mapping[0].clone(), mapping[0].clone()],
            &fixture.0.join("duplicate")
        )
        .is_err());
        assert!(!fixture.0.join("duplicate").exists());
        let large_sheet = fixture.0.join("large-sheet.png");
        write_png(&large_sheet, 16, 8, &[0, 255, 0, 128].repeat(128)).unwrap();
        let strict = fixture.0.join("strict-sheet");
        assert!(import_sheet(&project, &large_sheet, 8, 8, &mapping, &strict).is_err());
        assert!(!strict.exists());
        let resized = fixture.0.join("resized-sheet");
        import_sheet_options(
            &project,
            &large_sheet,
            8,
            8,
            &mapping,
            &resized,
            ImportOptions {
                fit: true,
                binary_alpha: true,
                trim_transparent: false,
            },
        )
        .unwrap();
        let (width, height, pixels) = read_png(&resized.join("frames/000001-image.png")).unwrap();
        assert_eq!((width, height), (4, 4));
        assert!(pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p == &[0, 255, 0, 255]));
        assert_eq!(
            load_manifest(&resized).unwrap().slots[1].offset_x,
            load_manifest(&project).unwrap().slots[1].offset_x
        );
    }
    #[test]
    fn canvas_atlas_populates_absent_slots_clears_auxiliary_layers_and_rejects_duplicates() {
        let fixture = Fixture::new(0);
        let project = fixture.0.join("project");
        export(&fixture.0.join("original.zl"), &project).unwrap();
        let sheet = fixture.0.join("atlas.png");
        write_png(&sheet, 8, 8, &[30, 220, 50, 128].repeat(64)).unwrap();
        let mapping = vec![
            SheetCell {
                slot: 0,
                layer: "image".into(),
                column: 0,
                row: 0,
            },
            SheetCell {
                slot: 1,
                layer: "image".into(),
                column: 0,
                row: 0,
            },
        ];
        let request = CanvasSheetOptions {
            options: ImportOptions {
                fit: true,
                binary_alpha: true,
                trim_transparent: false,
            },
            canvas: [6, 6],
            offsets: [-3, -5],
            replace_layers: true,
        };
        let output = fixture.0.join("atlas-project");
        import_sheet_canvas(&project, &sheet, 8, 8, &mapping, &output, request).unwrap();
        let manifest = load_manifest(&output).unwrap();
        for slot in &manifest.slots[..2] {
            assert!(slot.present && slot.shadow.is_none() && slot.overlay.is_none());
            assert_eq!((slot.offset_x, slot.offset_y), (-3, -5));
            assert_eq!(slot.image.as_ref().unwrap().width, 6);
        }
        let built = build(&output, &fixture.0.join("atlas-build")).unwrap();
        let library = ZlLibrary::open(built.library.unwrap()).unwrap();
        assert!(library.decode(0, SurfaceKind::Image).unwrap().is_some());
        let original = ZlLibrary::open(project.join("source.zl")).unwrap();
        assert_eq!(
            library.raw(2).unwrap().unwrap().payload,
            original.raw(2).unwrap().unwrap().payload
        );
        assert!(!load_manifest(&project).unwrap().slots[0].present);
        let rejected = fixture.0.join("rejected");
        assert!(import_sheet_canvas(
            &project,
            &sheet,
            8,
            8,
            &[mapping[0].clone(), mapping[0].clone()],
            &rejected,
            request
        )
        .is_err());
        assert!(!rejected.exists());
        assert!(import_sheet_canvas(
            &project,
            &sheet,
            8,
            8,
            &mapping,
            &rejected,
            CanvasSheetOptions {
                canvas: [0, 6],
                ..request
            }
        )
        .is_err());
        assert!(!rejected.exists());
    }

    #[test]
    fn invalid_paths_indices_source_checksums_and_alpha_are_rejected() {
        let fixture = Fixture::new(0);
        let project = fixture.0.join("project");
        export(&fixture.0.join("original.zl"), &project).unwrap();
        let pristine = std::fs::read(project.join("project.json")).unwrap();
        let mut manifest = load_manifest(&project).unwrap();
        manifest.slots[1].image.as_mut().unwrap().path = "frames/../../outside.png".into();
        std::fs::write(
            project.join("project.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(validate(&project).is_err());
        std::fs::write(project.join("project.json"), &pristine).unwrap();
        let mut manifest = load_manifest(&project).unwrap();
        manifest.slots[2].index = 1;
        std::fs::write(
            project.join("project.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(validate(&project).is_err());
        std::fs::write(project.join("project.json"), &pristine).unwrap();
        let input = fixture.0.join("alpha.png");
        let mut translucent = [20, 40, 60, 128].repeat(16);
        translucent[3] = 127;
        write_png(&input, 4, 4, &translucent).unwrap();
        assert!(import_image(
            &project,
            1,
            "image",
            &input,
            &fixture.0.join("alpha-project")
        )
        .is_err());
        assert!(!fixture.0.join("alpha-project").exists());
        let converted = fixture.0.join("alpha-binary");
        import_image_options(
            &project,
            1,
            "image",
            &input,
            &converted,
            ImportOptions {
                fit: false,
                binary_alpha: true,
                trim_transparent: false,
            },
        )
        .unwrap();
        let converted_manifest = load_manifest(&converted).unwrap();
        let (_, _, converted_pixels) =
            read_png(&converted.join(&converted_manifest.slots[1].image.as_ref().unwrap().path))
                .unwrap();
        assert_eq!(converted_pixels[3], 0);
        assert!(converted_pixels
            .as_chunks::<4>()
            .0
            .iter()
            .skip(1)
            .all(|pixel| pixel[3] == 255));
        assert_eq!(read_png(&input).unwrap().2, translucent);

        std::fs::write(project.join("source.zl"), b"tampered").unwrap();
        assert!(validate(&project).is_err());
        let malformed = fixture.0.join("oversized.zl");
        std::fs::write(&malformed, [255; 8]).unwrap();
        assert!(export(&malformed, &fixture.0.join("bad-export")).is_err());
        assert!(!fixture.0.join("bad-export").exists());
    }
    #[cfg(unix)]
    #[test]
    fn symlinked_pngs_cannot_escape_the_project() {
        let fixture = Fixture::new(1);
        let project = fixture.0.join("project");
        export(&fixture.0.join("original.zl"), &project).unwrap();
        let path = project.join("frames/000001-image.png");
        let outside = fixture.0.join("outside.png");
        std::fs::copy(&path, &outside).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        assert!(validate(&project).is_err());
    }
    #[test]
    fn immutable_import_revisions_preserve_identity_contract_and_reference_files() {
        let fixture = Fixture::new(1);
        let project = fixture.0.join("project");
        export(&fixture.0.join("original.zl"), &project).unwrap();
        for name in [
            "identity.json",
            "contract.json",
            "reference.json",
            "animation.json",
        ] {
            write_new(&project.join(name), b"{\"fixture\":true}").unwrap();
        }
        write_png(
            &project.join("reference.png"),
            4,
            4,
            &[20, 30, 40, 255].repeat(16),
        )
        .unwrap();
        let input = fixture.0.join("edit.png");
        write_png(&input, 4, 4, &[0, 255, 0, 255].repeat(16)).unwrap();
        let output = fixture.0.join("import");
        import_image(&project, 1, "image", &input, &output).unwrap();
        for name in [
            "identity.json",
            "contract.json",
            "reference.json",
            "animation.json",
            "reference.png",
        ] {
            assert_eq!(
                std::fs::read(project.join(name)).unwrap(),
                std::fs::read(output.join(name)).unwrap()
            );
        }
        #[cfg(unix)]
        {
            let external = fixture.0.join("external.json");
            std::fs::write(&external, b"{}").unwrap();
            std::fs::remove_file(project.join("identity.json")).unwrap();
            std::os::unix::fs::symlink(&external, project.join("identity.json")).unwrap();
            let refused = fixture.0.join("refused");
            assert!(import_image(&project, 1, "image", &input, &refused).is_err());
            assert!(!refused.exists());
        }
    }
    #[test]
    fn asset_monster_shape_exports_and_builds_without_changing_raw_frames_or_source() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        let fixture = Fixture::new(1);
        let (lib, shape) = mir_formats::monster_table::monster_sprite(33).unwrap();
        let relative = mir_formats::monster_table::library_path(lib).unwrap();
        let path = crate::preview::resolve_case_insensitive(&assets, relative).unwrap();
        let before = file_checksum(&path).unwrap();
        let project = fixture.0.join("oma");
        export_range(&path, usize::from(shape) * 1000, 1000, &project).unwrap();
        let manifest = load_manifest(&project).unwrap();
        assert_eq!(manifest.source_start, usize::from(shape) * 1000);
        assert_eq!(manifest.source_checksum, before);
        assert_eq!(manifest.slots.len(), 1000);
        validate(&project).unwrap();
        let result = build(&project, &fixture.0.join("oma-built")).unwrap();
        assert_eq!(result.changed_slots, 0);
        let built = open_library(&result.library.unwrap()).unwrap();
        let original = open_library(&path).unwrap();
        for index in 0..1000 {
            assert_eq!(
                built.raw(index).unwrap().map(|raw| raw.payload),
                original
                    .raw(manifest.source_start + index)
                    .unwrap()
                    .map(|raw| raw.payload)
            );
        }
        assert_eq!(file_checksum(&path).unwrap(), before);
    }
}
