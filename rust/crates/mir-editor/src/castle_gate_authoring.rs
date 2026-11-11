//! Place genuine castle gates without changing their existing monster artwork.
use anyhow::{Context, Result};
use mir_formats::{
    map::MapFile,
    mirdb::{Collection, Mapping, MirDb, Property, Record, Value},
};
use std::{collections::BTreeSet, path::Path};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub gate: Option<i32>,
    pub castle: Option<i32>,
    pub monster: Option<i32>,
    pub gate_image: Option<i32>,
    pub x: i32,
    pub y: i32,
    pub repair_cost: i32,
}

#[derive(Debug, serde::Serialize)]
pub struct Preview {
    pub gate: Option<i32>,
    pub castle: i32,
    pub map: i32,
    pub file: String,
    pub monster: i32,
    pub clones_monster: bool,
    pub image: i32,
    pub face_image: i32,
    pub x: i32,
    pub y: i32,
    pub repair_cost: i32,
    pub closed_cells: Vec<(i32, i32)>,
}

fn row<'a>(db: &'a MirDb, name: &str, id: i32) -> Result<(&'a Collection, &'a Record)> {
    let collection = db
        .collection(name)
        .with_context(|| format!("Missing {name} collection"))?;
    let mut found = collection
        .records
        .iter()
        .filter(|r| collection.index(r) == id);
    let record = found
        .next()
        .with_context(|| format!("Missing {name} record {id}"))?;
    anyhow::ensure!(found.next().is_none(), "Duplicate {name} identity {id}");
    Ok((collection, record))
}

fn integer(c: &Collection, r: &Record, name: &str) -> Result<i32> {
    i32::try_from(
        c.int(r, name)
            .with_context(|| format!("Missing {}.{name}", c.short_name()))?,
    )
    .with_context(|| format!("{}.{name} is outside Int32", c.short_name()))
}

/// Matches the reference CastleGate.OnSpawned and server gate_blocks layouts.
fn footprint(face: i32, x: i32, y: i32) -> Result<Vec<(i32, i32)>> {
    let offsets: &[(i32, i32)] = match face {
        0 => &[],
        1 => &[(1, 0), (0, 1), (-1, -1), (-1, 0), (0, -1)],
        2 => &[(1, -1), (-1, 0), (0, -1), (0, 1), (1, 0)],
        3 => &[(-1, -1), (-1, 0), (0, 1), (1, 0), (0, -1)],
        _ => anyhow::bail!("Unsupported castle gate FaceImage {face}; expected 0–3"),
    };
    std::iter::once((0, 0))
        .chain(offsets.iter().copied())
        .map(|(dx, dy)| {
            Ok((
                x.checked_add(dx).context("Gate X coordinate overflow")?,
                y.checked_add(dy).context("Gate Y coordinate overflow")?,
            ))
        })
        .collect()
}

pub fn preview(db: &MirDb, assets: &Path, request: &Request) -> Result<Preview> {
    anyhow::ensure!(
        request.gate.is_none() || request.gate_image.is_none(),
        "Relocation cannot change gate artwork"
    );
    let (castle, monster, repair_cost) = if let Some(id) = request.gate {
        let (c, r) = row(db, "CastleGateInfo", id)?;
        let castle = integer(c, r, "Castle")?;
        let monster = integer(c, r, "Monster")?;
        anyhow::ensure!(
            request.castle.is_none_or(|v| v == castle)
                && request.monster.is_none_or(|v| v == monster),
            "Relocation must preserve the gate's castle and monster"
        );
        (castle, monster, integer(c, r, "RepairCost")?)
    } else {
        anyhow::ensure!(request.repair_cost >= 0, "Repair cost must be nonnegative");
        (
            request
                .castle
                .context("Creating a gate requires an existing castle")?,
            request
                .monster
                .context("Creating a gate requires an existing gate monster")?,
            request.repair_cost,
        )
    };
    let (castles, castle_row) = row(db, "CastleInfo", castle)?;
    let map_id = integer(castles, castle_row, "Map")?;
    let (monsters, monster_row) = row(db, "MonsterInfo", monster)?;
    let image = request
        .gate_image
        .unwrap_or(integer(monsters, monster_row, "Image")?);
    anyhow::ensure!(
        (540..=543).contains(&image),
        "Monster must use existing Sabuk gate artwork (Image 540–543)"
    );
    let face = if request.gate_image.is_some() {
        gate_draft(db, monster, castle, image)?;
        [2, 0, 3, 1][(image - 540) as usize]
    } else {
        integer(monsters, monster_row, "FaceImage")?
    };
    let cells = footprint(face, request.x, request.y)?;
    let (maps, map_row) = row(db, "MapInfo", map_id)?;
    let file = maps.str_or(map_row, "FileName", "").to_string();
    anyhow::ensure!(
        !file.is_empty()
            && Path::new(&file)
                .components()
                .all(|p| matches!(p, std::path::Component::Normal(_))),
        "Invalid castle map filename"
    );
    let map = MapFile::load(assets.join("Map").join(format!("{file}.map")))?;
    for &(x, y) in &cells {
        anyhow::ensure!(
            map.cell(x, y).is_some(),
            "Gate footprint ({x}, {y}) is outside the castle map"
        );
        anyhow::ensure!(
            map.is_walkable(x, y),
            "Gate footprint ({x}, {y}) overlaps blocked map cells"
        );
    }
    let occupied: BTreeSet<_> = cells.iter().copied().collect();
    if let Some(gates) = db.collection("CastleGateInfo") {
        for other in &gates.records {
            if request.gate == Some(gates.index(other)) {
                continue;
            }
            let (c, r) = row(db, "CastleInfo", integer(gates, other, "Castle")?)?;
            if integer(c, r, "Map")? != map_id {
                continue;
            }
            let (m, r) = row(db, "MonsterInfo", integer(gates, other, "Monster")?)?;
            let other_cells = footprint(
                integer(m, r, "FaceImage")?,
                integer(gates, other, "X")?,
                integer(gates, other, "Y")?,
            )?;
            anyhow::ensure!(
                other_cells.iter().all(|p| !occupied.contains(p)),
                "Gate footprint overlaps existing gate {}",
                gates.index(other)
            );
        }
    }
    Ok(Preview {
        gate: request.gate,
        castle,
        map: map_id,
        file,
        monster,
        clones_monster: request.gate_image.is_some(),
        image,
        face_image: face,
        x: request.x,
        y: request.y,
        repair_cost,
        closed_cells: cells,
    })
}

fn gate_draft(
    db: &MirDb,
    source: i32,
    castle: i32,
    image: i32,
) -> Result<crate::monster_draft::Draft> {
    let (monsters, r) = row(db, "MonsterInfo", source)?;
    let next = i64::from(monsters.next_index).max(
        i64::from(
            monsters
                .records
                .iter()
                .map(|r| monsters.index(r))
                .max()
                .unwrap_or(0),
        ) + 1,
    );
    crate::monster_draft::propose(
        db,
        source,
        integer(monsters, r, "Level")?,
        &format!("Gate {castle}-{image}-{next}"),
    )
}

fn collection() -> Result<Collection> {
    let name = "Library.SystemModels.CastleGateInfo".to_string();
    Ok(Collection::new(Mapping {
        type_name: name.clone(),
        stored_type_name: name,
        properties: [
            ("Castle", "System.Int32"),
            ("Monster", "System.Int32"),
            ("X", "System.Int32"),
            ("Y", "System.Int32"),
            ("RepairCost", "System.Int32"),
            ("Index", "System.Int32"),
        ]
        .into_iter()
        .map(|(name, type_name)| Property {
            name: name.into(),
            type_name: type_name.into(),
        })
        .collect(),
    })?)
}

pub fn apply(db: &mut MirDb, assets: &Path, request: &Request) -> Result<Preview> {
    let mut result = preview(db, assets, request)?;
    // Stage all collections together: a failure must not leave an orphan monster.
    let mut staged = MirDb {
        collections: db.collections.clone(),
    };
    if request.gate_image.is_some() {
        let draft = gate_draft(&staged, result.monster, result.castle, result.image)?;
        result.monster = draft.apply(&mut staged)?;
        let c = staged.collection_mut("MonsterInfo").unwrap();
        let position = c
            .records
            .iter()
            .position(|r| c.index(r) == result.monster)
            .unwrap();
        let mut record = c.records[position].clone();
        for (field, value) in [
            ("Image", result.image),
            ("FaceImage", result.face_image),
            ("AI", 0),
        ] {
            anyhow::ensure!(
                c.set(&mut record, field, Value::Int(value.into())),
                "Missing gate monster field {field}"
            );
        }
        c.records[position] = record;
    }
    let mut gates = staged
        .collection("CastleGateInfo")
        .cloned()
        .map(Ok)
        .unwrap_or_else(collection)?;
    for name in ["Index", "X", "Y"] {
        anyhow::ensure!(
            gates
                .mapping
                .properties
                .iter()
                .any(|p| p.name == name && p.type_name == "System.Int32"),
            "CastleGateInfo.{name} requires System.Int32"
        );
    }
    if let Some(id) = request.gate {
        let position = gates
            .records
            .iter()
            .position(|r| gates.index(r) == id)
            .context("Gate disappeared")?;
        let mut record = gates.records[position].clone();
        anyhow::ensure!(
            gates.set(&mut record, "X", Value::Int(request.x.into()))
                && gates.set(&mut record, "Y", Value::Int(request.y.into())),
            "Incomplete gate coordinates"
        );
        gates.records[position] = record;
    } else {
        anyhow::ensure!(
            gates.next_index < i32::MAX
                && gates.records.iter().all(|r| gates.index(r) < i32::MAX - 1),
            "Gate identities exhausted"
        );
        for name in ["Castle", "Monster", "RepairCost"] {
            anyhow::ensure!(gates.has_property(name), "Missing CastleGateInfo.{name}");
        }
        result.gate = Some(gates.push_record(vec![
            ("Castle", Value::Int(result.castle.into())),
            ("Monster", Value::Int(result.monster.into())),
            ("X", Value::Int(request.x.into())),
            ("Y", Value::Int(request.y.into())),
            ("RepairCost", Value::Int(request.repair_cost.into())),
        ]));
    }
    if let Some(existing) = staged.collection_mut("CastleGateInfo") {
        *existing = gates;
    } else {
        staged.collections.push(gates);
    }
    staged.to_bytes()?;
    *db = staged;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footprints_are_bounded_and_match_reference_orientations() {
        assert_eq!(footprint(0, 4, 5).unwrap(), [(4, 5)]);
        assert_eq!(
            footprint(1, 4, 5).unwrap(),
            [(4, 5), (5, 5), (4, 6), (3, 4), (3, 5), (4, 4)]
        );
        assert_eq!(
            footprint(2, 4, 5).unwrap(),
            [(4, 5), (5, 4), (3, 5), (4, 4), (4, 6), (5, 5)]
        );
        assert_eq!(
            footprint(3, 4, 5).unwrap(),
            [(4, 5), (3, 4), (3, 5), (4, 6), (5, 5), (4, 4)]
        );
        assert!(footprint(4, 0, 0).is_err());
        assert!(footprint(1, i32::MAX, 0).is_err());
    }

    #[test]
    fn stock_gate_creation_and_relocation_preserve_database_and_validate_footprints() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let mut db = MirDb::load(assets.join("../Database/System.db")).unwrap();
        let source = db.to_bytes().unwrap();
        let monsters = db.collection("MonsterInfo").unwrap();
        let monster = monsters.index(
            monsters
                .records
                .iter()
                .find(|r| monsters.str_or(r, "MonsterName", "") == "Scarecrow")
                .expect("source stat template"),
        );
        let castles = db.collection("CastleInfo").unwrap();
        let castle = castles.index(&castles.records[0]);
        let map_id = integer(castles, &castles.records[0], "Map").unwrap();
        let (maps, r) = row(&db, "MapInfo", map_id).unwrap();
        let file = maps.str_or(r, "FileName", "");
        let map = MapFile::load(assets.join(format!("Map/{file}.map"))).unwrap();
        let candidates: Vec<_> = (3..i32::from(map.width) - 3)
            .flat_map(|x| (3..i32::from(map.height) - 3).map(move |y| (x, y)))
            .filter(|&(x, y)| (-2..=2).all(|dx| (-2..=2).all(|dy| map.is_walkable(x + dx, y + dy))))
            .take(20)
            .collect();
        let (x, y) = candidates[0];
        let request = Request {
            gate: None,
            castle: Some(castle),
            monster: Some(monster),
            gate_image: Some(543),
            x,
            y,
            repair_cost: 1000,
        };
        for invalid in [
            Request {
                castle: Some(i32::MAX),
                ..request
            },
            Request {
                monster: Some(i32::MAX),
                ..request
            },
            Request {
                gate_image: Some(544),
                ..request
            },
            Request {
                repair_cost: -1,
                ..request
            },
            Request {
                x: 0,
                y: 0,
                ..request
            },
        ] {
            assert!(apply(&mut db, &assets, &invalid).is_err());
            assert_eq!(db.to_bytes().unwrap(), source);
        }
        let staged = preview(&db, &assets, &request).unwrap();
        assert_eq!(db.to_bytes().unwrap(), source);
        let made = apply(&mut db, &assets, &request).unwrap();
        assert_eq!(staged.closed_cells, made.closed_cells);
        let created = db.to_bytes().unwrap();
        assert!(apply(&mut db, &assets, &request).is_err());
        assert_eq!(db.to_bytes().unwrap(), created);
        let next = candidates
            .iter()
            .copied()
            .find(|&(xx, yy)| {
                preview(
                    &db,
                    &assets,
                    &Request {
                        gate: made.gate,
                        castle: None,
                        monster: None,
                        gate_image: None,
                        x: xx,
                        y: yy,
                        repair_cost: 0,
                    },
                )
                .is_ok()
                    && (xx, yy) != (x, y)
            })
            .unwrap();
        let move_request = Request {
            gate: made.gate,
            castle: None,
            monster: None,
            gate_image: None,
            x: next.0,
            y: next.1,
            repair_cost: 0,
        };
        let moved = apply(&mut db, &assets, &move_request).unwrap();
        assert_eq!(moved.repair_cost, 1000);
        let loaded = MirDb::parse(&db.to_bytes().unwrap()).unwrap();
        let mut data = mir_server::data::GameData::from_db(&loaded).unwrap();
        let gate = &data
            .castles
            .iter()
            .find(|c| c.index == castle)
            .unwrap()
            .gates[0];
        assert_eq!((gate.x, gate.y), next);
        assert_eq!(gate.monster, made.monster);
        assert_ne!(gate.monster, monster);
        let stats = db.collection("MonsterInfoStat").unwrap();
        let amounts = |id| {
            stats
                .records
                .iter()
                .filter(|r| stats.int(r, "Monster") == Some(i64::from(id)))
                .map(|r| (stats.int(r, "Stat"), stats.int(r, "Amount")))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        assert_eq!(amounts(made.monster), amounts(monster));
        assert_eq!(data.monsters[&gate.monster].image, 543);
        assert!(data.drops.iter().all(|drop| drop.monster != made.monster));
        assert!(data
            .respawns
            .iter()
            .all(|spawn| spawn.monster != made.monster));
        let original = MirDb::parse(&source).unwrap();
        let mut preserved = MirDb {
            collections: vec![],
        };
        for old in &original.collections {
            let mut current = db.collection(old.short_name()).unwrap().clone();
            current.records.truncate(old.records.len());
            current.next_index = old.next_index;
            preserved.collections.push(current);
        }
        assert_eq!(
            preserved.to_bytes().unwrap(),
            source,
            "Existing rows must remain byte-identical"
        );
        // Real runtime: relocating the persisted row moves the gate object and
        // its full blocking footprint. Owner toggles animate and change movement.
        data.respawns.clear(); // Isolate gate movement from unrelated random spawns.
        data.movements.clear(); // Movement proof must not succeed via a teleport.
        let mut world = mir_server::world::World::new(data, assets.join("Map"), None);
        let mut actor = mir_server::accounts::test_character("GateAuthor");
        actor.map = moved.file.clone();
        actor.location = mir_proto::Point::new(moved.x - 2, moved.y);
        let player = world.add_player(1, 1, &actor).unwrap();
        assert_eq!(world.objects[&player].location, actor.location);
        let object = world
            .on_map(map_id)
            .find(|o| o.monster().is_some_and(|m| m.def == moved.monster))
            .unwrap()
            .id;
        assert_eq!(
            world.objects[&object].location,
            mir_proto::Point::new(moved.x, moved.y)
        );
        assert!(world.objects[&object].blocking());
        world
            .objects
            .get_mut(&player)
            .unwrap()
            .player_mut()
            .unwrap()
            .guild = Some(1);
        world
            .guild_store
            .castles
            .push(mir_server::world::guilds::CastleOwner { castle, guild: 1 });
        world.player_move(player, mir_proto::Direction::Right, false);
        assert_eq!(world.objects[&player].location, actor.location);
        world.guild_toggle_castle_gates(player);
        assert!(!world.objects[&object].blocking());
        assert_eq!(
            world.objects[&object].direction,
            mir_proto::Direction::UpLeft
        );
        world.player_move(player, mir_proto::Direction::Right, false);
        assert_eq!(
            world.objects[&player].location,
            mir_proto::Point::new(moved.x - 1, moved.y)
        );
        world.now += 1000;
        world.player_move(player, mir_proto::Direction::Left, false);
        assert_eq!(world.objects[&player].location, actor.location);
        world.now += 1000;
        world.guild_toggle_castle_gates(player);
        assert!(world.objects[&object].blocking());
        assert_eq!(world.objects[&object].direction, mir_proto::Direction::Up);
        world.player_move(player, mir_proto::Direction::Right, false);
        assert_eq!(world.objects[&player].location, actor.location);
        if let Some(out) = std::env::var_os("ZIRCON_GATE_QA_DIR") {
            let out = std::path::PathBuf::from(out);
            std::fs::create_dir_all(&out).unwrap();
            let mut qa_db = MirDb::parse(&db.to_bytes().unwrap()).unwrap();
            let maps = qa_db.collection_mut("MapInfo").unwrap();
            let position = maps
                .records
                .iter()
                .position(|r| maps.index(r) == moved.map)
                .unwrap();
            let mut row = maps.records[position].clone();
            assert!(maps.set(&mut row, "Light", Value::Int(1)));
            maps.records[position] = row;
            qa_db.save(out.join("System.db")).unwrap();
            std::fs::write(
                out.join("gate.json"),
                serde_json::to_vec_pretty(&moved).unwrap(),
            )
            .unwrap();
        }
        let before_invalid = db.to_bytes().unwrap();
        assert!(apply(
            &mut db,
            &assets,
            &Request {
                x: -1,
                ..move_request
            }
        )
        .is_err());
        assert_eq!(db.to_bytes().unwrap(), before_invalid);
        assert_eq!(
            MirDb::load(assets.join("../Database/System.db"))
                .unwrap()
                .to_bytes()
                .unwrap(),
            source
        );
    }
}
