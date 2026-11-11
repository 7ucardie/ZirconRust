//! Private Wood door art for the existing Sabuk gate animation slots.
use anyhow::{ensure, Context, Result};
use mir_formats::{
    monster_animation::{self, Action},
    monster_table::monster_sprite,
    zl::{Sprite, ZlBuilder, ZlLibrary},
};
use std::path::Path;

pub const LIBRARY: &str = "Data/Mon-54.Zl";
const IMAGE: u16 = 543;
const SHAPE: u16 = 3;
const WIDTH: usize = 112;
const HEIGHT: usize = 230;

fn color(base: [u8; 3], x: usize, y: usize, grain: u8) -> [u8; 4] {
    let noise = ((x * 37 + y * 59 + x * y * 7) % 17) as i16 - 8;
    let change = noise * i16::from(grain) / 8;
    [
        base[0].saturating_add_signed(change as i8),
        base[1].saturating_add_signed(change as i8),
        base[2].saturating_add_signed(change as i8),
        255,
    ]
}

/// The attack sequence opens by sliding the plank leaf into the right jamb.
fn frame(step: u8) -> Vec<u8> {
    let mut rgba = vec![0; WIDTH * HEIGHT * 4];
    let leaf_left = 29 + usize::from(step) * 9;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let mut pixel = [0; 4];
            let dx = x.abs_diff(56);
            if (176..188).contains(&y)
                && (dx * dx / 2304 + y.abs_diff(182) * y.abs_diff(182) / 36) < 2
            {
                pixel = [16, 13, 10, 90];
            }
            if (45..169).contains(&y) && (leaf_left..92).contains(&x) {
                let plank = (x - leaf_left) / 11;
                let seam = (x - leaf_left) % 11 <= 1;
                let base = if seam {
                    [45, 29, 20]
                } else if plank % 2 == 0 {
                    [109, 70, 38]
                } else {
                    [92, 57, 32]
                };
                pixel = color(base, x, y, 11);
                if (61..67).contains(&y) || (101..107).contains(&y) || (144..150).contains(&y) {
                    pixel = color([67, 59, 48], x, y, 5);
                    if y == 61 || y == 101 || y == 144 {
                        pixel = [144, 122, 84, 255];
                    }
                }
                let slope = (x - 29) * 72 / 62;
                if (72..143).contains(&y)
                    && (y.abs_diff(72 + slope) <= 2 || y.abs_diff(143 - slope) <= 2)
                {
                    pixel = color([139, 91, 46], x, y, 5);
                }
                if x > 74 && x < 81 && y > 115 && y < 124 {
                    pixel = [170, 139, 77, 255];
                }
            }
            let post = (15..30).contains(&x) || (91..106).contains(&x);
            let lintel = (16..105).contains(&x) && (29 + dx / 12..48 + dx / 16).contains(&y);
            let sill = (16..105).contains(&x) && (169..179).contains(&y);
            if ((35..179).contains(&y) && post) || lintel || sill {
                let edge = !(19..=101).contains(&x) || y < 34 || (168..172).contains(&y);
                pixel = color(if edge { [133, 112, 78] } else { [79, 72, 58] }, x, y, 8);
                if !(30..=90).contains(&x) && (y == 79 || y == 127) {
                    pixel = [35, 33, 29, 255];
                }
            }
            if (52..61).contains(&x) && (24..45).contains(&y) {
                pixel = color([153, 127, 85], x, y, 5);
            }
            if ((23..27).contains(&x) || (94..98).contains(&x)) && (y == 54 || y == 165) {
                pixel = [181, 152, 91, 255];
            }
            rgba[(y * WIDTH + x) * 4..(y * WIDTH + x) * 4 + 4].copy_from_slice(&pixel);
        }
    }
    let mut slanted = vec![0; rgba.len()];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let shifted = 199 + (y as isize - 179) * 23 / 20 + (x as isize - 56) * 3 / 8;
            if (0..HEIGHT as isize).contains(&shifted) {
                let from = (y * WIDTH + x) * 4;
                let to = (shifted as usize * WIDTH + x) * 4;
                slanted[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
            }
        }
    }
    slanted
}

/// Replace only image 543's shape-3 gate frames. Every other source slot stays byte-identical.
pub fn build(assets: &Path) -> Result<Vec<u8>> {
    ensure!(
        monster_sprite(IMAGE) == Some((238, SHAPE)),
        "Wood door sprite mapping changed."
    );
    let source = ZlLibrary::open(assets.join(LIBRARY)).context("Open Wood door sprite source")?;
    ensure!(
        source.len() > 3646,
        "Wood door sprite source lacks gate slots."
    );
    let mut library = source.to_builder()?;
    let mut encoded = Vec::with_capacity(7);
    for step in 0..7 {
        let rgba = frame(step);
        let mut sprite = ZlBuilder::new(source.version());
        sprite.push_rgba(&Sprite {
            width: WIDTH as u16,
            height: HEIGHT as u16,
            offset_x: -56,
            offset_y: -170,
            rgba: &rgba,
            shadow_type: 0,
            shadow_offset_x: 0,
            shadow_offset_y: 0,
            shadow: None,
        });
        encoded.push(sprite.images()[0].clone());
    }
    for direction in [0, 1, 2, 7] {
        let index = monster_animation::slot_for_image(IMAGE, SHAPE, Action::Standing, direction, 0)
            .context("Missing Wood door standing slot")? as usize;
        library.images_mut()[index] = encoded[if direction == 7 { 6 } else { 0 }].clone();
    }
    for direction in [0, 1, 2] {
        for index in 0..2 {
            let slot =
                monster_animation::slot_for_image(IMAGE, SHAPE, Action::Struck, direction, index)
                    .context("Missing Wood door struck slot")? as usize;
            library.images_mut()[slot] = encoded[0].clone();
        }
    }
    for index in 0..7 {
        let slot = monster_animation::slot_for_image(IMAGE, SHAPE, Action::Attack, 7, index)
            .context("Missing Wood door transition slot")? as usize;
        library.images_mut()[slot] = encoded[index as usize].clone();
    }
    for index in 0..8 {
        let slot = monster_animation::slot_for_image(IMAGE, SHAPE, Action::Die, 7, index)
            .context("Missing Wood door final slot")? as usize;
        library.images_mut()[slot] = encoded[(index as usize).min(6)].clone();
    }
    Ok(library.to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mir_formats::zl::SurfaceKind;
    use std::collections::BTreeSet;

    #[test]
    fn door_opens_with_clear_center_and_private_pack_keeps_other_slots() {
        let closed = frame(0);
        let open = frame(6);
        let center = (110 * WIDTH + 56) * 4 + 3;
        assert_eq!(closed[center], 255);
        assert_eq!(open[center], 0);
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source = ZlLibrary::open(assets.join(LIBRARY)).unwrap();
        let original = std::fs::read(assets.join(LIBRARY)).unwrap();
        let built = build(&assets).unwrap();
        let path =
            std::env::temp_dir().join(format!("zircon-wood-door-art-{}.Zl", std::process::id()));
        std::fs::write(&path, built).unwrap();
        let private = ZlLibrary::open(&path).unwrap();
        assert_eq!(source.len(), private.len());
        let replaced: BTreeSet<usize> = [3000, 3010, 3020, 3070]
            .into_iter()
            .chain([3240, 3241, 3250, 3251, 3260, 3261])
            .chain(3320..=3327)
            .chain(3640..=3646)
            .collect();
        assert_eq!(replaced.len(), 25);
        for index in 0..source.len() {
            if !replaced.contains(&index) {
                let source = source.raw(index).unwrap();
                let private = private.raw(index).unwrap();
                assert_eq!(
                    source.as_ref().map(|r| &r.payload),
                    private.as_ref().map(|r| &r.payload),
                    "slot {index}"
                );
                assert_eq!(
                    source.as_ref().map(|r| (
                        r.info.width,
                        r.info.height,
                        r.info.offset_x,
                        r.info.offset_y
                    )),
                    private.as_ref().map(|r| (
                        r.info.width,
                        r.info.height,
                        r.info.offset_x,
                        r.info.offset_y
                    )),
                    "slot {index}"
                );
            }
        }
        for index in [3000, 3010, 3020, 3070, 3240, 3251, 3327, 3640, 3643, 3646] {
            let art = private.decode(index, SurfaceKind::Image).unwrap().unwrap();
            assert!(
                art.rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0),
                "slot {index}"
            );
        }
        assert_eq!(
            private
                .decode(3000, SurfaceKind::Image)
                .unwrap()
                .unwrap()
                .rgba[center],
            255
        );
        assert_eq!(
            private
                .decode(3070, SurfaceKind::Image)
                .unwrap()
                .unwrap()
                .rgba[center],
            0
        );
        if let Some(path) = std::env::var_os("ZIRCON_DOOR_PREVIEW") {
            let closed = private.decode(3000, SurfaceKind::Image).unwrap().unwrap();
            let open = private.decode(3070, SurfaceKind::Image).unwrap().unwrap();
            let mut pair = vec![0; WIDTH * 2 * HEIGHT * 4];
            for y in 0..HEIGHT {
                for (side, image) in [&closed, &open].into_iter().enumerate() {
                    let from = y * WIDTH * 4;
                    let to = (y * WIDTH * 2 + side * WIDTH) * 4;
                    pair[to..to + WIDTH * 4].copy_from_slice(&image.rgba[from..from + WIDTH * 4]);
                }
            }
            let mut encoder = png::Encoder::new(
                std::fs::File::create(path).unwrap(),
                (WIDTH * 2) as u32,
                HEIGHT as u32,
            );
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&pair)
                .unwrap();
        }
        assert_eq!(std::fs::read(assets.join(LIBRARY)).unwrap(), original);
        std::fs::remove_file(path).unwrap();
    }
}
