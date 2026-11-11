//! Author private monster asset overrides without changing the renderer or source pack.
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use eframe::egui;
use mir_formats::monster_table::{library_path, monster_sprite};
use mir_formats::zl::{DecodedSurface, RawImage, SurfaceKind, ZlBuilder, ZlLibrary};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Settings {
    pub scale_percent: u16,
    pub tint: [u8; 3],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            scale_percent: 100,
            tint: [255; 3],
        }
    }
}

impl Settings {
    fn validate(self) -> Result<()> {
        ensure!(
            (50..=200).contains(&self.scale_percent),
            "Scale must be 50–200%."
        );
        Ok(())
    }

    fn dimension(self, size: u16) -> Result<u16> {
        if size == 0 {
            return Ok(0);
        }
        let scaled = (u32::from(size) * u32::from(self.scale_percent) + 50) / 100;
        ensure!(
            scaled <= 4096,
            "A scaled sprite surface exceeds 4096 pixels."
        );
        Ok(scaled.max(1) as u16)
    }

    fn offset(self, offset: i16) -> Result<i16> {
        let scaled = (f64::from(offset) * f64::from(self.scale_percent) / 100.0).round();
        ensure!(
            scaled >= f64::from(i16::MIN) && scaled <= f64::from(i16::MAX),
            "Scaled sprite offset is out of range."
        );
        Ok(scaled as i16)
    }
}

fn resized(
    surface: &DecodedSurface,
    width: u16,
    height: u16,
    out_w: u16,
    out_h: u16,
    tint: [u8; 3],
) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(usize::from(out_w) * usize::from(out_h) * 4);
    for y in 0..u32::from(out_h) {
        let sy = y * u32::from(height) / u32::from(out_h);
        for x in 0..u32::from(out_w) {
            let sx = x * u32::from(width) / u32::from(out_w);
            let offset = ((sy * surface.width + sx) * 4) as usize;
            for (channel, multiplier) in tint.iter().enumerate() {
                rgba.push(
                    ((u16::from(surface.rgba[offset + channel]) * u16::from(*multiplier) + 127)
                        / 255) as u8,
                );
            }
            rgba.push(surface.rgba[offset + 3]);
        }
    }
    rgba
}

/// Transform all three surfaces of a slot together. Empty slots and neutral
/// settings remain byte-identical; shadows keep their colour and shadow type.
fn transformed(library: &ZlLibrary, index: usize, settings: Settings) -> Result<Option<RawImage>> {
    settings.validate()?;
    let Some(mut raw) = library.raw(index)? else {
        return Ok(None);
    };
    if settings == Settings::default() || raw.payload.is_empty() {
        return Ok(Some(raw));
    }
    let source = raw.info;
    raw.info.offset_x = settings.offset(source.offset_x)?;
    raw.info.offset_y = settings.offset(source.offset_y)?;
    raw.info.shadow_offset_x = settings.offset(source.shadow_offset_x)?;
    raw.info.shadow_offset_y = settings.offset(source.shadow_offset_y)?;
    let builder = ZlBuilder::new(library.version());
    let original_payload = std::mem::take(&mut raw.payload);
    let mut source_offset = 0;
    for kind in [
        SurfaceKind::Image,
        SurfaceKind::Shadow,
        SurfaceKind::Overlay,
    ] {
        let (w, h) = source.surface_dims(kind);
        let encoded_size = (u32::from(w).div_ceil(4)
            * u32::from(h).div_ceil(4)
            * if library.is_dxt5() { 16 } else { 8 }) as usize;
        let source_end = source_offset + encoded_size;
        if kind == SurfaceKind::Shadow && settings.scale_percent == 100 {
            raw.payload
                .extend_from_slice(&original_payload[source_offset..source_end]);
            source_offset = source_end;
            continue;
        }
        source_offset = source_end;
        let (nw, nh) = (settings.dimension(w)?, settings.dimension(h)?);
        match kind {
            SurfaceKind::Image => {
                raw.info.width = nw;
                raw.info.height = nh;
            }
            SurfaceKind::Shadow => {
                raw.info.shadow_width = nw;
                raw.info.shadow_height = nh;
            }
            SurfaceKind::Overlay => {
                raw.info.overlay_width = nw;
                raw.info.overlay_height = nh;
            }
        }
        let Some(surface) = library.decode(index, kind)? else {
            continue;
        };
        let tint = if kind == SurfaceKind::Shadow {
            [255; 3]
        } else {
            settings.tint
        };
        let rgba = resized(&surface, w, h, nw, nh, tint);
        raw.payload.extend(builder.encode_surface(&rgba, nw, nh));
    }
    Ok(Some(raw))
}

fn source(assets: &Path, image: u16) -> Result<(ZlLibrary, &'static str, usize)> {
    let (lib, shape) =
        monster_sprite(image).context("This monster has no supported sprite mapping.")?;
    let relative = library_path(lib).context("Missing sprite library mapping.")?;
    let path = crate::preview::resolve_case_insensitive(assets, relative)
        .context("The source sprite library was not found.")?;
    let library = ZlLibrary::open(path)?;
    let start = usize::from(shape) * 1000;
    ensure!(
        start < library.len(),
        "The source library has no slots for this monster."
    );
    Ok((library, relative, start))
}

/// Build a complete replacement library. Preserving indices and every other
/// shape's raw payload makes the result compatible with the existing mappings.
fn build(library: &ZlLibrary, start: usize, settings: Settings) -> Result<(ZlBuilder, usize)> {
    settings.validate()?;
    ensure!(start < library.len(), "Missing source shape.");
    let mut builder = ZlBuilder::new(library.version());
    let mut populated = 0;
    for index in 0..library.len() {
        let raw = if (start..start + 1000).contains(&index) {
            let raw = transformed(library, index, settings)?;
            populated += usize::from(raw.as_ref().is_some_and(|r| !r.payload.is_empty()));
            raw
        } else {
            library.raw(index)?
        };
        builder.push_raw(raw);
    }
    ensure!(
        populated > 0,
        "This monster's animation slots contain no surfaces."
    );
    Ok((builder, populated))
}

/// Only new output directories outside the source asset pack are accepted.
fn export(assets: &Path, image: u16, settings: Settings, output: &Path) -> Result<usize> {
    let (library, relative, start) = source(assets, image)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent
        .canonicalize()
        .context("The output parent folder must already exist.")?;
    ensure!(
        !parent.starts_with(assets.canonicalize()?),
        "Export outside the original asset pack."
    );
    ensure!(
        !output.exists(),
        "Choose a new output folder; existing folders are never overwritten."
    );
    let (builder, count) = build(&library, start, settings)?;
    std::fs::create_dir(output)?;
    let result = (|| -> Result<()> {
        let destination = output.join(relative);
        std::fs::create_dir_all(destination.parent().context("Invalid library path")?)?;
        std::fs::write(&destination, builder.to_bytes())?;
        let manifest = serde_json::json!({
            "format": 1, "monster_image": image, "library": relative,
            "first_slot": start, "slot_count": 1000, "populated_slots": count,
            "settings": settings,
            "usage": "Private replacement asset override. All monsters using this Image share the variant. Merge into a separate copy of the complete asset pack; do not overwrite original assets."
        });
        std::fs::write(
            output.join("sprite-variant.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        // Reopen the actual product before reporting success.
        let saved = ZlLibrary::open(&destination)?;
        ensure!(saved.len() == library.len(), "Exported slot count changed.");
        Ok(())
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(output);
        return Err(error);
    }
    Ok(count)
}

#[derive(Default)]
pub struct VariantEditor {
    settings: Settings,
    image: Option<u16>,
    frame: u16,
    output: String,
    status: String,
    texture: Option<(egui::TextureHandle, [u16; 2], [i16; 2])>,
    preview_key: Option<(u16, u16, Settings)>,
}

impl VariantEditor {
    pub fn ui(&mut self, ui: &mut egui::Ui, assets: &Path, image: u16) {
        if self.image != Some(image) {
            self.image = Some(image);
            self.preview_key = None;
            self.status.clear();
        }
        egui::CollapsingHeader::new("Sprite variant export")
            .default_open(std::env::var_os("ZIRCON_EDITOR_SPRITE_VARIANT").is_some())
            .show(ui, |ui| {
            ui.label("Preview a recolour/resize, then export every animation slot, shadow and overlay.");
            ui.small("This replaces the selected Image in a private asset pack. All monsters sharing that Image change together.");
            ui.add(egui::Slider::new(&mut self.settings.scale_percent, 50..=200).text("Size %"));
            ui.horizontal(|ui| { ui.label("Multiply colour:"); ui.color_edit_button_srgb(&mut self.settings.tint); });
            ui.add(egui::Slider::new(&mut self.frame, 0..=999).text("Animation slot"));
            ui.small("Standing directions begin at 0, 10, …, 70. Empty slots stay empty.");
            let key = (image, self.frame, self.settings);
            if self.preview_key != Some(key) {
                self.preview_key = Some(key);
                self.texture = None;
                match self.preview(ui.ctx(), assets, image) {
                    Ok(texture) => { self.texture = Some(texture); self.status.clear(); }
                    Err(error) => self.status = error.to_string(),
                }
            }
            if let Some((texture, [w, h], offsets)) = &self.texture {
                ui.add(egui::Image::new((texture.id(), egui::vec2(f32::from(*w), f32::from(*h)))).bg_fill(egui::Color32::from_gray(40)));
                ui.small(format!("{} × {} px; anchor offset {}, {}", w, h, offsets[0], offsets[1]));
            }
            if self.output.is_empty() { self.output = format!("sprite-variant-{image}"); }
            ui.horizontal(|ui| { ui.label("New output folder:"); ui.text_edit_singleline(&mut self.output); });
            if ui.button("Export complete animation variant").clicked() {
                self.status = match export(assets, image, self.settings, &PathBuf::from(self.output.trim())) {
                    Ok(count) => format!("Exported {count} populated slots to {}. The database is unchanged.", self.output),
                    Err(error) => format!("Export failed: {error:#}"),
                };
            }
            if !self.status.is_empty() { ui.label(&self.status); }
        });
    }

    fn preview(
        &self,
        ctx: &egui::Context,
        assets: &Path,
        image: u16,
    ) -> Result<(egui::TextureHandle, [u16; 2], [i16; 2])> {
        let (library, _, start) = source(assets, image)?;
        let raw = transformed(&library, start + usize::from(self.frame), self.settings)?
            .context("Empty animation slot.")?;
        let info = raw.info;
        if info.width == 0 || info.height == 0 || raw.payload.is_empty() {
            bail!("This slot has no body surface.");
        }
        // Preview the encoded result so compression matches the exported asset.
        let pw = u32::from(info.width).div_ceil(4) * 4;
        let ph = u32::from(info.height).div_ceil(4) * 4;
        let rgba = mir_formats::dxt::decode(&raw.payload, pw, ph, library.is_dxt5());
        let surface = DecodedSurface {
            width: pw,
            height: ph,
            rgba,
        };
        let rgba = resized(
            &surface,
            info.width,
            info.height,
            info.width,
            info.height,
            [255; 3],
        );
        let pixels = egui::ColorImage::from_rgba_unmultiplied(
            [usize::from(info.width), usize::from(info.height)],
            &rgba,
        );
        let texture = ctx.load_texture(
            "monster-variant-preview",
            pixels,
            egui::TextureOptions::NEAREST,
        );
        Ok((
            texture,
            [info.width, info.height],
            [info.offset_x, info.offset_y],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mir_formats::zl::ImageInfo;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn fixture() -> (PathBuf, ZlLibrary) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "zircon-sprite-variant-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut b = ZlBuilder::new(1);
        let body = [240, 160, 80, 255].repeat(15);
        let shadow = [32, 32, 32, 128].repeat(8);
        let overlay = [80, 160, 240, 255].repeat(6);
        let mut payload = b.encode_surface(&body, 5, 3);
        payload.extend(b.encode_surface(&shadow, 4, 2));
        payload.extend(b.encode_surface(&overlay, 3, 2));
        let raw = RawImage {
            info: ImageInfo {
                width: 5,
                height: 3,
                offset_x: -7,
                offset_y: 3,
                shadow_type: 2,
                shadow_width: 4,
                shadow_height: 2,
                shadow_offset_x: -5,
                shadow_offset_y: -3,
                overlay_width: 3,
                overlay_height: 2,
                ..Default::default()
            },
            payload,
        };
        b.push_raw(Some(raw.clone()));
        b.images_mut().resize(1000, None);
        b.push_raw(Some(raw));
        let file = dir.join("source.Zl");
        std::fs::write(&file, b.to_bytes()).unwrap();
        (dir, ZlLibrary::open(file).unwrap())
    }

    #[test]
    fn variant_scales_all_surfaces_and_offsets_but_keeps_shadow_colour_and_other_shapes() {
        let (dir, library) = fixture();
        let settings = Settings {
            scale_percent: 200,
            tint: [255, 0, 0],
        };
        let (builder, count) = build(&library, 0, settings).unwrap();
        assert_eq!(count, 1);
        assert_eq!(builder.len(), library.len());
        assert!(builder.images()[1].is_none());
        assert_eq!(
            builder.images()[1000].as_ref().unwrap().payload,
            library.raw(1000).unwrap().unwrap().payload
        );
        std::fs::write(dir.join("variant.Zl"), builder.to_bytes()).unwrap();
        let variant = ZlLibrary::open(dir.join("variant.Zl")).unwrap();
        let info = variant.info(0).unwrap();
        assert_eq!(
            (info.width, info.height, info.offset_x, info.offset_y),
            (10, 6, -14, 6)
        );
        assert_eq!(
            (
                info.shadow_width,
                info.shadow_height,
                info.shadow_offset_x,
                info.shadow_offset_y
            ),
            (8, 4, -10, -6)
        );
        assert_eq!(
            (info.overlay_width, info.overlay_height, info.shadow_type),
            (6, 4, 2)
        );
        for kind in [SurfaceKind::Image, SurfaceKind::Overlay] {
            let px = variant.decode(0, kind).unwrap().unwrap();
            assert!(px.rgba[0] > 30);
            assert_eq!(&px.rgba[1..4], &[0, 0, 255]);
        }
        let original = library.decode(0, SurfaceKind::Shadow).unwrap().unwrap();
        let altered = variant.decode(0, SurfaceKind::Shadow).unwrap().unwrap();
        assert_eq!(&original.rgba[..4], &altered.rgba[..4]);
        drop(library);
        drop(variant);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn neutral_variant_is_byte_identical_and_invalid_settings_reject() {
        let (dir, library) = fixture();
        let (builder, _) = build(&library, 0, Settings::default()).unwrap();
        assert_eq!(builder.to_bytes(), std::fs::read(library.path()).unwrap());
        assert!(build(
            &library,
            0,
            Settings {
                scale_percent: 0,
                tint: [255; 3]
            }
        )
        .is_err());
        assert!(build(&library, 2000, Settings::default()).is_err());
        let settings = Settings {
            scale_percent: 200,
            tint: [255; 3],
        };
        assert!(settings.offset(i16::MIN).is_err());
        assert!(settings.dimension(3000).is_err());
        assert_eq!(
            Settings {
                scale_percent: 50,
                ..settings
            }
            .offset(-3)
            .unwrap(),
            -2
        );
        drop(library);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn asset_oma_variant_exports_all_slots_without_touching_source() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        let output = std::env::var_os("ZIRCON_SPRITE_QA_DIR").map(PathBuf::from);
        let keep = output.is_some();
        let output = output.unwrap_or_else(|| {
            std::env::temp_dir().join(format!("zircon-oma-variant-{}", std::process::id()))
        });
        let (original, relative, start) = source(&assets, 33).unwrap();
        let original_bytes = std::fs::read(original.path()).unwrap();
        let settings = Settings {
            scale_percent: 125,
            tint: [170, 255, 170],
        };
        let count = export(&assets, 33, settings, &output).unwrap();
        assert!(
            count > 100,
            "Expected complete Oma animation set, got {count}"
        );
        let variant = ZlLibrary::open(output.join(relative)).unwrap();
        assert_eq!(variant.len(), original.len());
        for index in 0..original.len() {
            if !(start..start + 1000).contains(&index) {
                assert_eq!(
                    variant.raw(index).unwrap().map(|r| r.payload),
                    original.raw(index).unwrap().map(|r| r.payload)
                );
            } else {
                assert_eq!(
                    variant.info(index).is_some(),
                    original.info(index).is_some()
                );
                if let Some(before) = original.info(index) {
                    let after = variant.info(index).unwrap();
                    assert_eq!(after.shadow_type, before.shadow_type);
                    for kind in [
                        SurfaceKind::Image,
                        SurfaceKind::Shadow,
                        SurfaceKind::Overlay,
                    ] {
                        assert_eq!(
                            variant.decode(index, kind).unwrap().is_some(),
                            original.decode(index, kind).unwrap().is_some()
                        );
                    }
                }
            }
        }
        assert_eq!(std::fs::read(original.path()).unwrap(), original_bytes);
        assert!(
            export(&assets, 33, settings, &output).is_err(),
            "An existing output must never be replaced."
        );
        assert!(export(&assets, 33, settings, &assets.join("forbidden-variant")).is_err());
        println!(
            "Oma Image=33, scale=125%, tint=170,255,170; {count} populated slots: {}",
            output.display()
        );
        drop(variant);
        drop(original);
        if !keep {
            std::fs::remove_dir_all(output).unwrap();
        }
    }

    #[test]
    fn resizing_crops_dxt_padding_and_preserves_transparent_pixels() {
        let surface = DecodedSurface {
            width: 4,
            height: 4,
            rgba: vec![
                200, 100, 40, 0, 100, 50, 20, 128, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
        };
        assert_eq!(
            resized(&surface, 2, 1, 4, 1, [255, 0, 255]),
            vec![200, 0, 40, 0, 200, 0, 40, 0, 100, 0, 20, 128, 100, 0, 20, 128]
        );
    }
}
