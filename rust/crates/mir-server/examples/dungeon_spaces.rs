//! Export dungeon map capacity and spawn data from System.db and Map files.
//! cargo run -p mir-server --example dungeon_spaces -- System.db Client/Map > dungeon_spaces.csv

use std::{collections::BTreeSet, path::Path};

use mir_formats::MapFile;
use mir_server::data::GameData;

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let db = args.next().expect("System.db path required");
    let maps = args.next().expect("Client/Map path required");
    let data = GameData::load(db)?;
    let mut dungeon_maps: Vec<_> = data
        .maps
        .values()
        .filter(|map| map.file_name.starts_with('D'))
        .collect();
    dungeon_maps.sort_by_key(|map| map.index);
    println!("map_id,map_name,map_file,width,height,total_cells,walkable_cells,blocked_cells,walkable_percent,spawn_groups,spawn_count,monster_types,boss_names,exits");
    for map in dungeon_maps {
        let file = MapFile::load(Path::new(&maps).join(format!("{}.map", map.file_name)))?;
        let total = u64::from(file.width) * u64::from(file.height);
        let walkable = file.walkable_count() as u64;
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
        let monsters: BTreeSet<_> = spawns.iter().map(|spawn| spawn.monster).collect();
        let bosses: BTreeSet<_> = monsters
            .iter()
            .filter_map(|id| data.monsters.get(id))
            .filter(|monster| monster.is_boss)
            .map(|monster| monster.name.as_str())
            .collect();
        let exits = data
            .movements
            .iter()
            .filter(|movement| {
                data.regions
                    .get(&movement.source_region)
                    .is_some_and(|region| region.map == map.index)
                    && data
                        .regions
                        .get(&movement.destination_region)
                        .is_some_and(|region| region.map != map.index)
            })
            .count();
        println!(
            "{},{},{},{},{},{},{},{},{:.1},{},{},{},{},{}",
            map.index,
            quoted(&map.description),
            quoted(&map.file_name),
            file.width,
            file.height,
            total,
            walkable,
            total - walkable,
            walkable as f64 * 100.0 / total.max(1) as f64,
            spawns.len(),
            spawns.iter().map(|spawn| spawn.count.max(0)).sum::<i32>(),
            monsters.len(),
            quoted(&bosses.into_iter().collect::<Vec<_>>().join("; ")),
            exits,
        );
    }
    Ok(())
}
