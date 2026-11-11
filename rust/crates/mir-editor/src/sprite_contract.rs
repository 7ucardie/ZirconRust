//! Runtime animation slots and source-art references for editable sprite projects.
use crate::{preview::resolve_case_insensitive, sprite_project};
use anyhow::{ensure, Context, Result};
use mir_formats::{
    monster_animation,
    monster_table::{library_path, monster_sprite},
    zl::{ImageInfo, SurfaceKind, ZlLibrary},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredFrame {
    pub action: String,
    pub direction: u8,
    pub frame: u32,
    pub index: usize,
    pub interval_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotInfo {
    pub index: usize,
    pub present: bool,
    pub image: [u16; 2],
    pub offset: [i16; 2],
    /// Object-origin position inside the image; offsets remain authoritative.
    pub origin_anchor: [i32; 2],
    pub shadow: [u16; 2],
    pub shadow_offset: [i16; 2],
    pub shadow_type: u8,
    pub overlay: [u16; 2],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub version: u16,
    pub kind: String,
    pub appearance: u32,
    pub library: u16,
    pub library_path: String,
    pub source_checksum: String,
    pub source_start: usize,
    pub source_slots: usize,
    pub directions: Vec<String>,
    pub frames: Vec<RequiredFrame>,
    pub slots: Vec<SlotInfo>,
    pub missing_required: Vec<usize>,
}
fn source_path(assets: &Path, library: u16) -> Result<PathBuf> {
    let path = library_path(library).context("Unknown source sprite library.")?;
    resolve_case_insensitive(assets, path)
        .or_else(|| {
            (library == 14)
                .then(|| resolve_case_insensitive(assets, "Data/StoreItem.Zl"))
                .flatten()
        })
        .context("Source sprite library is not installed.")
}
fn slot_info(index: usize, info: Option<&ImageInfo>) -> SlotInfo {
    let i = info.copied().unwrap_or_default();
    SlotInfo {
        index,
        present: info.is_some(),
        image: [i.width, i.height],
        offset: [i.offset_x, i.offset_y],
        origin_anchor: [-i32::from(i.offset_x), -i32::from(i.offset_y)],
        shadow: [i.shadow_width, i.shadow_height],
        shadow_offset: [i.shadow_offset_x, i.shadow_offset_y],
        shadow_type: i.shadow_type,
        overlay: [i.overlay_width, i.overlay_height],
    }
}
fn inventory(mut contract: Contract, assets: &Path) -> Result<Contract> {
    let path = source_path(assets, contract.library)?;
    let source = ZlLibrary::open(&path)?;
    ensure!(
        contract.source_start < source.len(),
        "Appearance starts beyond its source library."
    );
    contract.source_checksum = sprite_project::file_checksum(&path)?;
    contract.slots = (contract.source_start..contract.source_start + contract.source_slots)
        .map(|index| slot_info(index, source.info(index)))
        .collect();
    contract.missing_required = contract
        .frames
        .iter()
        .filter(|frame| {
            source
                .info(frame.index)
                .is_none_or(|info| info.width == 0 || info.height == 0)
        })
        .map(|f| f.index)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(contract)
}
pub(crate) fn monster_directions(image: u16, action: monster_animation::Action) -> &'static [u8] {
    if !monster_animation::is_gate(image) {
        return &[0, 1, 2, 3, 4, 5, 6, 7];
    }
    if action == monster_animation::Action::Struck {
        &[0, 1, 2]
    } else {
        &[0, 1, 2, 7]
    }
}
pub fn monster(image: u16, assets: &Path) -> Result<Contract> {
    let (library, shape) =
        monster_sprite(image).context("Monster appearance has no runtime mapping.")?;
    let mut frames = vec![];
    for action in monster_animation::ACTIONS {
        let spec = monster_animation::frame_for_image(image, action);
        for &direction in monster_directions(image, action) {
            for frame in 0..spec.count {
                frames.push(RequiredFrame {
                    action: format!("{action:?}"),
                    direction,
                    frame,
                    index: monster_animation::slot_for_image(image, shape, action, direction, frame)
                        .unwrap() as usize,
                    interval_ms: spec.interval,
                });
            }
        }
    }
    inventory(
        Contract {
            version: 1,
            kind: "monster".into(),
            appearance: u32::from(image),
            library,
            library_path: library_path(library).unwrap().into(),
            source_checksum: String::new(),
            source_start: usize::from(shape % 10) * 1000,
            source_slots: 1000,
            directions: monster_directions(image, monster_animation::Action::Standing)
                .iter()
                .map(|&direction| {
                    [
                        "Up",
                        "UpRight",
                        "Right",
                        "DownRight",
                        "Down",
                        "DownLeft",
                        "Left",
                        "UpLeft",
                    ][usize::from(direction)]
                    .to_owned()
                })
                .collect(),
            frames,
            slots: vec![],
            missing_required: vec![],
        },
        assets,
    )
}
/// ItemInfo.Image selects the same index in StoreItems (inventory) and Ground.
/// Each library receives a separate editable project and contract.
pub fn item(image: u32, ground: bool, assets: &Path) -> Result<Contract> {
    ensure!(
        image <= 1_000_000,
        "Item image index exceeds the authoring limit."
    );
    let library = if ground { 16 } else { 14 };
    inventory(
        Contract {
            version: 1,
            kind: if ground { "item-ground" } else { "item-icon" }.into(),
            appearance: image,
            library,
            library_path: library_path(library).unwrap().into(),
            source_checksum: String::new(),
            source_start: image as usize,
            source_slots: 1,
            directions: vec!["Static".into()],
            frames: vec![RequiredFrame {
                action: "Static".into(),
                direction: 0,
                frame: 0,
                index: image as usize,
                interval_ms: 0,
            }],
            slots: vec![],
            missing_required: vec![],
        },
        assets,
    )
}

/// Body families use separate libraries; `appearance` is ItemInfo.Shape.
pub fn armour(kind: &str, shape: u16, assets: &Path) -> Result<Contract> {
    use mir_formats::wearable_animation as animation;
    let (female, assassin) = armour_family(kind).context("Unknown armour body family")?;
    let library = animation::armour_library(shape, female, assassin)
        .context("Armour shape has no runtime library for this body family")?;
    // The client chooses fishing support from the base male body library.
    let fishing = ZlLibrary::open(source_path(assets, 31)?)?
        .info(2000)
        .is_some();
    let mut frames = Vec::new();
    for mounted in [false, true] {
        for action in animation::ACTIONS {
            if mounted
                && !matches!(
                    action,
                    animation::Action::Standing
                        | animation::Action::Walking
                        | animation::Action::Running
                        | animation::Action::Struck
                )
            {
                continue;
            }
            let spec = animation::body_frame(action, mounted, fishing);
            for direction in 0..8 {
                for frame in 0..spec.count {
                    frames.push(RequiredFrame {
                        action: if mounted {
                            format!("Mounted{action:?}")
                        } else {
                            format!("{action:?}")
                        },
                        direction,
                        frame,
                        index: animation::slot(
                            shape, action, direction, frame, assassin, mounted, fishing,
                        )
                        .unwrap() as usize,
                        interval_ms: spec.interval,
                    });
                }
            }
        }
    }
    inventory(
        Contract {
            version: 1,
            kind: kind.into(),
            appearance: u32::from(shape),
            library,
            library_path: library_path(library).unwrap().into(),
            source_checksum: String::new(),
            source_start: usize::from(shape % 11) * animation::shape_stride(assassin) as usize,
            source_slots: animation::shape_stride(assassin) as usize,
            directions: [
                "Up",
                "UpRight",
                "Right",
                "DownRight",
                "Down",
                "DownLeft",
                "Left",
                "UpLeft",
            ]
            .map(str::to_owned)
            .to_vec(),
            frames,
            slots: vec![],
            missing_required: vec![],
        },
        assets,
    )
}

pub fn armour_family(kind: &str) -> Option<(bool, bool)> {
    match kind {
        "armour-male" => Some((false, false)),
        "armour-female" => Some((true, false)),
        "armour-assassin-male" => Some((false, true)),
        "armour-assassin-female" => Some((true, true)),
        _ => None,
    }
}

/// ItemType, gender and Assassin flag for equipped art families.
pub fn equipment_family(kind: &str) -> Option<(u8, bool, bool)> {
    match kind {
        "weapon-male" => Some((2, false, false)),
        "weapon-female" => Some((2, true, false)),
        "helmet-male" => Some((5, false, false)),
        "helmet-female" => Some((5, true, false)),
        "helmet-assassin-male" => Some((5, false, true)),
        "helmet-assassin-female" => Some((5, true, true)),
        "shield-male" => Some((27, false, false)),
        "shield-female" => Some((27, true, false)),
        "shield-assassin-male" => Some((27, false, true)),
        "shield-assassin-female" => Some((27, true, true)),
        _ => None,
    }
}

pub fn equipment(kind: &str, shape: u16, assets: &Path) -> Result<Contract> {
    use mir_formats::wearable_animation as animation;
    let (item_type, female, assassin) =
        equipment_family(kind).context("Unknown equipment family")?;
    let library = match item_type {
        2 => animation::weapon_library(shape, female),
        5 => animation::helmet_library(shape, female, assassin),
        _ => animation::shield_library(shape, female),
    }
    .context("Equipment shape has no runtime library for this family")?;
    let stride = if item_type == 2 {
        5000
    } else {
        animation::shape_stride(assassin) as usize
    };
    let local_shape = if item_type == 5 {
        shape
            .checked_sub(1)
            .context("Helmet shape must be positive")?
            % 10
    } else {
        shape % 10
    };
    let fishing = ZlLibrary::open(source_path(assets, 31)?)?
        .info(2000)
        .is_some();
    let mut frames = Vec::new();
    for mounted in [false, true] {
        for action in animation::ACTIONS {
            if mounted
                && !matches!(
                    action,
                    animation::Action::Standing
                        | animation::Action::Walking
                        | animation::Action::Running
                        | animation::Action::Struck
                )
            {
                continue;
            }
            let spec = animation::body_frame(action, mounted, fishing);
            let shift = if item_type == 2 {
                0
            } else {
                animation::armour_shift(action, assassin, mounted, fishing)
            };
            for direction in 0..8 {
                for frame in 0..spec.count {
                    frames.push(RequiredFrame {
                        action: if mounted {
                            format!("Mounted{action:?}")
                        } else {
                            format!("{action:?}")
                        },
                        direction,
                        frame,
                        index: (usize::from(local_shape) * stride
                            + spec.start as usize
                            + usize::from(direction) * 10
                            + frame as usize)
                            .saturating_add_signed(shift as isize),
                        interval_ms: spec.interval,
                    });
                }
            }
        }
    }
    let mut contract = inventory(
        Contract {
            version: 1,
            kind: kind.into(),
            appearance: u32::from(shape),
            library,
            library_path: library_path(library).unwrap().into(),
            source_checksum: String::new(),
            source_start: usize::from(local_shape) * stride,
            source_slots: stride,
            directions: [
                "Up",
                "UpRight",
                "Right",
                "DownRight",
                "Down",
                "DownLeft",
                "Left",
                "UpLeft",
            ]
            .map(str::to_owned)
            .to_vec(),
            frames,
            slots: vec![],
            missing_required: vec![],
        },
        assets,
    )?;
    // Equipment overlays omit actions where no equipment is drawn.
    contract.frames.retain(|frame| {
        contract
            .slots
            .get(frame.index - contract.source_start)
            .is_some_and(|slot| slot.present && slot.image[0] > 0 && slot.image[1] > 0)
    });
    ensure!(
        !contract.frames.is_empty(),
        "Equipment source has no visible animation frames"
    );
    contract.missing_required.clear();
    Ok(contract)
}

pub fn validate_project(contract: &Contract, project: &Path) -> Result<()> {
    let manifest = sprite_project::load_manifest(project)?;
    sprite_project::validate(project)?;
    ensure!(
        contract.version == 1
            && contract.source_checksum == manifest.source_checksum
            && contract.source_start == manifest.source_start
            && contract.source_slots == manifest.slots.len(),
        "Animation contract does not match the project's source identity or frame range."
    );
    let mut missing = BTreeSet::new();
    for frame in &contract.frames {
        let local = frame
            .index
            .checked_sub(manifest.source_start)
            .context("Required frame precedes project range.")?;
        let slot = manifest
            .slots
            .get(local)
            .context("Required frame exceeds project range.")?;
        if !slot.present || slot.image.is_none() {
            missing.insert(frame.index);
        }
    }
    ensure!(
        missing.is_empty(),
        "Missing required animation image slots: {missing:?}"
    );
    let required: BTreeSet<_> = contract.frames.iter().map(|frame| frame.index).collect();
    for index in required {
        let surface = manifest.slots[index - manifest.source_start]
            .image
            .as_ref()
            .unwrap();
        let (_, _, rgba) = sprite_project::read_png(&project.join(&surface.path))?;
        ensure!(
            rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0),
            "Required animation slot {index} is fully transparent."
        );
    }
    Ok(())
}

struct Pixels {
    width: u16,
    height: u16,
    rgba: Vec<u8>,
    offset: [i16; 2],
}
fn source_pixels(source: &ZlLibrary, index: usize, kind: SurfaceKind) -> Result<Option<Pixels>> {
    let Some(info) = source.info(index) else {
        return Ok(None);
    };
    let (width, height) = info.surface_dims(kind);
    ensure!(
        width <= 1024 && height <= 1024,
        "Contact-sheet surfaces must be at most 1024 pixels per side."
    );
    let Some(decoded) = source.decode(index, kind)? else {
        return Ok(None);
    };
    let mut rgba = vec![];
    for row in 0..u32::from(height) {
        let start = (row * decoded.width * 4) as usize;
        rgba.extend_from_slice(&decoded.rgba[start..start + usize::from(width) * 4]);
    }
    Ok(Some(Pixels {
        width,
        height,
        rgba,
        offset: if kind == SurfaceKind::Shadow {
            [info.shadow_offset_x, info.shadow_offset_y]
        } else {
            [info.offset_x, info.offset_y]
        },
    }))
}
fn render_sheet(
    contract: &Contract,
    output: &Path,
    mut pixels: impl FnMut(usize, SurfaceKind) -> Result<Option<Pixels>>,
) -> Result<()> {
    ensure!(
        !contract.frames.is_empty() && contract.frames.len() <= 2048,
        "Contact sheet needs1–2048 frames."
    );
    let mut images = Vec::new();
    let mut decoded_bytes = 0usize;
    let (mut left, mut top, mut right, mut bottom) = (0i32, 0i32, 48i32, 32i32);
    for frame in contract.frames.iter().filter(|frame| {
        (armour_family(&contract.kind).is_none() && equipment_family(&contract.kind).is_none())
            || frame.frame == 0
    }) {
        let mut layers = vec![];
        // Baked shadows and image surfaces are references; procedural shadow
        // projection and overlay tinting remain the client's responsibility.
        for kind in [SurfaceKind::Shadow, SurfaceKind::Image] {
            if let Some(p) = pixels(frame.index, kind)? {
                decoded_bytes += p.rgba.len();
                ensure!(
                    decoded_bytes <= 128_000_000,
                    "Contact-sheet decoded surfaces exceed 128 MB."
                );
                left = left.min(i32::from(p.offset[0]));
                top = top.min(i32::from(p.offset[1]));
                right = right.max(i32::from(p.offset[0]) + i32::from(p.width));
                bottom = bottom.max(i32::from(p.offset[1]) + i32::from(p.height));
                layers.push(p);
            }
        }
        images.push(layers);
    }
    let (cw, ch) = ((right - left + 16) as usize, (bottom - top + 16) as usize);
    ensure!(
        cw <= 1024 && ch <= 1024,
        "Contact-sheet frame bounds exceed1024 pixels; inspect offsets."
    );
    let columns = 8.min(images.len());
    let rows = images.len().div_ceil(columns);
    let (width, height) = (columns * cw, rows * ch);
    ensure!(
        width * height <= 32_000_000,
        "Contact sheet exceeds32 million pixels."
    );
    let mut rgba = vec![0u8; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let offset = (y * width + x) * 4;
            let shade = if (x / 8 + y / 8) % 2 == 0 { 35 } else { 48 };
            rgba[offset..offset + 4].copy_from_slice(&[shade, shade, shade, 255]);
        }
    }
    for (n, layers) in images.into_iter().enumerate() {
        let ox = (n % columns * cw) as i32 - left + 8;
        let oy = (n / columns * ch) as i32 - top + 8;
        for layer in layers {
            for y in 0..i32::from(layer.height) {
                for x in 0..i32::from(layer.width) {
                    let to = ((oy + i32::from(layer.offset[1]) + y) as usize * width
                        + (ox + i32::from(layer.offset[0]) + x) as usize)
                        * 4;
                    let from = (y as usize * usize::from(layer.width) + x as usize) * 4;
                    let a = u32::from(layer.rgba[from + 3]);
                    for c in 0..3 {
                        rgba[to + c] = ((u32::from(layer.rgba[from + c]) * a
                            + u32::from(rgba[to + c]) * (255 - a)
                            + 127)
                            / 255) as u8;
                    }
                }
            }
        }
        // The red origin crosses are at the exact same map-origin anchor in
        // every frame. The green marker is the common48×32 cell center.
        for (dx, dy) in [
            (-2, 0),
            (-1, 0),
            (0, 0),
            (1, 0),
            (2, 0),
            (0, -2),
            (0, -1),
            (0, 1),
            (0, 2),
        ] {
            let at = ((oy + dy) as usize * width + (ox + dx) as usize) * 4;
            rgba[at..at + 4].copy_from_slice(&[255, 90, 70, 255]);
        }
        let at = ((oy + 16) as usize * width + (ox + 24) as usize) * 4;
        rgba[at..at + 4].copy_from_slice(&[70, 255, 90, 255]);
    }
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut encoder = png::Encoder::new(file, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&rgba)?;
    Ok(())
}
pub fn export_reference(contract: &Contract, assets: &Path, output: &Path) -> Result<()> {
    let source_path = source_path(assets, contract.library)?;
    ensure!(
        sprite_project::file_checksum(&source_path)? == contract.source_checksum,
        "Source library changed since contract capture."
    );
    let source = ZlLibrary::open(source_path)?;
    fs::create_dir(output)?;
    let json = serde_json::to_vec_pretty(contract)?;
    fs::write(output.join("reference.json"), json)?;
    render_sheet(
        contract,
        &output.join("contact-sheet.png"),
        |index, kind| source_pixels(&source, index, kind),
    )
}
pub fn preview_project(contract: &Contract, project: &Path, output: &Path) -> Result<()> {
    let manifest = sprite_project::load_manifest(project)?;
    sprite_project::validate(project)?;
    ensure!(
        contract.source_checksum == manifest.source_checksum
            && contract.source_start == manifest.source_start,
        "Preview contract source does not match the project."
    );
    render_sheet(contract, output, |index, kind| {
        let local = index
            .checked_sub(manifest.source_start)
            .context("Preview frame precedes project range.")?;
        let slot = manifest
            .slots
            .get(local)
            .context("Preview frame exceeds project range.")?;
        let surface = match kind {
            SurfaceKind::Image => &slot.image,
            SurfaceKind::Shadow => &slot.shadow,
            SurfaceKind::Overlay => &slot.overlay,
        };
        let Some(surface) = surface else {
            return Ok(None);
        };
        ensure!(
            surface.width <= 1024 && surface.height <= 1024,
            "Contact-sheet surfaces must be at most 1024 pixels per side."
        );
        let (width, height, rgba) = sprite_project::read_png(&project.join(&surface.path))?;
        Ok(Some(Pixels {
            width,
            height,
            rgba,
            offset: if kind == SurfaceKind::Shadow {
                [slot.shadow_offset_x, slot.shadow_offset_y]
            } else {
                [slot.offset_x, slot.offset_y]
            },
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_gate_contract_has_complete_special_animation() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        for image in 540..=543 {
            let c = monster(image, &assets).unwrap();
            assert!(
                c.missing_required.is_empty(),
                "{image}: {:?}",
                c.missing_required
            );
            assert_eq!(c.directions, ["Up", "UpRight", "Right", "UpLeft"]);
            assert_eq!(
                c.frames
                    .iter()
                    .map(|f| f.index)
                    .collect::<BTreeSet<_>>()
                    .len(),
                25
            );
            assert!(!c
                .frames
                .iter()
                .any(|f| f.action == "Struck" && f.direction == 7));
        }
    }
    #[test]
    fn real_monster_contract_uses_runtime_slots_and_exports_references() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        for image in [73, 33] {
            let c = monster(image, &assets).unwrap();
            assert_eq!(c.frames.len(), 240);
            let (_, shape) = monster_sprite(image).unwrap();
            assert_eq!(c.source_start, usize::from(shape % 10) * 1000);
            for frame in &c.frames {
                let action = monster_animation::ACTIONS
                    .into_iter()
                    .find(|a| format!("{a:?}") == frame.action)
                    .unwrap();
                assert_eq!(
                    Some(frame.index as u32),
                    monster_animation::slot(shape, action, frame.direction, frame.frame)
                );
            }
            println!(
                "Monster {image}: {} missing required slots {:?}",
                c.missing_required.len(),
                c.missing_required
            );
            if image == 73 {
                if let Some(output) = std::env::var_os("ZIRCON_SPRITE_REFERENCE_QA_DIR") {
                    export_reference(&c, &assets, Path::new(&output)).unwrap();
                }
            }
        }
    }
    #[test]
    fn png_contact_sheet_has_consistent_origin_guides_and_refuses_overwrite() {
        let output = std::env::temp_dir().join(format!(
            "zircon-contract-sheet-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let c = Contract {
            version: 1,
            kind: "item-icon".into(),
            appearance: 0,
            library: 14,
            library_path: String::new(),
            source_checksum: String::new(),
            source_start: 0,
            source_slots: 1,
            directions: vec!["Static".into()],
            frames: vec![RequiredFrame {
                action: "Static".into(),
                direction: 0,
                frame: 0,
                index: 0,
                interval_ms: 0,
            }],
            slots: vec![],
            missing_required: vec![],
        };
        render_sheet(&c, &output, |_, kind| {
            Ok((kind == SurfaceKind::Image).then(|| Pixels {
                width: 2,
                height: 2,
                rgba: vec![255; 16],
                offset: [-3, -4],
            }))
        })
        .unwrap();
        let (w, h, rgba) = sprite_project::read_png(&output).unwrap();
        assert_eq!((w, h), (67, 52));
        let origin = ((12usize) * usize::from(w) + 11) * 4;
        assert_eq!(&rgba[origin..origin + 4], &[255, 90, 70, 255]);
        assert!(render_sheet(&c, &output, |_, _| Ok(None)).is_err());
        fs::remove_file(output).unwrap();
    }
}
