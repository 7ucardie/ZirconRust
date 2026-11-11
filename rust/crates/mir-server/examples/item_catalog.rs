//! Export item metadata and Storeitems.Zl icons for the static dungeon atlas.
//! cargo run -p mir-server --example item_catalog -- System.db Storeitems.Zl docs/

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use mir_formats::zl::{SurfaceKind, ZlLibrary};
use mir_server::data::GameData;
use serde_json::json;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let db = args.next().expect("System.db path required");
    let sprites = args.next().expect("Storeitems.Zl path required");
    let output = args.next().expect("docs output directory required");
    let data = GameData::load(db)?;
    let library = ZlLibrary::open(sprites)?;
    let output = Path::new(&output);
    let icon_dir = output.join("atlas/items");
    fs::create_dir_all(&icon_dir)?;

    let mut items: Vec<_> = data.items.values().collect();
    items.sort_by_key(|item| item.index);
    let mut exported = BTreeSet::new();
    let mut csv = BufWriter::new(File::create(output.join("data/item_ids.csv"))?);
    writeln!(csv, "id,name,type,image")?;
    let mut catalog = Vec::with_capacity(items.len());
    for item in items {
        let image = item.image;
        let has_icon = if image >= 0 && (image as usize) < library.len() {
            let index = image as usize;
            if exported.contains(&image) {
                true
            } else if let Some(info) = library
                .info(index)
                .filter(|info| info.has_surface(SurfaceKind::Image))
            {
                let decoded = library.decode(index, SurfaceKind::Image)?.unwrap();
                let mut pixels = Vec::with_capacity(info.width as usize * info.height as usize * 4);
                for y in 0..u32::from(info.height) {
                    let start = (y * decoded.width * 4) as usize;
                    pixels.extend_from_slice(
                        &decoded.rgba[start..start + usize::from(info.width) * 4],
                    );
                }
                let file = File::create(icon_dir.join(format!("{image}.png")))?;
                let mut encoder =
                    png::Encoder::new(file, u32::from(info.width), u32::from(info.height));
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder.write_header()?.write_image_data(&pixels)?;
                exported.insert(image);
                true
            } else {
                false
            }
        } else {
            false
        };
        writeln!(
            csv,
            "{},\"{}\",{},{}",
            item.index,
            item.name.replace('"', "\"\""),
            item.item_type,
            image
        )?;
        catalog.push(json!({"id": item.index, "name": item.name, "type": item.item_type, "image": image, "has_icon": has_icon}));
    }
    fs::write(
        output.join("atlas/items.json"),
        serde_json::to_vec(&catalog)?,
    )?;
    eprintln!(
        "Exported {} items and {} icons",
        catalog.len(),
        exported.len()
    );
    Ok(())
}
