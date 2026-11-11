use super::{parse_world_brief, Workspace};
use crate::{generate, item_draft};
use mir_formats::{map::MapFile, mirdb::MirDb, navigation::Navigation};
use serde_json::json;
use std::path::{Path, PathBuf};

static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "zircon-mcp-authoring-{}-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn workspace(&self) -> Workspace {
        let assets = self.0.join("assets");
        std::fs::create_dir_all(assets.join("Map")).unwrap();
        let db = self.0.join("Source.db");
        std::fs::write(
            &db,
            MirDb {
                collections: vec![],
            }
            .to_bytes()
            .unwrap(),
        )
        .unwrap();
        Workspace::open(&self.0.join("project"), &assets, &db).unwrap()
    }
    fn project(&self, relative: &str) -> PathBuf {
        self.0.join("project").join(relative)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn item_exterior_effect_writes_private_revision() {
    let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
        return;
    };
    let fixture = Fixture::new();
    let database = assets.join("../Database/System.db");
    let source = std::fs::read(&database).unwrap();
    let workspace = Workspace::open(&fixture.0.join("project"), &assets, &database).unwrap();
    std::fs::create_dir_all(fixture.project("base")).unwrap();
    std::fs::write(fixture.project("base/System.db"), &source).unwrap();
    workspace
        .call(
            "item_exterior_effect",
            json!({
                "database":"project:base/System.db", "item_id":1162,
                "effect":10, "output":"winged-blade"
            }),
        )
        .unwrap();
    let changed = MirDb::load(fixture.project("winged-blade/System.db")).unwrap();
    let items = changed.collection("ItemInfo").unwrap();
    let blade = items
        .records
        .iter()
        .find(|row| items.index(row) == 1162)
        .unwrap();
    assert_eq!(items.int_or(blade, "ExteriorEffect", 0), 10);
    assert_eq!(std::fs::read(&database).unwrap(), source);
    assert_eq!(
        std::fs::read(fixture.project("base/System.db")).unwrap(),
        source
    );
}

#[test]
fn simple_world_briefs_select_shape_and_reject_ignored_details() {
    let dungeon = parse_world_brief("Make a six-room Wood dungeon with chickens").unwrap();
    assert_eq!(dungeon.mode, "rooms");
    assert_eq!(dungeon.rooms, Some(6));
    assert_eq!(dungeon.monster, "chickens");
    assert_eq!(
        parse_world_brief("organic Wood cave featuring Chicken")
            .unwrap()
            .mode,
        "organic"
    );
    assert_eq!(
        parse_world_brief("Wood chamber with Chicken").unwrap().mode,
        "chamber"
    );
    for brief in [
        "a thirteen-room Wood dungeon with chickens",
        "a mossy Wood dungeon with chickens",
        "a Wood cave with chickens and goblins",
        "a Wood dungeon with chickens and treasure",
    ] {
        assert!(parse_world_brief(brief).is_err());
    }
}

#[test]
fn world_brief_creates_playable_bundle_with_installed_monster() {
    let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
        return;
    };
    let fixture = Fixture::new();
    let workspace = Workspace::open(
        &fixture.0.join("project"),
        &assets,
        &assets.join("../Database/System.db"),
    )
    .unwrap();
    let result = workspace
        .call(
            "world_from_brief",
            json!({
                "brief":"Wood chamber with chickens", "output":"brief-world", "seed":42
            }),
        )
        .unwrap();
    assert_eq!(result["interpreted"]["mode"], "chamber");
    assert_eq!(result["interpreted"]["monster"], "Chicken");
    assert!(fixture.project("brief-world/System.db").is_file());
    assert!(fixture.project("brief-world/README.txt").is_file());
    let rooms = workspace
        .call(
            "world_from_brief",
            json!({"brief":"four-room Wood dungeon with chickens","output":"brief-rooms","seed":42}),
        )
        .unwrap();
    assert_eq!(rooms["interpreted"]["rooms"], 4);
    let layout_path = std::fs::read_dir(fixture.project("brief-rooms/Map"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().ends_with(".cave.json"))
        .unwrap();
    let layout: serde_json::Value =
        serde_json::from_slice(&std::fs::read(layout_path).unwrap()).unwrap();
    assert_eq!(layout[0]["rooms"].as_array().unwrap().len(), 4);
    assert!(workspace
        .call(
            "world_from_brief",
            json!({
                "brief":"mossy Wood cave with chickens", "output":"bad-brief"
            })
        )
        .is_err());
    assert!(!fixture.project("bad-brief").exists());
    assert!(workspace
        .call(
            "world_from_brief",
            json!({"brief":"Wood chamber with nonexistentmonster","output":"bad-monster"})
        )
        .is_err());
    assert!(!fixture.project("bad-monster").exists());
}

#[test]
fn map_revisions_preserve_sources_and_reject_invalid_edits_without_output() {
    let fixture = Fixture::new();
    let workspace = fixture.workspace();
    workspace
        .call("map_create", json!({"output":"base","width":4,"height":4}))
        .unwrap();
    let base = fixture.project("base/Map.map");
    let original = std::fs::read(&base).unwrap();
    let inspection = workspace
        .call(
            "map_inspect",
            json!({"map":"project:base/Map.map","points":[[1,1]]}),
        )
        .unwrap();
    assert_eq!(inspection["walkable_cells"], 16);
    workspace.call("map_collision", json!({"map":"project:base/Map.map","output":"blocked","points":[[1,1],[1,1]],"walkable":false})).unwrap();
    let blocked = MapFile::load(fixture.project("blocked/Map.map")).unwrap();
    assert!(!blocked.is_walkable(1, 1));
    assert_eq!(blocked.walkable_count(), 15);
    assert_eq!(std::fs::read(&base).unwrap(), original);
    workspace.call("map_copy_area",json!({"source":"project:blocked/Map.map","map":"project:base/Map.map","bounds":[0,0,2,2],"x":2,"y":2,"output":"area"})).unwrap();
    let area = MapFile::load(fixture.project("area/Map.map")).unwrap();
    assert!(!area.is_walkable(3, 3));
    assert_eq!(area.cell(2, 2), blocked.cell(0, 0));
    for (bounds, x, output) in [
        (json!([0, 0, 3, 2]), 0, "oddarea"),
        (json!([0, 0, 2, 2]), 1, "oddtarget"),
        (json!([0, 0, 66, 2]), 0, "largearea"),
    ] {
        assert!(workspace.call("map_copy_area",json!({"source":"project:blocked/Map.map","map":"project:base/Map.map","bounds":bounds,"x":x,"y":0,"output":output})).is_err());
        assert!(!fixture.project(output).exists());
    }
    workspace
        .call(
            "map_copy",
            json!({"map":"project:blocked/Map.map","output":"copy"}),
        )
        .unwrap();
    assert_eq!(
        std::fs::read(fixture.project("copy/Map.map")).unwrap(),
        blocked.to_bytes()
    );
    for (name, arguments) in [
        ("map_create", json!({"output":"odd","width":3,"height":4})),
        (
            "map_collision",
            json!({"map":"project:base/Map.map","output":"outside","points":[[1,1],[4,4]],"walkable":false}),
        ),
        (
            "map_stamp",
            json!({"map":"project:base/Map.map","output":"badkit","kit":"unknown","x":0,"y":0}),
        ),
        ("map_create", json!({"output":"base","width":8,"height":8})),
    ] {
        assert!(workspace.call(name, arguments).is_err());
    }
    for missing in ["odd", "outside", "badkit"] {
        assert!(!fixture.project(missing).exists());
    }
    assert_eq!(std::fs::read(base).unwrap(), original);
}

#[test]
fn map_layout_is_retained_and_invalidating_collision_is_refused_atomically() {
    let fixture = Fixture::new();
    let workspace = fixture.workspace();
    workspace
        .call(
            "map_create",
            json!({"output":"base","width":16,"height":16}),
        )
        .unwrap();
    let base = fixture.project("base/Map.map");
    let map = MapFile::load(&base).unwrap();
    let layout = generate::layout(&map, 42).unwrap();
    std::fs::write(
        base.with_extension("cave.json"),
        serde_json::to_vec(&(layout.clone(), 123)).unwrap(),
    )
    .unwrap();
    layout.navigation(&map).unwrap().save(&base, &map).unwrap();
    workspace
        .call(
            "map_copy",
            json!({"map":"project:base/Map.map","output":"copy"}),
        )
        .unwrap();
    let copy_path = fixture.project("copy/Map.map");
    let copy = MapFile::load(&copy_path).unwrap();
    let copied_layout = generate::read_layout(&copy_path).unwrap().unwrap();
    assert_eq!(copied_layout.0.start, layout.start);
    assert_eq!(copied_layout.1, 123);
    assert_eq!(
        Navigation::load(&copy_path, &copy).unwrap().unwrap(),
        layout.navigation(&map).unwrap()
    );
    assert!(workspace.call("map_collision",json!({"map":"project:base/Map.map","output":"invalid","points":[layout.start],"walkable":false})).is_err());
    assert!(!fixture.project("invalid").exists());
    assert_eq!(std::fs::read(base).unwrap(), map.to_bytes());
}

#[test]
fn traversal_and_asset_project_roots_are_rejected() {
    let fixture = Fixture::new();
    let workspace = fixture.workspace();
    for map in [
        "project:../Source.db",
        "asset:../secret.map",
        "project:/tmp/map.map",
        "unknown:Map.map",
    ] {
        assert!(workspace.call("map_inspect", json!({"map":map})).is_err());
    }
    for output in ["../outside", "nested/revision", "", "bad.name"] {
        assert!(workspace
            .call("map_create", json!({"output":output,"width":4,"height":4}))
            .is_err());
    }
    assert!(Workspace::open(
        &fixture.0.join("assets/forbidden"),
        &fixture.0.join("assets"),
        &fixture.0.join("Source.db")
    )
    .is_err());
    assert!(!fixture.0.join("assets/forbidden").exists());
}

#[test]
fn standalone_navigation_survives_copy_and_collision_with_a_fresh_checksum() {
    let fixture = Fixture::new();
    let workspace = fixture.workspace();
    workspace
        .call("map_create", json!({"output":"base","width":4,"height":4}))
        .unwrap();
    let base = fixture.project("base/Map.map");
    let map = MapFile::load(&base).unwrap();
    let navigation = Navigation::new(
        &map,
        vec![mir_formats::navigation::Portal {
            position: (0, 0),
            destination: (3, 3),
        }],
    )
    .unwrap();
    navigation.save(&base, &map).unwrap();
    workspace
        .call(
            "map_copy",
            json!({"map":"project:base/Map.map","output":"copy"}),
        )
        .unwrap();
    let copy = fixture.project("copy/Map.map");
    assert_eq!(
        Navigation::load(&copy, &MapFile::load(&copy).unwrap())
            .unwrap()
            .unwrap(),
        navigation
    );
    workspace.call("map_collision", json!({"map":"project:base/Map.map","output":"edited","points":[[1,1]],"walkable":false})).unwrap();
    let edited_path = fixture.project("edited/Map.map");
    let edited = MapFile::load(&edited_path).unwrap();
    let updated = Navigation::load(&edited_path, &edited).unwrap().unwrap();
    assert_eq!(updated.portals, navigation.portals);
    assert_ne!(updated.map_checksum, navigation.map_checksum);
    assert!(workspace.call("map_collision", json!({"map":"project:base/Map.map","output":"invalid","points":[[0,0]],"walkable":false})).is_err());
    assert!(!fixture.project("invalid").exists());
}

#[cfg(unix)]
#[test]
fn escaped_or_oversized_metadata_is_rejected_before_revision_creation() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let workspace = fixture.workspace();
    workspace
        .call(
            "map_create",
            json!({"output":"base","width":16,"height":16}),
        )
        .unwrap();
    let base = fixture.project("base/Map.map");
    let map = MapFile::load(&base).unwrap();
    let layout = generate::layout(&map, 42).unwrap();
    for extension in ["cave.json", "navigation.json"] {
        let target = fixture.0.join(extension);
        let bytes = if extension == "cave.json" {
            serde_json::to_vec(&(layout.clone(), 1)).unwrap()
        } else {
            serde_json::to_vec(&layout.navigation(&map).unwrap()).unwrap()
        };
        std::fs::write(&target, bytes).unwrap();
        symlink(&target, base.with_extension(extension)).unwrap();
        let error = workspace
            .call(
                "map_copy",
                json!({"map":"project:base/Map.map","output":"escaped"}),
            )
            .unwrap_err();
        assert!(error.to_string().contains("metadata escapes"));
        assert!(!fixture.project("escaped").exists());
        std::fs::remove_file(base.with_extension(extension)).unwrap();
    }
    std::fs::write(
        base.with_extension("cave.json"),
        vec![b' '; 1024 * 1024 + 1],
    )
    .unwrap();
    let error = workspace
        .call(
            "map_copy",
            json!({"map":"project:base/Map.map","output":"oversized"}),
        )
        .unwrap_err();
    assert!(error.to_string().contains("exceeds 1 MiB"));
    assert!(!fixture.project("oversized").exists());
}

#[cfg(unix)]
#[test]
fn symlink_inputs_and_output_revisions_cannot_escape_project() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let workspace = fixture.workspace();
    let outside = fixture.0.join("outside.map");
    let original = MapFile::blank(4, 4).to_bytes();
    std::fs::write(&outside, &original).unwrap();
    symlink(&outside, fixture.project("escape.map")).unwrap();
    assert!(workspace
        .call(
            "map_copy",
            json!({"map":"project:escape.map","output":"escaped"})
        )
        .is_err());
    let target = fixture.0.join("outside-folder");
    std::fs::create_dir(&target).unwrap();
    symlink(&target, fixture.project("linked")).unwrap();
    assert!(workspace
        .call(
            "map_create",
            json!({"output":"linked","width":4,"height":4})
        )
        .is_err());
    symlink(
        fixture.0.join("missing-folder"),
        fixture.project("dangling"),
    )
    .unwrap();
    assert!(workspace
        .call(
            "map_create",
            json!({"output":"dangling","width":4,"height":4})
        )
        .is_err());
    assert!(!fixture.project("escaped").exists());
    assert_eq!(std::fs::read_dir(target).unwrap().count(), 0);
    assert_eq!(std::fs::read(outside).unwrap(), original);
}

fn verify_bundle(folder: &Path, monster: i32, expected_item: i32) -> PathBuf {
    let map_path = std::fs::read_dir(folder.join("Map"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "map"))
        .unwrap();
    let map = MapFile::load(&map_path).unwrap();
    let (layout, selected) = generate::read_layout(&map_path).unwrap().unwrap();
    assert_eq!(selected, monster);
    layout.validate(&map).unwrap();
    assert_eq!(
        Navigation::load(&map_path, &map).unwrap().unwrap(),
        layout.navigation(&map).unwrap()
    );
    let db = MirDb::load(folder.join("System.db")).unwrap();
    let items = db.collection("ItemInfo").unwrap();
    assert!(items
        .records
        .iter()
        .any(|row| items.index(row) == expected_item));
    let monsters = db.collection("MonsterInfo").unwrap();
    assert!(monsters
        .records
        .iter()
        .any(|row| monsters.index(row) == monster));
    let maps = db.collection("MapInfo").unwrap();
    let file = map_path.file_stem().unwrap().to_str().unwrap();
    let map_id = maps
        .records
        .iter()
        .find(|row| maps.str_or(row, "FileName", "") == file)
        .map(|row| maps.index(row))
        .unwrap();
    let regions = db.collection("MapRegion").unwrap();
    let ids: Vec<_> = regions
        .records
        .iter()
        .filter(|row| regions.int_or(row, "Map", -1) == i64::from(map_id))
        .map(|row| regions.index(row))
        .collect();
    let spawns = db.collection("RespawnInfo").unwrap();
    let spawn = spawns
        .records
        .iter()
        .find(|row| ids.contains(&(spawns.int_or(row, "Region", -1) as i32)))
        .unwrap();
    assert_eq!(spawns.int_or(spawn, "Monster", -1), i64::from(monster));
    assert_eq!(spawns.int_or(spawn, "Count", 0), 8);
    assert_eq!(spawns.int_or(spawn, "Delay", 0), 30);
    map_path
}

#[test]
fn asset_drafts_and_playable_bundles_chain_through_private_database_revisions() {
    let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
        return;
    };
    let fixture = Fixture::new();
    let database = assets.join("../Database/System.db");
    let source_bytes = std::fs::read(&database).unwrap();
    let map_bytes = std::fs::read(assets.join("Map/D1006.map")).unwrap();
    let workspace = Workspace::open(&fixture.0.join("project"), &assets, &database).unwrap();
    let search = workspace
        .call(
            "content_search",
            json!({"kind":"monsters","filter":"Chicken","limit":100}),
        )
        .unwrap();
    let chicken = search["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "Chicken")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let monster=workspace.call("draft_variant",json!({"kind":"monster","source_id":chicken,"level":20,"name":"MCP Test Chicken","output":"monster"})).unwrap()["created_id"].as_i64().unwrap() as i32;
    let db = MirDb::load(fixture.project("monster/System.db")).unwrap();
    let items = db.collection("ItemInfo").unwrap();
    let item_source = items
        .records
        .iter()
        .find_map(|row| item_draft::propose(&db, items.index(row), 30, "MCP Test Item").ok())
        .unwrap()
        .source_id;
    let item=workspace.call("draft_variant",json!({"kind":"item","source_id":item_source,"level":30,"name":"MCP Test Item","output":"item","database":"project:monster/System.db"})).unwrap()["created_id"].as_i64().unwrap() as i32;
    let search = workspace
        .call(
            "content_search",
            json!({"kind":"items","filter":"MCP Test Item","database":"project:item/System.db"}),
        )
        .unwrap();
    assert_eq!(search["entries"][0]["id"], item);
    workspace.call("world_generate",json!({"source":"asset:D1006.map","mode":"chamber","width":80,"height":80,"seed":42,"monster_id":monster,"output":"world","database":"project:item/System.db"})).unwrap();
    let map_path = verify_bundle(&fixture.project("world"), monster, item);
    let relative = format!(
        "project:world/Map/{}",
        map_path.file_name().unwrap().to_str().unwrap()
    );
    workspace.call("world_populate",json!({"map":relative,"seed":7,"monster_id":monster,"output":"populated","database":"project:world/System.db"})).unwrap();
    verify_bundle(&fixture.project("populated"), monster, item);
    let populated_map = std::fs::read_dir(fixture.project("populated/Map"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|extension| extension == "map"))
        .unwrap();
    let populated_layout = generate::read_layout(&populated_map).unwrap().unwrap().0;
    assert!(populated_layout.enclosed);
    assert_eq!(populated_layout.seed, 42);
    let perimeter_source = std::fs::read(assets.join("Map/D1008.map")).unwrap();
    let perimeter = workspace.call("world_perimeter", json!({"nw":3,"ne":4,"seed":23,"monster_id":monster,"output":"perimeter","database":"project:item/System.db"})).unwrap();
    assert_eq!(perimeter["wall_repeats"], json!([3, 4]));
    assert_eq!(perimeter["enclosed"], true);
    let perimeter_map_path = verify_bundle(&fixture.project("perimeter"), monster, item);
    let perimeter_map = MapFile::load(&perimeter_map_path).unwrap();
    let perimeter_layout = generate::read_layout(&perimeter_map_path)
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(perimeter_layout.wall_repeats, Some([3, 4]));
    assert!(perimeter_layout.enclosed);
    assert_eq!(perimeter_layout.wall_rooms, 1);
    let connected = workspace.call("world_perimeter", json!({"rooms":2,"nw":3,"ne":4,"seed":23,"monster_id":monster,"output":"connected","database":"project:item/System.db"})).unwrap();
    assert_eq!(connected["wall_rooms"], 2);
    let connected_path = verify_bundle(&fixture.project("connected"), monster, item);
    let connected_layout = generate::read_layout(&connected_path).unwrap().unwrap().0;
    assert_eq!(connected_layout.wall_rooms, 2);
    assert!(connected_layout.enclosed);
    let connected_map = MapFile::load(&connected_path).unwrap();
    let doorways = connected["doorways"].as_array().unwrap();
    assert_eq!(doorways.len(), 1);
    let center = doorways[0]["center"].as_array().unwrap();
    let center = (
        center[0].as_i64().unwrap() as i32,
        center[1].as_i64().unwrap() as i32,
    );
    assert_eq!(doorways[0]["interactive"], false);
    assert_eq!(doorways[0]["passable"], true);
    assert_eq!(
        serde_json::to_value(crate::wood_doorway::inspect(&connected_map, center).unwrap())
            .unwrap(),
        doorways[0]
    );
    let varied = workspace.call("world_perimeter", json!({"repeats":[[3,3],[4,3],[3,3]],"seed":23,"monster_id":monster,"output":"varied","database":"project:item/System.db"})).unwrap();
    assert_eq!(varied["wall_room_repeats"], json!([[3, 3], [4, 3], [3, 3]]));
    assert_eq!(varied["doorways"].as_array().unwrap().len(), 2);
    let varied_path = verify_bundle(&fixture.project("varied"), monster, item);
    let varied_map = MapFile::load(&varied_path).unwrap();
    for door in varied["doorways"].as_array().unwrap() {
        let point = door["center"].as_array().unwrap();
        let center = (
            point[0].as_i64().unwrap() as i32,
            point[1].as_i64().unwrap() as i32,
        );
        crate::wood_doorway::inspect(&varied_map, center).unwrap();
    }
    let source_map = std::fs::read(&varied_path).unwrap();
    let source_db = std::fs::read(fixture.project("varied/System.db")).unwrap();
    let first = varied["doorways"][0]["center"].clone();
    let second = varied["doorways"][1]["center"].clone();
    let door_one = workspace.call("room_door_place", json!({"map":"project:varied/Map/GeneratedCave.map","database":"project:varied/System.db","center":first,"source_monster":monster,"output":"door-one"})).unwrap();
    assert_eq!(door_one["doors"], 1);
    assert_eq!(door_one["doorway"]["interactive"], true);
    let door_two = workspace.call("room_door_place", json!({"map":"project:door-one/Map/GeneratedCave.map","database":"project:door-one/System.db","center":second,"source_monster":monster,"output":"door-two"})).unwrap();
    assert_eq!(door_two["doors"], 2);
    let door_library = assets.join(crate::wood_door_art::LIBRARY);
    let original_door_art = std::fs::read(&door_library).unwrap();
    let private_door_art = fixture
        .project("door-two/Client")
        .join(crate::wood_door_art::LIBRARY);
    assert!(private_door_art.is_file());
    assert_eq!(
        std::path::Path::new(door_two["assets"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        fixture.project("door-two/Client").canonicalize().unwrap()
    );
    assert_ne!(std::fs::read(&private_door_art).unwrap(), original_door_art);
    assert_eq!(std::fs::read(&door_library).unwrap(), original_door_art);
    workspace.call("world_generate", json!({"source":"asset:D1006.map","mode":"chamber","width":80,"height":80,"seed":42,"monster_id":monster,"output":"generated-with-art","database":"project:door-two/System.db"})).unwrap();
    workspace.call("world_perimeter", json!({"nw":3,"ne":4,"seed":23,"monster_id":monster,"output":"perimeter-with-art","database":"project:door-two/System.db"})).unwrap();
    workspace.call("world_populate", json!({"map":"project:perimeter/Map/GeneratedCave.map","seed":23,"monster_id":monster,"output":"populated-with-art","database":"project:door-two/System.db"})).unwrap();
    for name in [
        "generated-with-art",
        "perimeter-with-art",
        "populated-with-art",
    ] {
        assert_eq!(
            std::fs::read(
                fixture
                    .project(name)
                    .join("Client")
                    .join(crate::wood_door_art::LIBRARY)
            )
            .unwrap(),
            std::fs::read(&private_door_art).unwrap()
        );
    }
    workspace.call("draft_variant", json!({"kind":"monster","source_id":chicken,"level":20,"name":"Private Art Chicken","output":"draft-with-art","database":"project:door-two/System.db"})).unwrap();
    assert_eq!(
        std::fs::read(
            fixture
                .project("draft-with-art/Client")
                .join(crate::wood_door_art::LIBRARY)
        )
        .unwrap(),
        std::fs::read(&private_door_art).unwrap()
    );
    let map_id = workspace
        .call(
            "content_search",
            json!({"kind":"maps","filter":"GeneratedCave","database":"project:door-two/System.db"}),
        )
        .unwrap()["entries"][0]["id"]
        .as_i64()
        .unwrap();
    let varied_layout = generate::read_layout(&varied_path).unwrap().unwrap().0;
    let spawned_with_art = workspace.call("world_spawn", json!({"map":"project:door-two/Map/GeneratedCave.map","map_id":map_id,"database":"project:door-two/System.db","monster_id":monster,"points":&varied_layout.spawns[..2],"count":3,"delay_seconds":45,"output":"spawn-with-art"})).unwrap();
    assert_eq!(
        std::fs::read(
            fixture
                .project("spawn-with-art/Client")
                .join(crate::wood_door_art::LIBRARY)
        )
        .unwrap(),
        std::fs::read(&private_door_art).unwrap()
    );
    assert_eq!(
        spawned_with_art["launch"]["ZIRCON_ASSETS"],
        spawned_with_art["assets"]
    );
    assert!(workspace.call("world_populate", json!({"map":"project:door-two/Map/GeneratedCave.map","seed":23,"monster_id":chicken,"output":"bad-door-database"})).is_err());
    assert!(!fixture.project("bad-door-database").exists());
    let final_map = fixture.project("door-two/Map/GeneratedCave.map");
    assert_eq!(std::fs::read(&varied_path).unwrap(), source_map);
    assert_eq!(std::fs::read(&final_map).unwrap(), source_map);
    assert_eq!(
        std::fs::read(fixture.project("door-two/System.db")).unwrap(),
        source_db
    );
    let saved_doors = mir_formats::room_door::RoomDoors::load(&final_map, &varied_map)
        .unwrap()
        .unwrap();
    assert_eq!(saved_doors.doors.len(), 2);
    workspace
        .call(
            "map_copy",
            json!({"map":"project:door-two/Map/GeneratedCave.map","output":"copied-door-map"}),
        )
        .unwrap();
    let copied_map = fixture.project("copied-door-map/Map.map");
    assert!(fixture
        .project("copied-door-map/Client")
        .join(crate::wood_door_art::LIBRARY)
        .is_file());
    workspace
        .call(
            "map_copy",
            json!({"map":"project:copied-door-map/Map.map","output":"copied-door-again"}),
        )
        .unwrap();
    assert!(fixture
        .project("copied-door-again/Client")
        .join(crate::wood_door_art::LIBRARY)
        .is_file());
    assert_eq!(
        mir_formats::room_door::RoomDoors::load(&copied_map, &MapFile::load(&copied_map).unwrap())
            .unwrap()
            .unwrap()
            .doors
            .len(),
        2
    );
    workspace.call("world_populate", json!({"map":"project:door-two/Map/GeneratedCave.map","database":"project:door-two/System.db","seed":23,"monster_id":monster,"output":"repopulated-door"})).unwrap();
    let repopulated_map = verify_bundle(&fixture.project("repopulated-door"), monster, item);
    assert!(fixture
        .project("repopulated-door/Client")
        .join(crate::wood_door_art::LIBRARY)
        .is_file());
    assert_eq!(
        mir_formats::room_door::RoomDoors::load(
            &repopulated_map,
            &MapFile::load(&repopulated_map).unwrap()
        )
        .unwrap()
        .unwrap()
        .doors
        .len(),
        2
    );
    assert!(workspace.call("map_collision", json!({"map":"project:door-two/Map/GeneratedCave.map","output":"blocked-door-map","points":[first],"walkable":false})).is_err());
    assert!(!fixture.project("blocked-door-map").exists());
    assert!(workspace.call("room_door_place", json!({"map":"project:door-two/Map/GeneratedCave.map","database":"project:door-two/System.db","center":first,"source_monster":monster,"output":"duplicate-door"})).is_err());
    assert!(!fixture.project("duplicate-door").exists());
    let wood_library = assets.join(crate::wood_branch::LIBRARY);
    let wood_before = std::fs::read(&wood_library).unwrap();
    let branch = workspace
        .call(
            "world_branch",
            json!({"arms":[2,5,8],"seed":23,
        "monster_id":monster,"output":"branch","database":"project:item/System.db"}),
        )
        .unwrap();
    assert_eq!(branch["arms"], json!([2, 5, 8]));
    assert_eq!(branch["centers"].as_array().unwrap().len(), 4);
    verify_bundle(&fixture.project("branch"), monster, item);
    let private_library = fixture
        .project("branch/Client")
        .join(crate::wood_branch::LIBRARY);
    assert!(private_library.is_file());
    assert_eq!(
        mir_formats::zl::ZlLibrary::open(&private_library)
            .unwrap()
            .len(),
        branch["first_new_image"].as_u64().unwrap() as usize + crate::wood_branch::NEW_IMAGES
    );
    assert_eq!(std::fs::read(&wood_library).unwrap(), wood_before);
    workspace.call("world_generate", json!({"source":"project:branch/Map/GeneratedCave.map","mode":"chamber","width":120,"height":120,"seed":42,"monster_id":monster,"output":"generated-from-private-map","database":"project:item/System.db"})).unwrap();
    assert_eq!(
        std::fs::read(
            fixture
                .project("generated-from-private-map/Client")
                .join(crate::wood_branch::LIBRARY)
        )
        .unwrap(),
        std::fs::read(&private_library).unwrap()
    );
    workspace
        .call(
            "map_copy",
            json!({"map":"project:branch/Map/GeneratedCave.map","output":"branch-copy"}),
        )
        .unwrap();
    assert!(fixture
        .project("branch-copy/Client")
        .join(crate::wood_branch::LIBRARY)
        .is_file());
    assert!(workspace.call("world_populate", json!({"map":"project:branch-copy/Map.map","database":"project:branch/System.db","seed":23,"monster_id":monster,"output":"ambiguous-branch-art"})).is_err());
    assert!(!fixture.project("ambiguous-branch-art").exists());
    workspace.call("world_populate", json!({"map":"project:branch-copy/Map.map","database":"project:branch/System.db","assets":"project:branch-copy/Client","seed":23,"monster_id":monster,"output":"branch-copy-populated"})).unwrap();
    assert!(fixture
        .project("branch-copy-populated/Client")
        .join(crate::wood_branch::LIBRARY)
        .is_file());
    workspace.call("world_branch", json!({"seed":23,"monster_id":monster,"output":"branch-inherited","database":"project:door-two/System.db"})).unwrap();
    assert_eq!(
        std::fs::read(
            fixture
                .project("branch-inherited/Client")
                .join(crate::wood_door_art::LIBRARY)
        )
        .unwrap(),
        std::fs::read(&private_door_art).unwrap()
    );
    let y_branch = workspace.call("world_branch", json!({"shape":"y","organic":true,"arms":[3,4,5],"seed":23,"monster_id":monster,"output":"y-branch","database":"project:item/System.db"})).unwrap();
    assert_eq!(y_branch["centers"].as_array().unwrap().len(), 4);
    verify_bundle(&fixture.project("y-branch"), monster, item);
    let graph = workspace.call("world_branch", json!({"nodes":[[0,0],[1,0],[2,0],[0,1],[1,1],[2,1]],"links":[[0,1],[1,2],[2,5],[5,4],[4,3],[3,0],[1,4]],"organic":true,"seed":23,"monster_id":monster,"output":"graph","database":"project:item/System.db"})).unwrap();
    assert_eq!(graph["centers"].as_array().unwrap().len(), 6);
    verify_bundle(&fixture.project("graph"), monster, item);
    assert!(workspace
        .call(
            "world_branch",
            json!({"arms":[0,5,5],"seed":23,
        "monster_id":monster,"output":"bad-branch","database":"project:item/System.db"})
        )
        .is_err());
    assert!(!fixture.project("bad-branch").exists());
    for (repeats, output) in [
        (json!([[3, 3]]), "one-varied"),
        (json!([[4, 4], [10, 10]]), "large-varied"),
        (json!([[3, 3], [3, 3]]), "mixed-uniform"),
    ] {
        let mut arguments = json!({"repeats":repeats,"seed":23,"monster_id":monster,"output":output,"database":"project:item/System.db"});
        if output == "mixed-uniform" {
            arguments["nw"] = json!(3);
        }
        assert!(workspace.call("world_perimeter", arguments).is_err());
        assert!(!fixture.project(output).exists());
    }
    for (rooms, nw, output) in [
        (0, 3, "zerorooms"),
        (4, 3, "fourrooms"),
        (2, 10, "oversizedconnected"),
    ] {
        assert!(workspace.call("world_perimeter", json!({"rooms":rooms,"nw":nw,"ne":4,"seed":23,"monster_id":monster,"output":output,"database":"project:item/System.db"})).is_err());
        assert!(!fixture.project(output).exists());
    }
    let database_before = std::fs::read(fixture.project("perimeter/System.db")).unwrap();
    let map_before = std::fs::read(&perimeter_map_path).unwrap();
    let perimeter_db = MirDb::parse(&database_before).unwrap();
    let maps = perimeter_db.collection("MapInfo").unwrap();
    let file_name = perimeter_map_path.file_stem().unwrap().to_str().unwrap();
    let map_id = maps
        .records
        .iter()
        .find(|row| maps.str_or(row, "FileName", "") == file_name)
        .map(|row| maps.index(row))
        .unwrap();
    let map_reference = format!(
        "project:perimeter/Map/{}",
        perimeter_map_path.file_name().unwrap().to_str().unwrap()
    );
    let points = vec![perimeter_layout.spawns[0], perimeter_layout.spawns[1]];
    let spawn_args = json!({"map":map_reference,"map_id":map_id,"database":"project:perimeter/System.db","monster_id":monster,"points":points,"count":3,"delay_seconds":45,"output":"spawn"});
    let spawned = workspace.call("world_spawn", spawn_args.clone()).unwrap();
    let spawn_db = MirDb::load(fixture.project("spawn/System.db")).unwrap();
    let regions = spawn_db.collection("MapRegion").unwrap();
    let region = regions
        .records
        .iter()
        .find(|row| i64::from(regions.index(row)) == spawned["region_id"].as_i64().unwrap())
        .unwrap();
    assert_eq!(regions.int_or(region, "Map", -1), i64::from(map_id));
    assert_eq!(
        regions.get(region, "PointRegion"),
        Some(&mir_formats::mirdb::Value::PointArray(Some(points.clone())))
    );
    let spawns = spawn_db.collection("RespawnInfo").unwrap();
    let row = spawns
        .records
        .iter()
        .find(|row| i64::from(spawns.index(row)) == spawned["spawn_id"].as_i64().unwrap())
        .unwrap();
    assert_eq!(spawns.int_or(row, "Monster", -1), i64::from(monster));
    assert_eq!(spawns.int_or(row, "Count", -1), 3);
    assert_eq!(spawns.int_or(row, "Delay", -1), 45);
    assert_eq!(
        std::fs::read_dir(fixture.project("spawn")).unwrap().count(),
        1
    );
    for (field, value) in [
        ("points", json!([perimeter_layout.start])),
        ("map_id", json!(-1)),
        ("monster_id", json!(-1)),
        ("count", json!(0)),
        ("delay_seconds", json!(3601)),
    ] {
        let mut invalid = spawn_args.clone();
        invalid[field] = value;
        invalid["output"] = json!("invalidspawn");
        assert!(workspace.call("world_spawn", invalid).is_err());
        assert!(!fixture.project("invalidspawn").exists());
    }
    // Original database regions must protect safe cells even without sidecars.
    std::fs::create_dir(fixture.project("baremap")).unwrap();
    std::fs::write(
        fixture.project(&format!("baremap/{file_name}.map")),
        &map_before,
    )
    .unwrap();
    let mut invalid = spawn_args;
    invalid["map"] = json!(format!("project:baremap/{file_name}.map"));
    invalid["points"] = json!([perimeter_layout.start]);
    invalid["output"] = json!("baresafespawn");
    assert!(workspace.call("world_spawn", invalid).is_err());
    assert!(!fixture.project("baresafespawn").exists());
    assert_eq!(
        std::fs::read(fixture.project("perimeter/System.db")).unwrap(),
        database_before
    );
    assert_eq!(std::fs::read(&perimeter_map_path).unwrap(), map_before);
    for x in 0..i32::from(perimeter_map.width) {
        assert!(!perimeter_map.is_walkable(x, 0));
        assert!(!perimeter_map.is_walkable(x, i32::from(perimeter_map.height) - 1));
    }
    for y in 0..i32::from(perimeter_map.height) {
        assert!(!perimeter_map.is_walkable(0, y));
        assert!(!perimeter_map.is_walkable(i32::from(perimeter_map.width) - 1, y));
    }
    assert!(workspace.call("world_perimeter", json!({"nw":2,"ne":4,"seed":23,"monster_id":monster,"output":"badperimeter","database":"project:item/System.db"})).is_err());
    assert!(!fixture.project("badperimeter").exists());
    assert_eq!(
        std::fs::read(assets.join("Map/D1008.map")).unwrap(),
        perimeter_source
    );
    workspace
        .call(
            "map_create",
            json!({"output":"kit","width":64,"height":64,"kit":"wood-north","x":14,"y":2}),
        )
        .unwrap();
    assert!(
        MapFile::load(fixture.project("kit/Map.map"))
            .unwrap()
            .cell(16, 6)
            .unwrap()
            .middle_image
            > 0
    );
    let kit_path = fixture.project("kit/Map.map");
    let kit_map = MapFile::load(&kit_path).unwrap();
    let navigation = Navigation::new(
        &kit_map,
        vec![mir_formats::navigation::Portal {
            position: (0, 0),
            destination: (63, 63),
        }],
    )
    .unwrap();
    navigation.save(&kit_path, &kit_map).unwrap();
    workspace
        .call(
            "map_stamp",
            json!({"map":"project:kit/Map.map","output":"stamped","kit":"wood-north","x":14,"y":2}),
        )
        .unwrap();
    let stamped_path = fixture.project("stamped/Map.map");
    let stamped = MapFile::load(&stamped_path).unwrap();
    assert_eq!(stamped.to_bytes(), kit_map.to_bytes());
    assert_eq!(
        Navigation::load(&stamped_path, &stamped).unwrap().unwrap(),
        navigation
    );
    assert!(workspace
        .call(
            "world_populate",
            json!({"map":"asset:D1006.map","seed":0,"monster_id":-1,"output":"invalidmonster"})
        )
        .is_err());
    assert!(!fixture.project("invalidmonster").exists());
    assert_eq!(std::fs::read(database).unwrap(), source_bytes);
    assert_eq!(
        std::fs::read(assets.join("Map/D1006.map")).unwrap(),
        map_bytes
    );
}

#[test]
fn wood_wall_junction_kit_preserves_authored_cells_and_collision() {
    let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
        return;
    };
    let fixture = Fixture::new();
    let workspace = Workspace::open(
        &fixture.0.join("project"),
        &assets,
        &assets.join("../Database/System.db"),
    )
    .unwrap();
    workspace
        .call(
            "map_create",
            json!({"output":"blank","width":128,"height":128}),
        )
        .unwrap();
    workspace.call("map_stamp", json!({"map":"project:blank/Map.map","output":"threshold","kit":"wood-wall-junction","x":20,"y":20})).unwrap();
    let placed = MapFile::load(fixture.project("threshold/Map.map")).unwrap();
    let piece = crate::wood_wall_entry::entry(&assets).unwrap();
    let mut floor = 0;
    let mut wall = 0;
    for &(x, y, cell) in piece.cells() {
        assert_eq!(placed.cell(x + 20, y + 20), Some(&cell));
        if cell.is_walkable() {
            floor += 1;
        } else {
            wall += 1;
        }
    }
    assert!(floor > 0 && wall > 0);
    workspace.call("map_create", json!({"output":"direct","width":128,"height":128,"kit":"wood-wall-junction","x":20,"y":20})).unwrap();
    assert_eq!(
        std::fs::read(fixture.project("direct/Map.map")).unwrap(),
        placed.to_bytes()
    );
}

#[test]
fn kit_tools_reject_odd_origins_instead_of_silently_snapping() {
    let fixture = Fixture::new();
    let workspace = fixture.workspace();
    workspace
        .call(
            "map_create",
            json!({"output":"base","width":32,"height":32}),
        )
        .unwrap();
    for (tool, args) in [
        (
            "map_create",
            json!({"output":"odd_create","width":32,"height":32,"kit":"wood-north","x":1,"y":0}),
        ),
        (
            "map_stamp",
            json!({"map":"project:base/Map.map","output":"odd_stamp","kit":"wood-north","x":0,"y":1}),
        ),
    ] {
        assert!(workspace
            .call(tool, args)
            .unwrap_err()
            .to_string()
            .contains("must be even"));
    }
    assert!(!fixture.project("odd_create").exists());
    assert!(!fixture.project("odd_stamp").exists());
}
