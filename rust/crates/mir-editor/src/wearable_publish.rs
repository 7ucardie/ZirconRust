//! Publish a complete set of wearable body families as one private revision.
use crate::{sprite_authoring, sprite_contract, sprite_project, sprite_publish};
use anyhow::{ensure, Context, Result};
use mir_formats::{
    mirdb::{MirDb, Value},
    wearable_animation as animation,
};
use serde_json::{json, Value as Json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub fn families(db: &MirDb, source_id: i32) -> Result<(u16, u16, Vec<&'static str>)> {
    let c = db.collection("ItemInfo").context("Missing ItemInfo")?;
    let row = c
        .records
        .iter()
        .find(|r| c.index(r) == source_id)
        .context("Source armour missing")?;
    ensure!(
        c.int(row, "ItemType") == Some(3),
        "Wearable publication requires an armour item"
    );
    let shape = u16::try_from(c.int(row, "Shape").context("Missing Shape")?)?;
    let image = u16::try_from(c.int(row, "Image").context("Missing Image")?)?;
    let classes = c.int_or(row, "RequiredClass", 0);
    let genders = c.int_or(row, "RequiredGender", 0);
    ensure!(
        classes > 0 && classes & !15 == 0 && genders > 0 && genders & !3 == 0,
        "Armour needs explicit supported class and gender flags"
    );
    let mut required = Vec::new();
    for (kind, class, gender) in [
        ("armour-male", 7, 1),
        ("armour-female", 7, 2),
        ("armour-assassin-male", 8, 1),
        ("armour-assassin-female", 8, 2),
    ] {
        if classes & class != 0 && genders & gender != 0 {
            required.push(kind);
        }
    }
    Ok((shape, image, required))
}

pub fn equipment_families(db: &MirDb, source_id: i32) -> Result<(u8, u16, u16, Vec<&'static str>)> {
    let c = db.collection("ItemInfo").context("Missing ItemInfo")?;
    let row = c
        .records
        .iter()
        .find(|r| c.index(r) == source_id)
        .context("Source equipment missing")?;
    let item_type = u8::try_from(c.int(row, "ItemType").context("Missing ItemType")?)?;
    ensure!(
        matches!(item_type, 2 | 5 | 27),
        "Equipment publication requires a weapon, helmet or shield"
    );
    let shape = u16::try_from(c.int(row, "Shape").context("Missing Shape")?)?;
    let image = u16::try_from(c.int(row, "Image").context("Missing Image")?)?;
    let classes = c.int_or(row, "RequiredClass", 0);
    let genders = c.int_or(row, "RequiredGender", 0);
    ensure!(
        classes > 0 && classes & !15 == 0 && genders > 0 && genders & !3 == 0,
        "Equipment needs explicit supported class and gender flags"
    );
    let mut required = Vec::new();
    for (kind, class, gender) in [
        ("weapon-male", 15, 1),
        ("weapon-female", 15, 2),
        ("helmet-male", 7, 1),
        ("helmet-female", 7, 2),
        ("helmet-assassin-male", 8, 1),
        ("helmet-assassin-female", 8, 2),
        ("shield-male", 7, 1),
        ("shield-female", 7, 2),
        ("shield-assassin-male", 8, 1),
        ("shield-assassin-female", 8, 2),
    ] {
        if crate::sprite_contract::equipment_family(kind)
            .is_some_and(|family| family.0 == item_type)
            && classes & class != 0
            && genders & gender != 0
        {
            required.push(kind);
        }
    }
    ensure!(
        !required.is_empty(),
        "No supported equipment family for this item"
    );
    Ok((item_type, shape, image, required))
}

fn shield_shape_free(shape: u16, required: &[&str], used: &BTreeSet<i64>) -> bool {
    let targets: Vec<_> = required
        .iter()
        .filter_map(|kind| {
            let (_, female, assassin) = sprite_contract::equipment_family(kind)?;
            let library = animation::shield_library(shape, female)?;
            let stride = animation::shape_stride(assassin);
            let start = u32::from(shape % 10) * stride;
            Some((library, start, start + stride))
        })
        .collect();
    let overlaps = |a: &(u16, u32, u32), b: &(u16, u32, u32)| a.0 == b.0 && a.1 < b.2 && b.1 < a.2;
    // Distinct normal/Assassin projects cannot overwrite each other.
    if targets
        .iter()
        .enumerate()
        .any(|(i, a)| targets[i + 1..].iter().any(|b| overlaps(a, b)))
    {
        return false;
    }
    for &old in used {
        let Ok(old) = u16::try_from(old) else {
            continue;
        };
        for female in [false, true] {
            let Some(library) = animation::shield_library(old, female) else {
                continue;
            };
            for assassin in [false, true] {
                let stride = animation::shape_stride(assassin);
                let start = u32::from(old % 10) * stride;
                let occupied = (library, start, start + stride);
                if targets.iter().any(|target| overlaps(target, &occupied)) {
                    return false;
                }
            }
        }
    }
    true
}

pub fn publish(
    assets: &Path,
    db: &MirDb,
    projects: &[PathBuf],
    source_id: i32,
    name: &str,
    output: &Path,
) -> Result<Json> {
    let assets = assets.canonicalize()?;
    let parent = output
        .parent()
        .context("Output parent missing")?
        .canonicalize()?;
    ensure!(
        !parent.starts_with(&assets),
        "Publish outside source assets"
    );
    ensure!(
        std::fs::symlink_metadata(output).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "Output must be new"
    );
    let (source_shape, image, required) = families(db, source_id)?;
    ensure!(
        projects.len() == required.len(),
        "Provide exactly these body families: {required:?}"
    );
    let mut sources = BTreeMap::new();
    for project in projects {
        let identity = sprite_authoring::identity(project)?;
        ensure!(
            identity.image == source_shape && required.contains(&identity.kind.as_str()),
            "Project differs from source armour shape/body family"
        );
        ensure!(
            !sources.contains_key(&identity.kind),
            "Duplicate body family"
        );
        sprite_authoring::validate(&assets, project)?;
        sources.insert(identity.kind, project);
    }
    let c = db.collection("ItemInfo").unwrap();
    // Only armour uses this appearance namespace; weapon shapes are independent.
    let used: BTreeSet<_> = c
        .records
        .iter()
        .filter(|r| c.int(r, "ItemType") == Some(3))
        .filter_map(|r| c.int(r, "Shape"))
        .collect();
    let shape = (1..231u16)
        .find(|shape| {
            !used.contains(&i64::from(*shape))
                && required.iter().all(|kind| {
                    let (female, assassin) = sprite_contract::armour_family(kind).unwrap();
                    animation::armour_library(*shape, female, assassin)
                        .is_some_and(|lib| sprite_publish::source_library(&assets, lib).is_ok())
                })
        })
        .context("No unused mapped armour shape remains")?;
    let (mut next, created_id) = sprite_publish::clone_record(
        db,
        &sprite_publish::Request {
            kind: sprite_publish::Kind::Item,
            source_id,
            name: name.into(),
        },
        image,
    )?;
    let c = next.collection_mut("ItemInfo").unwrap();
    let index = c
        .records
        .iter()
        .position(|r| c.index(r) == created_id)
        .unwrap();
    let mut row = c.records[index].clone();
    ensure!(
        c.set(&mut row, "Shape", Value::Int(i64::from(shape))),
        "Cannot update armour Shape"
    );
    c.records[index] = row;
    std::fs::create_dir(output)?;
    let result = (|| -> Result<Json> {
        let mut overrides = BTreeMap::new();
        for (kind, project) in sources {
            let (female, assassin) = sprite_contract::armour_family(&kind).unwrap();
            let built_dir = output.join(format!("build-{kind}"));
            let report = sprite_project::build(project, &built_dir)?;
            let built =
                sprite_publish::open_bounded(&report.library.context("Missing built library")?)?;
            let source = sprite_publish::source_library(
                &assets,
                animation::armour_library(shape, female, assassin).unwrap(),
            )?;
            let original = sprite_publish::open_bounded(&source)?;
            let start = usize::from(shape % 11) * animation::shape_stride(assassin) as usize;
            overrides.insert(
                source.strip_prefix(&assets)?.to_path_buf(),
                sprite_publish::replacement(&original, &built, start)?,
            );
        }
        let client = output.join("Client");
        sprite_publish::asset_view(&assets, &assets, &client, &overrides)?;
        for (relative, bytes) in &overrides {
            let path = client.join(relative);
            ensure!(
                std::fs::symlink_metadata(&path)
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
                "Override destination exists"
            );
            std::fs::write(path, bytes)?;
        }
        let database = output.join("System.db");
        next.save(&database)?;
        let report = json!({"directory":output,"assets":client,"database":database,"created_id":created_id,"shape":shape,"image":image,"body_families":required,"libraries":overrides.keys().collect::<Vec<_>>(),"scope":"New wearable shape; original icons, item restrictions, equipment layers and existing appearances preserved."});
        std::fs::write(
            output.join("wearable-publish.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        Ok(report)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(output);
    }
    result
}

/// Publish equipped weapon, helmet or shield frames to a new private shape.
pub fn publish_equipment(
    assets: &Path,
    db: &MirDb,
    projects: &[PathBuf],
    source_id: i32,
    name: &str,
    output: &Path,
) -> Result<Json> {
    let assets = assets.canonicalize()?;
    let parent = output
        .parent()
        .context("Output parent missing")?
        .canonicalize()?;
    ensure!(
        !parent.starts_with(&assets),
        "Publish outside source assets"
    );
    ensure!(!output.exists(), "Output must be new");
    let (item_type, source_shape, image, required) = equipment_families(db, source_id)?;
    ensure!(
        projects.len() == required.len(),
        "Provide exactly these equipment families: {required:?}"
    );
    let mut sources = BTreeMap::new();
    for project in projects {
        let identity = sprite_authoring::identity(project)?;
        ensure!(
            identity.image == source_shape && required.contains(&identity.kind.as_str()),
            "Project differs from source equipment shape/family"
        );
        ensure!(
            !sources.contains_key(&identity.kind),
            "Duplicate equipment family"
        );
        sprite_authoring::validate(&assets, project)?;
        sources.insert(identity.kind, project);
    }
    let c = db.collection("ItemInfo").unwrap();
    let used: BTreeSet<_> = c
        .records
        .iter()
        .filter(|r| c.int(r, "ItemType") == Some(i64::from(item_type)))
        .filter_map(|r| c.int(r, "Shape"))
        .collect();
    let (first, end) = match item_type {
        5 => (1u16, 231u16),
        27 => (0, 20),
        _ => (0, 1260),
    };
    let shape = (first..end)
        .filter(|shape| !used.contains(&i64::from(*shape)))
        .find(|shape| {
            (item_type != 27 || shield_shape_free(*shape, &required, &used))
                && required.iter().all(|kind| {
                    let (_, female, assassin) = sprite_contract::equipment_family(kind).unwrap();
                    let library = match item_type {
                        2 => animation::weapon_library(*shape, female),
                        5 => animation::helmet_library(*shape, female, assassin),
                        _ => animation::shield_library(*shape, female),
                    };
                    library.is_some_and(|lib| sprite_publish::source_library(&assets, lib).is_ok())
                })
        })
        .context("No unused mapped equipment shape remains")?;
    let (mut next, created_id) = sprite_publish::clone_record(
        db,
        &sprite_publish::Request {
            kind: sprite_publish::Kind::Item,
            source_id,
            name: name.into(),
        },
        image,
    )?;
    let c = next.collection_mut("ItemInfo").unwrap();
    let index = c
        .records
        .iter()
        .position(|r| c.index(r) == created_id)
        .unwrap();
    let mut row = c.records[index].clone();
    ensure!(
        c.set(&mut row, "Shape", Value::Int(i64::from(shape))),
        "Cannot update equipment Shape"
    );
    c.records[index] = row;
    std::fs::create_dir(output)?;
    let result = (|| -> Result<Json> {
        let mut overrides = BTreeMap::new();
        for (kind, project) in sources {
            let (_, female, assassin) = sprite_contract::equipment_family(&kind).unwrap();
            let library = match item_type {
                2 => animation::weapon_library(shape, female),
                5 => animation::helmet_library(shape, female, assassin),
                _ => animation::shield_library(shape, female),
            }
            .unwrap();
            let built_dir = output.join(format!("build-{kind}"));
            let report = sprite_project::build(project, &built_dir)?;
            let built =
                sprite_publish::open_bounded(&report.library.context("Missing built library")?)?;
            let source = sprite_publish::source_library(&assets, library)?;
            let relative = source.strip_prefix(&assets)?.to_path_buf();
            // Shield normal/Assassin families can share one library; apply both ranges.
            let previous = output.join(".equipment-merge.zl");
            if let Some(bytes) = overrides.get(&relative) {
                std::fs::write(&previous, bytes)?;
            }
            let original = sprite_publish::open_bounded(if overrides.contains_key(&relative) {
                &previous
            } else {
                &source
            })?;
            let stride = if item_type == 2 {
                5000
            } else {
                animation::shape_stride(assassin) as usize
            };
            let local = if item_type == 5 {
                (shape - 1) % 10
            } else {
                shape % 10
            };
            overrides.insert(
                relative,
                sprite_publish::replacement(&original, &built, usize::from(local) * stride)?,
            );
        }
        let _ = std::fs::remove_file(output.join(".equipment-merge.zl"));
        let client = output.join("Client");
        sprite_publish::asset_view(&assets, &assets, &client, &overrides)?;
        for (relative, bytes) in &overrides {
            std::fs::write(client.join(relative), bytes)?;
        }
        let database = output.join("System.db");
        next.save(&database)?;
        Ok(
            json!({"directory":output,"assets":client,"database":database,
            "created_id":created_id,"shape":shape,"image":image,
            "equipment_families":required,"libraries":overrides.keys().collect::<Vec<_>>() }),
        )
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(output);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use mir_formats::zl::ZlLibrary;

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn equipped_weapon_helmet_shield_publish_distinct_private_shapes() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        let database = assets.join("../Database/System.db");
        let source_bytes = std::fs::read(&database).unwrap();
        for (item_type, kind) in [(2, "weapon-male"), (5, "helmet-male"), (27, "shield-male")] {
            let mut db = MirDb::load(&database).unwrap();
            let c = db.collection_mut("ItemInfo").unwrap();
            let (index, shape) = c
                .records
                .iter()
                .enumerate()
                .find_map(|(index, row)| {
                    if c.int(row, "ItemType") != Some(item_type) {
                        return None;
                    }
                    let shape = u16::try_from(c.int(row, "Shape")?).ok()?;
                    let contract = sprite_contract::equipment(kind, shape, &assets).ok()?;
                    contract
                        .missing_required
                        .is_empty()
                        .then_some((index, shape))
                })
                .unwrap_or_else(|| panic!("installed {kind} with a complete animation contract"));
            let mut source = c.records[index].clone();
            let source_id = c.index(&source);
            assert!(c.set(&mut source, "RequiredClass", Value::Int(1)));
            assert!(c.set(&mut source, "RequiredGender", Value::Int(1)));
            c.records[index] = source;
            let fixture = Fixture(std::env::temp_dir().join(
                format!("zircon-{kind}-publish-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
            ));
            std::fs::create_dir(&fixture.0).unwrap();
            let project = fixture.0.join(kind);
            sprite_authoring::export(
                &assets,
                &sprite_authoring::Identity {
                    kind: kind.into(),
                    image: shape,
                },
                &project,
            )
            .unwrap();
            let project = if item_type == 2 {
                let edited = fixture.0.join("edited-weapon");
                let report =
                    sprite_project::tint(&project, &edited, "image", [170, 235, 255]).unwrap();
                assert!(report.changed_slots > 0);
                edited
            } else {
                project
            };
            let published = publish_equipment(
                &assets,
                &db,
                &[project],
                source_id,
                &format!("Authored {kind}"),
                &fixture.0.join("published"),
            )
            .unwrap();
            assert_ne!(published["shape"], shape);
            assert!(fixture.0.join("published/System.db").is_file());
            assert_eq!(std::fs::read(&database).unwrap(), source_bytes);
        }
    }
    #[test]
    fn shield_allocation_respects_normal_and_assassin_ranges() {
        let used = BTreeSet::from([0, 2]);
        assert!(!shield_shape_free(1, &["shield-male"], &used));
        assert!(!shield_shape_free(
            1,
            &["shield-male", "shield-assassin-male"],
            &BTreeSet::new()
        ));
        assert!(shield_shape_free(
            3,
            &["shield-male", "shield-assassin-male"],
            &BTreeSet::new()
        ));
    }
    #[test]
    fn complete_gender_set_publishes_new_shape_and_rejects_incomplete_or_wrong_sources() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        let database = assets.join("../Database/System.db");
        let original_database = std::fs::read(&database).unwrap();
        let mut db = MirDb::load(&database).unwrap();
        let c = db.collection_mut("ItemInfo").unwrap();
        let source_index = c
            .records
            .iter()
            .position(|r| {
                c.int(r, "ItemType") == Some(3)
                    && c.int(r, "Shape")
                        .is_some_and(|shape| (0..11).contains(&shape))
            })
            .unwrap();
        let mut source = c.records[source_index].clone();
        let source_id = c.index(&source);
        let source_shape = u16::try_from(c.int(&source, "Shape").unwrap()).unwrap();
        // Test one legitimate shared normal-class, both-gender contract.
        assert!(c.set(&mut source, "RequiredClass", Value::Int(7)));
        assert!(c.set(&mut source, "RequiredGender", Value::Int(3)));
        c.records[source_index] = source;
        assert_eq!(
            families(&db, source_id).unwrap().2,
            vec!["armour-male", "armour-female"]
        );
        let fixture = Fixture(std::env::temp_dir().join(format!(
                "zircon-wearable-publish-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir(&fixture.0).unwrap();
        let projects: Vec<_> = ["armour-male", "armour-female"]
            .into_iter()
            .map(|kind| {
                let output = fixture.0.join(kind);
                sprite_authoring::export(
                    &assets,
                    &sprite_authoring::Identity {
                        kind: kind.into(),
                        image: source_shape,
                    },
                    &output,
                )
                .unwrap();
                output
            })
            .collect();
        let source_bytes = db.to_bytes().unwrap();
        let rejected = fixture.0.join("rejected");
        assert!(publish(
            &assets,
            &db,
            &projects[..1],
            source_id,
            "Incomplete",
            &rejected
        )
        .is_err());
        assert!(!rejected.exists());
        assert!(publish(
            &assets,
            &db,
            &[projects[0].clone(), projects[0].clone()],
            source_id,
            "Duplicate",
            &rejected
        )
        .is_err());
        assert!(!rejected.exists());
        let identity_path = projects[1].join("identity.json");
        let identity = std::fs::read(&identity_path).unwrap();
        std::fs::write(
            &identity_path,
            serde_json::to_vec(&json!({"kind":"armour-female","image":source_shape+1})).unwrap(),
        )
        .unwrap();
        assert!(publish(
            &assets,
            &db,
            &projects,
            source_id,
            "Wrong source",
            &rejected
        )
        .is_err());
        assert!(!rejected.exists());
        std::fs::write(&identity_path, identity).unwrap();
        let checksums: Vec<_> = [false, true]
            .into_iter()
            .map(|female| {
                let path = sprite_publish::source_library(
                    &assets,
                    animation::armour_library(0, female, false).unwrap(),
                )
                .unwrap();
                let checksum = sprite_project::file_checksum(&path).unwrap();
                (path, checksum)
            })
            .collect();
        let output = fixture.0.join("published");
        let result = publish(
            &assets,
            &db,
            &projects,
            source_id,
            "Wearable Test Robe",
            &output,
        )
        .unwrap();
        let shape = u16::try_from(result["shape"].as_u64().unwrap()).unwrap();
        let next = MirDb::load(result["database"].as_str().unwrap()).unwrap();
        let before = db.collection("ItemInfo").unwrap();
        let after = next.collection("ItemInfo").unwrap();
        assert_eq!(after.records.len(), before.records.len() + 1);
        assert!(!before
            .records
            .iter()
            .any(|r| before.int(r, "ItemType") == Some(3)
                && before.int(r, "Shape") == Some(i64::from(shape))));
        for old in &before.records {
            let new = after
                .records
                .iter()
                .find(|r| after.index(r) == before.index(old))
                .unwrap();
            for field in ["Shape", "Image", "RequiredClass", "RequiredGender"] {
                assert_eq!(before.int(old, field), after.int(new, field));
            }
        }
        let created = after
            .records
            .iter()
            .find(|r| after.index(r) == result["created_id"].as_i64().unwrap() as i32)
            .unwrap();
        assert_eq!(after.int(created, "Shape"), Some(i64::from(shape)));
        assert_eq!(after.int(created, "RequiredGender"), Some(3));
        for female in [false, true] {
            let library = animation::armour_library(shape, female, false).unwrap();
            let original_path = sprite_publish::source_library(&assets, library).unwrap();
            let original = ZlLibrary::open(&original_path).unwrap();
            let private = ZlLibrary::open(
                Path::new(result["assets"].as_str().unwrap()).join(
                    original_path
                        .strip_prefix(assets.canonicalize().unwrap())
                        .unwrap(),
                ),
            )
            .unwrap();
            let start = usize::from(shape % 11) * 5000;
            for index in [0, start.saturating_sub(1), start + 5000, original.len() - 1] {
                if index < original.len() && !(start..start + 5000).contains(&index) {
                    assert_eq!(
                        original.raw(index).unwrap().map(|r| r.payload),
                        private.raw(index).unwrap().map(|r| r.payload)
                    );
                }
            }
        }
        assert_eq!(db.to_bytes().unwrap(), source_bytes);
        assert_eq!(std::fs::read(database).unwrap(), original_database);
        for (path, checksum) in checksums {
            assert_eq!(sprite_project::file_checksum(&path).unwrap(), checksum);
        }
        assert!(publish(&assets, &db, &projects, source_id, "Overwrite", &output).is_err());
    }
}
