//! Export public, read-only dungeon details from the default game database.
//! cargo run -p mir-server --example dungeon_atlas -- System.db Client/Map site/

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::path::Path;

use mir_formats::MapFile;
use mir_server::data::GameData;
use serde_json::json;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let db = args.next().expect("System.db path required");
    let maps = args.next().expect("Client/Map path required");
    let output = args.next().expect("output directory required");
    let data = GameData::load(db)?;
    let output = Path::new(&output);
    fs::create_dir_all(output.join("maps"))?;
    let mut atlas = Vec::new();
    for map in data
        .maps
        .values()
        .filter(|map| map.file_name.starts_with('D'))
    {
        let file = MapFile::load(Path::new(&maps).join(format!("{}.map", map.file_name)))?;
        let mut pixels = Vec::with_capacity(usize::from(file.width) * usize::from(file.height) * 3);
        for y in 0..i32::from(file.height) {
            for x in 0..i32::from(file.width) {
                pixels.extend_from_slice(if file.is_walkable(x, y) {
                    &[177, 151, 99]
                } else {
                    &[21, 26, 24]
                });
            }
        }
        let image = File::create(output.join("maps").join(format!("{}.png", map.index)))?;
        let mut encoder = png::Encoder::new(image, u32::from(file.width), u32::from(file.height));
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&pixels)?;

        let spawns: Vec<_> = data
            .respawns
            .iter()
            .filter(|spawn| !spawn.event_spawn)
            .filter(|spawn| {
                data.regions
                    .get(&spawn.region)
                    .is_some_and(|r| r.map == map.index)
            })
            .collect();
        let total: i32 = spawns.iter().map(|s| s.count.max(0)).sum();
        let mut populations: BTreeMap<i32, (i32, BTreeSet<i32>)> = BTreeMap::new();
        for spawn in spawns {
            let entry = populations.entry(spawn.monster).or_default();
            entry.0 += spawn.count.max(0);
            entry.1.insert(spawn.drop_set);
        }
        let monsters: Vec<_> = populations
            .into_iter()
            .filter_map(|(id, (count, sets))| {
                let monster = data.monsters.get(&id)?;
                let mut drops: Vec<_> = data
                    .drops
                    .iter()
                    .filter(|drop| {
                        drop.monster == id
                            && drop.chance > 0
                            && !drop.part_only
                            && !drop.easter_event
                    })
                    .filter(|drop| {
                        sets.iter()
                            .any(|set| (set & drop.drop_set) == drop.drop_set)
                    })
                    .filter_map(|drop| {
                        data.items.get(&drop.item).map(|item| {
                            json!({
                                "item_id":item.index,"item":item.name,"one_in":drop.chance,
                                "amount":drop.amount,"drop_set":drop.drop_set
                            })
                        })
                    })
                    .collect();
                drops.sort_by_key(|drop| drop["one_in"].as_i64().unwrap_or(i64::MAX));
                Some(
                    json!({"id":id,"name":monster.name,"boss":monster.is_boss,"count":count,
                "share":if total > 0 { 100.0 * f64::from(count) / f64::from(total) } else { 0.0 },
                "drops":drops}),
                )
            })
            .collect();
        atlas.push(json!({
            "map_id":map.index,
            "map_name":map.description,
            "map_file":map.file_name,
            "width":file.width,
            "height":file.height,
            "monsters":monsters
        }));
    }
    fs::write(output.join("details.json"), serde_json::to_vec(&atlas)?)?;
    Ok(())
}
