//! Asset-backed public MCP facade coverage; no production assets are edited.
use crate::{mcp_authoring::Workspace, sprite_project};
use mir_formats::{mirdb::MirDb, zl::ZlLibrary};
use serde_json::json;
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn sprite_facade_revisions_publish_new_appearance_and_preserve_sources() {
    let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
        return;
    };
    let database = assets.join("../Database/System.db");
    let original_db = std::fs::read(&database).unwrap();
    let item_libraries: Vec<_> = ["Data/StoreItems.Zl", "Data/Inventory.Zl", "Data/Ground.Zl"]
        .into_iter()
        .map(|name| {
            let path = crate::preview::resolve_case_insensitive(&assets, name).unwrap();
            let hash = sprite_project::file_checksum(&path).unwrap();
            (path, hash)
        })
        .collect();
    let db = MirDb::load(&database).unwrap();
    let items = db.collection("ItemInfo").unwrap();
    let sword = items
        .records
        .iter()
        .find(|r| items.str_or(r, "ItemName", "") == "WoodenSword")
        .or_else(|| {
            items
                .records
                .iter()
                .find(|r| items.str_or(r, "ItemName", "").contains("Sword"))
        })
        .unwrap();
    let source_id = items.index(sword);
    let source_image = items.int_or(sword, "Image", 0);
    let fixture = Fixture(std::env::temp_dir().join(format!(
            "zircon-sprite-facade-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    let workspace = Workspace::open(&fixture.0, &assets, &database).unwrap();
    workspace
        .call(
            "sprite_export_project",
            json!({"kind":"item","source_image":source_image,"output":"source"}),
        )
        .unwrap();
    let manifest = sprite_project::load_manifest(&fixture.0.join("source")).unwrap();
    let original_library =
        sprite_project::file_checksum(&PathBuf::from(&manifest.source_path)).unwrap();
    let original_project = std::fs::read(fixture.0.join("source/project.json")).unwrap();
    workspace
        .call(
            "sprite_validate_project",
            json!({"project":"project:source/project.json"}),
        )
        .unwrap();
    let art = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/authoring-demo/jade-beetle-talisman.png");
    std::fs::copy(art, fixture.0.join("art.png")).unwrap();
    workspace.call("sprite_import_image",json!({"project":"project:source/project.json","image":"project:art.png","slot":0,"layer":"image","fit":true,"binary_alpha":true,"trim_transparent":true,"canvas":[32,32],"offsets":[-16,-16],"output":"edited"})).unwrap();
    let changed = workspace
        .call(
            "sprite_validate_project",
            json!({"project":"project:edited/project.json"}),
        )
        .unwrap();
    assert_eq!(changed["changed_slots"], 1);
    let edited = sprite_project::load_manifest(&fixture.0.join("edited")).unwrap();
    let frame = &edited.slots[0];
    assert_eq!(
        (
            frame.image.as_ref().unwrap().width,
            frame.image.as_ref().unwrap().height
        ),
        (32, 32)
    );
    assert_eq!((frame.offset_x, frame.offset_y), (-16, -16));
    let (_, _, pixels) = sprite_project::read_png(
        &fixture
            .0
            .join("edited")
            .join(&frame.image.as_ref().unwrap().path),
    )
    .unwrap();
    let visible_rows = (0..32)
        .filter(|y| (0..32).any(|x| pixels[(y * 32 + x) * 4 + 3] > 0))
        .count();
    assert!(
        visible_rows >= 28,
        "Visible artwork should fill the requested icon height"
    );
    assert!(workspace.call("sprite_import_image",json!({"project":"project:source/project.json","image":"project:art.png","slot":0,"layer":"image","fit":true,"canvas":[32,32],"output":"missing-offsets"})).is_err());
    assert!(!fixture.0.join("missing-offsets").exists());

    workspace
        .call(
            "sprite_preview_sheet",
            json!({"project":"project:edited/project.json","output":"preview"}),
        )
        .unwrap();
    assert!(fixture.0.join("preview/preview.png").is_file());
    let built = workspace
        .call(
            "sprite_build_pack",
            json!({"project":"project:edited/project.json","output":"built"}),
        )
        .unwrap();
    assert_eq!(
        ZlLibrary::open(built["library"].as_str().unwrap())
            .unwrap()
            .len(),
        1
    );
    let published = workspace.call("sprite_publish",json!({"project":"project:edited/project.json","kind":"item","source_id":source_id,"name":"Sprite Facade Talisman","output":"published"})).unwrap();
    assert_ne!(published["image"].as_i64().unwrap(), source_image);
    let next = MirDb::load(published["database"].as_str().unwrap()).unwrap();
    let next_items = next.collection("ItemInfo").unwrap();
    assert_eq!(next_items.records.len(), items.records.len() + 1);
    for row in &items.records {
        let preserved = next_items
            .records
            .iter()
            .find(|r| next_items.index(r) == items.index(row))
            .unwrap();
        assert_eq!(next_items.int(preserved, "Image"), items.int(row, "Image"));
    }
    assert_eq!(std::fs::read(database).unwrap(), original_db);
    for (path, original) in item_libraries {
        assert_eq!(sprite_project::file_checksum(&path).unwrap(), original);
    }
    assert_eq!(
        std::fs::read(fixture.0.join("source/project.json")).unwrap(),
        original_project
    );
    assert_eq!(
        sprite_project::file_checksum(&PathBuf::from(&manifest.source_path)).unwrap(),
        original_library
    );
    for (tool, args, output) in [
        (
            "sprite_import_image",
            json!({"project":"project:source/project.json","image":"project:../art.png","slot":0,"layer":"image","output":"escape"}),
            "escape",
        ),
        (
            "sprite_import_image",
            json!({"project":"project:source/project.json","image":"project:art.png","slot":1,"layer":"image","fit":true,"binary_alpha":true,"output":"badslot"}),
            "badslot",
        ),
        (
            "sprite_publish",
            json!({"project":"project:edited/project.json","kind":"monster","source_id":source_id,"name":"Wrong","output":"wrongkind"}),
            "wrongkind",
        ),
    ] {
        assert!(workspace.call(tool, args).is_err());
        assert!(!fixture.0.join(output).exists());
    }
    assert!(workspace
        .call(
            "sprite_export_project",
            json!({"kind":"item","source_image":source_image,"output":"source"})
        )
        .is_err());
    assert_eq!(
        std::fs::read(fixture.0.join("source/project.json")).unwrap(),
        original_project
    );
}

#[test]
fn sprite_facade_incomplete_monster_contract_refuses_publishable_build() {
    let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
        return;
    };
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "zircon-sprite-monster-facade-{}",
        std::process::id()
    )));
    let workspace =
        Workspace::open(&fixture.0, &assets, &assets.join("../Database/System.db")).unwrap();
    workspace
        .call(
            "sprite_reference",
            json!({"kind":"monster","source_image":33,"output":"oma"}),
        )
        .unwrap();
    let error = workspace
        .call(
            "sprite_validate_project",
            json!({"project":"project:oma/project.json"}),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("Missing required animation image slots"));
    assert!(workspace
        .call(
            "sprite_build_pack",
            json!({"project":"project:oma/project.json","output":"built"})
        )
        .is_err());
    assert!(!fixture.0.join("built").exists());
    // The raw archive remains inspectable even when runtime validation refuses it.
    let checked = sprite_project::validate(&fixture.0.join("oma")).unwrap();
    assert_eq!(checked.slots, 1000);
    assert_eq!(checked.changed_slots, 0);
}
