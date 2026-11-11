//! Publish authored sprites using unused appearances and an isolated asset view.
use anyhow::{ensure, Context, Result};
use mir_formats::{
    mirdb::{MirDb, Value},
    monster_table::{library_path, monster_sprite},
    zl::{RawImage, SurfaceKind, ZlBuilder, ZlLibrary},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Item,
    Monster,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub kind: Kind,
    pub source_id: i32,
    pub name: String,
}
#[derive(Debug, Serialize)]
pub struct Published {
    pub directory: PathBuf,
    pub assets: PathBuf,
    pub database: PathBuf,
    pub created_id: i32,
    pub image: u16,
    pub libraries: Vec<String>,
}

fn monster_appearance(db: &MirDb, source_image: u16) -> Result<(u16, u16, usize)> {
    let monsters = db
        .collection("MonsterInfo")
        .context("Missing MonsterInfo")?;
    let used: BTreeSet<_> = monsters
        .records
        .iter()
        .filter_map(|r| monsters.int(r, "Image"))
        .filter_map(|i| u16::try_from(i).ok())
        .filter_map(monster_sprite)
        .map(|(library, shape)| (library, shape % 10))
        .collect();
    (1..570)
        .filter(|&image| mir_formats::monster_animation::is_gate(image) == mir_formats::monster_animation::is_gate(source_image))
        .filter_map(|image| {
            monster_sprite(image).map(|(library, shape)| (image, library, shape % 10))
        })
        .find(|(_, library, shape)| !used.contains(&(*library, *shape)))
        .map(|(image, library, shape)| (image, library, usize::from(shape) * 1000))
        .context("No unused mapped monster appearance with the source animation profile remains in this database.")
}

fn source_image(db: &MirDb, request: &Request) -> Result<u16> {
    let name = if request.kind == Kind::Monster {
        "MonsterInfo"
    } else {
        "ItemInfo"
    };
    let c = db
        .collection(name)
        .with_context(|| format!("Missing {name}"))?;
    let row = c
        .records
        .iter()
        .find(|r| c.index(r) == request.source_id)
        .context("Source sprite record is missing.")?;
    u16::try_from(c.int(row, "Image").context("Source record has no image.")?)
        .context("Source image is out of range.")
}
pub(crate) fn source_library(assets: &Path, id: u16) -> Result<PathBuf> {
    let relative = library_path(id).context("No asset path for sprite library.")?;
    let mut path = assets.to_path_buf();
    for part in relative.split('/') {
        path = std::fs::read_dir(&path)?
            .collect::<std::io::Result<Vec<_>>>()?
            .into_iter()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(part)
            })
            .context("Required sprite library is missing.")?
            .path();
    }
    Ok(path)
}
pub(crate) fn open_bounded(path: &Path) -> Result<ZlLibrary> {
    ensure!(
        std::fs::metadata(path)?.len() <= 512 * 1024 * 1024,
        "Sprite library exceeds 512 MiB."
    );
    Ok(ZlLibrary::open(path)?)
}
fn validate_built(library: &ZlLibrary, kind: Kind, source_image: u16) -> Result<()> {
    let slots = if kind == Kind::Monster { 1000 } else { 1 };
    ensure!(
        library.len() == slots,
        "Authored pack must contain exactly {slots} local slots."
    );
    let mut pixels = 0u64;
    for index in 0..library.len() {
        if let Some(info) = library.info(index) {
            for layer in [
                SurfaceKind::Image,
                SurfaceKind::Shadow,
                SurfaceKind::Overlay,
            ] {
                let (w, h) = info.surface_dims(layer);
                ensure!(
                    w <= 4096 && h <= 4096,
                    "Authored surface exceeds 4096 pixels."
                );
                pixels += u64::from(w) * u64::from(h);
            }
        }
    }
    ensure!(
        pixels <= 64 * 1024 * 1024,
        "Authored decoded surfaces exceed 64 million pixels."
    );
    let required: BTreeSet<usize> = if kind == Kind::Monster {
        use mir_formats::monster_animation::{self as animation, ACTIONS};
        ACTIONS
            .into_iter()
            .flat_map(|action| {
                crate::sprite_contract::monster_directions(source_image, action)
                    .iter()
                    .flat_map(move |&direction| {
                        (0..animation::frame_for_image(source_image, action).count).map(
                            move |frame| {
                                animation::slot_for_image(source_image, 0, action, direction, frame)
                                    .unwrap() as usize
                            },
                        )
                    })
            })
            .collect()
    } else {
        BTreeSet::from([0])
    };
    for slot in required {
        let image = library.decode(slot, SurfaceKind::Image)?.with_context(|| {
            format!("Required authored animation slot {slot} has no body image.")
        })?;
        ensure!(
            image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 0),
            "Required authored animation slot {slot} is fully transparent."
        );
    }
    for slot in 0..library.len() {
        for layer in [
            SurfaceKind::Image,
            SurfaceKind::Shadow,
            SurfaceKind::Overlay,
        ] {
            library.decode(slot, layer)?;
        }
    }
    Ok(())
}
fn converted(library: &ZlLibrary, index: usize, version: u8) -> Result<Option<RawImage>> {
    let Some(mut raw) = library.raw(index)? else {
        return Ok(None);
    };
    if library.version() == version {
        return Ok(Some(raw));
    }
    raw.payload.clear();
    let encoder = ZlBuilder::new(version);
    for layer in [
        SurfaceKind::Image,
        SurfaceKind::Shadow,
        SurfaceKind::Overlay,
    ] {
        if let Some(surface) = library.decode(index, layer)? {
            let (w, h) = raw.info.surface_dims(layer);
            let mut rgba = Vec::with_capacity(usize::from(w) * usize::from(h) * 4);
            for y in 0..usize::from(h) {
                let start = y * surface.width as usize * 4;
                rgba.extend_from_slice(&surface.rgba[start..start + usize::from(w) * 4]);
            }
            raw.payload.extend(encoder.encode_surface(&rgba, w, h));
        }
    }
    Ok(Some(raw))
}
pub(crate) fn replacement(
    original: &ZlLibrary,
    authored: &ZlLibrary,
    start: usize,
) -> Result<Vec<u8>> {
    ensure!(
        start
            .checked_add(authored.len())
            .is_some_and(|end| end <= crate::sprite_project::MAX_SLOTS),
        "Published sprite indices exceed 131072 slots."
    );
    let mut builder = ZlBuilder::new(original.version());
    for index in 0..original.len().max(start + authored.len()) {
        builder.push_raw(if (start..start + authored.len()).contains(&index) {
            converted(authored, index - start, original.version())?
        } else if index < original.len() {
            original.raw(index)?
        } else {
            None
        });
    }
    Ok(builder.to_bytes())
}
pub(crate) fn clone_record(db: &MirDb, request: &Request, image: u16) -> Result<(MirDb, i32)> {
    ensure!(
        !request.name.trim().is_empty() && request.name.chars().count() <= 80,
        "Sprite name must contain 1–80 characters."
    );
    let (kind, field, stats, link) = if request.kind == Kind::Monster {
        ("MonsterInfo", "MonsterName", "MonsterInfoStat", "Monster")
    } else {
        ("ItemInfo", "ItemName", "ItemInfoStat", "Item")
    };
    let mut next = MirDb::parse(&db.to_bytes()?)?;
    let c = next
        .collection(kind)
        .context("Source collection missing.")?;
    ensure!(
        !c.records.iter().any(|r| c
            .str_or(r, field, "")
            .eq_ignore_ascii_case(request.name.trim())),
        "A record with this sprite name already exists."
    );
    let mut record = c
        .records
        .iter()
        .find(|r| c.index(r) == request.source_id)
        .context("Source record is missing.")?
        .clone();
    for property in ["Index", field, "Image"] {
        ensure!(
            c.has_property(property),
            "Source collection lacks {property}."
        );
    }
    if request.kind == Kind::Item {
        ensure!(
            c.has_property("StartItem"),
            "Source item schema lacks StartItem."
        );
    }
    let stat_collection = next
        .collection(stats)
        .context("Source stats collection missing.")?;
    ensure!(
        stat_collection.has_property("Index") && stat_collection.has_property(link),
        "Source stats schema is invalid."
    );
    let mut rows: Vec<_> = stat_collection
        .records
        .iter()
        .filter(|r| stat_collection.int(r, link) == Some(i64::from(request.source_id)))
        .cloned()
        .collect();
    for (c, count) in [(c, 1), (stat_collection, rows.len())] {
        let start = i64::from(c.next_index)
            .max(i64::from(c.records.iter().map(|r| c.index(r)).max().unwrap_or(0)) + 1);
        ensure!(
            start > 0 && start + count as i64 <= i64::from(i32::MAX),
            "Record identities are exhausted."
        );
    }
    let c = next.collection_mut(kind).unwrap();
    let id = c.allocate_index();
    for (key, value) in [
        ("Index", Value::Int(i64::from(id))),
        (field, Value::Str(request.name.trim().into())),
        ("Image", Value::Int(i64::from(image))),
    ] {
        ensure!(
            c.set(&mut record, key, value),
            "Cannot set sprite record field."
        );
    }
    if request.kind == Kind::Item {
        ensure!(
            c.set(&mut record, "StartItem", Value::Bool(false)),
            "Cannot disable starting-item grant."
        );
    }
    c.records.push(record);
    let c = next.collection_mut(stats).unwrap();
    for row in &mut rows {
        let stat_id = c.allocate_index();
        c.set(row, "Index", Value::Int(i64::from(stat_id)));
        c.set(row, link, Value::Int(i64::from(id)));
    }
    c.records.extend(rows);
    Ok((next, id))
}

fn link_dependency(source: &Path, target: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, target)?;
    }
    #[cfg(not(unix))]
    {
        if source.is_dir() {
            std::fs::create_dir(target)?;
            for entry in std::fs::read_dir(source)
                .with_context(|| format!("Read asset directory {}", source.display()))?
            {
                let entry = entry?;
                link_dependency(&entry.path(), &target.join(entry.file_name()))?;
            }
        } else {
            std::fs::copy(source, target)?;
        }
    }
    Ok(())
}
pub(crate) fn asset_view(
    root: &Path,
    source: &Path,
    target: &Path,
    overrides: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<()> {
    std::fs::create_dir(target)?;
    for entry in std::fs::read_dir(source)
        .with_context(|| format!("Read asset directory {}", source.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(root)?;
        let destination = target.join(entry.file_name());
        let canonical = path
            .canonicalize()
            .with_context(|| format!("Resolve asset dependency {}", path.display()))?;
        if overrides.contains_key(relative) {
            continue;
        }
        if overrides.keys().any(|key| key.starts_with(relative)) {
            ensure!(path.is_dir(), "Override parent must be a source directory.");
            asset_view(root, &path, &destination, overrides)?;
        } else {
            link_dependency(&canonical, &destination).with_context(|| {
                format!(
                    "Link asset dependency {} to {}",
                    canonical.display(),
                    destination.display()
                )
            })?;
        }
    }
    Ok(())
}

/// Add a new item/monster record and unused appearance to a NEW private asset
/// view. Existing records and every other library slot remain unchanged. Unix
/// views reference unchanged source assets with symlinks; other platforms copy.
pub fn publish(
    assets: &Path,
    db: &MirDb,
    project: &Path,
    built_library: &Path,
    request: &Request,
    output: &Path,
) -> Result<Published> {
    let assets = assets.canonicalize()?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
        .context("Output parent must already exist.")?;
    ensure!(
        !parent.starts_with(&assets),
        "Publish outside the source Client directory."
    );
    ensure!(
        std::fs::symlink_metadata(output).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "Publish requires a new output directory."
    );
    let source_image = source_image(db, request)?;
    let (source_lib, source_start) = if request.kind == Kind::Monster {
        let (lib, shape) =
            monster_sprite(source_image).context("Source monster appearance is unsupported.")?;
        (lib, usize::from(shape % 10) * 1000)
    } else {
        (14, usize::from(source_image))
    };
    let source_path = source_library(&assets, source_lib)?.canonicalize()?;
    crate::sprite_project::validate(project)?;
    let manifest = crate::sprite_project::load_manifest(project)?;
    ensure!(
        Path::new(&manifest.source_path).canonicalize()? == source_path
            && manifest.source_start == source_start,
        "Sprite project does not match the selected source record appearance."
    );
    ensure!(
        crate::sprite_project::file_checksum(&source_path)? == manifest.source_checksum,
        "Source sprite library changed after project export."
    );
    let built = open_bounded(built_library)?;
    validate_built(&built, request.kind, source_image)?;
    ensure!(
        built.version() == manifest.library_version && built.len() == manifest.slots.len(),
        "Built sprite pack does not match its project contract."
    );
    let mut originals = vec![];
    let (image, start) = if request.kind == Kind::Monster {
        let (image, library, start) = monster_appearance(db, source_image)?;
        let path = source_library(&assets, library)?;
        originals.push((path.clone(), open_bounded(&path)?));
        (image, start)
    } else {
        let mut start = db
            .collection("ItemInfo")
            .context("Missing ItemInfo")?
            .records
            .iter()
            .filter_map(|r| db.collection("ItemInfo").unwrap().int(r, "Image"))
            .filter_map(|n| usize::try_from(n).ok())
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .context("Item image identities are exhausted.")?;
        for library in [14, 15, 16] {
            let path = source_library(&assets, library)?;
            let original = open_bounded(&path)?;
            start = start.max(original.len());
            originals.push((path, original));
        }
        (
            u16::try_from(start).context("No unused item icon index remains.")?,
            start,
        )
    };
    let (next, id) = clone_record(db, request, image)?;
    let mut overrides = BTreeMap::new();
    for (path, original) in &originals {
        overrides.insert(
            path.strip_prefix(&assets)?.to_path_buf(),
            replacement(original, &built, start)?,
        );
    }
    let published = Published {
        directory: output.into(),
        assets: output.join("Client"),
        database: output.join("System.db"),
        created_id: id,
        image,
        libraries: overrides
            .keys()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
    };
    std::fs::create_dir(output)?;
    let result = (|| -> Result<()> {
        asset_view(&assets, &assets, &published.assets, &overrides)
            .context("Create private asset view")?;
        for (relative, bytes) in &overrides {
            let path = published.assets.join(relative);
            std::fs::create_dir_all(path.parent().unwrap())?;
            ensure!(
                std::fs::symlink_metadata(&path)
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
                "Override destination must not exist or be a symlink."
            );
            std::fs::write(&path, bytes)?;
            let saved = ZlLibrary::open(&path)?;
            ensure!(
                saved.len() >= start + built.len(),
                "Published sprite slots missing."
            );
        }
        next.save(&published.database)
            .context("Save private database")?;
        let record = serde_json::json!({"version":1,"request":request,"published":published,"source_assets":assets,"source_image":source_image,"source_checksum":manifest.source_checksum,"usage":"Private asset view. Unchanged Unix dependencies are read-only references to installed assets. Use Client as --assets and System.db as --db; build a playable map bundle with this database. Do not edit linked source dependencies.","scope":"Only the new record uses the allocated appearance; existing records, drops and spawns are unchanged."});
        std::fs::write(
            output.join("sprite-publish.json"),
            serde_json::to_vec_pretty(&record)?,
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(output);
        return Err(error);
    }
    Ok(published)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn temporary(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "zircon-publish-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }
    fn sprite(version: u8, alpha: u8, path: &Path) -> ZlLibrary {
        let mut builder = ZlBuilder::new(version);
        builder.push_rgba(&mir_formats::zl::Sprite {
            width: 4,
            height: 4,
            offset_x: -2,
            offset_y: 7,
            rgba: &[20, 80, 160, alpha].repeat(16),
            shadow_type: 0,
            shadow_offset_x: 0,
            shadow_offset_y: 0,
            shadow: None,
        });
        std::fs::write(path, builder.to_bytes()).unwrap();
        ZlLibrary::open(path).unwrap()
    }
    #[test]
    fn replacement_preserves_other_raw_slots_and_converts_codec() {
        let dir = temporary("codec");
        let original = sprite(1, 255, &dir.join("original.zl"));
        let authored = sprite(2, 255, &dir.join("authored.zl"));
        std::fs::write(
            dir.join("result.zl"),
            replacement(&original, &authored, 2).unwrap(),
        )
        .unwrap();
        let result = ZlLibrary::open(dir.join("result.zl")).unwrap();
        assert_eq!(
            result.raw(0).unwrap().unwrap().payload,
            original.raw(0).unwrap().unwrap().payload
        );
        assert!(result.info(1).is_none());
        assert_eq!(result.info(2).unwrap().offset_x, -2);
        assert_eq!(result.info(2).unwrap().offset_y, 7);
        assert!(result
            .decode(2, SurfaceKind::Image)
            .unwrap()
            .unwrap()
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[3] > 200));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn required_images_reject_empty_and_transparent_packs() {
        let dir = temporary("invalid");
        let transparent = sprite(2, 0, &dir.join("transparent.zl"));
        assert!(validate_built(&transparent, Kind::Item, 0).is_err());
        let mut empty = ZlBuilder::new(2);
        for _ in 0..1000 {
            empty.push_raw(None);
        }
        std::fs::write(dir.join("empty.zl"), empty.to_bytes()).unwrap();
        assert!(validate_built(
            &ZlLibrary::open(dir.join("empty.zl")).unwrap(),
            Kind::Monster,
            33
        )
        .is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn item_publications_chain_without_reskinning_or_source_mutation() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        let dbpath = assets.join("../Database/System.db");
        let db = MirDb::load(&dbpath).unwrap();
        let original_db = std::fs::read(&dbpath).unwrap();
        let source = source_library(&assets, 14).unwrap();
        let checksum = crate::sprite_project::file_checksum(&source).unwrap();
        let library = ZlLibrary::open(&source).unwrap();
        let items = db.collection("ItemInfo").unwrap();
        let row = items
            .records
            .iter()
            .find(|r| {
                items
                    .int(r, "Image")
                    .and_then(|n| usize::try_from(n).ok())
                    .is_some_and(|n| library.info(n).is_some_and(|i| i.width > 0 && i.height > 0))
            })
            .unwrap();
        let source_id = items.index(row);
        let start = items.int(row, "Image").unwrap() as usize;
        let dir = temporary("chain");
        let project = dir.join("project");
        crate::sprite_project::export_range(&source, start, 1, &project).unwrap();
        let built = crate::sprite_project::build(&project, &dir.join("build"))
            .unwrap()
            .library
            .unwrap();
        let request = Request {
            kind: Kind::Item,
            source_id,
            name: "Private publish regression one".into(),
        };
        let first = publish(&assets, &db, &project, &built, &request, &dir.join("first")).unwrap();
        let next = MirDb::load(&first.database).unwrap();
        assert_eq!(
            format!(
                "{:?}",
                &next.collection("ItemInfo").unwrap().records[..items.records.len()]
            ),
            format!("{:?}", items.records.as_slice())
        );
        let request = Request {
            name: "Private publish regression two".into(),
            ..request
        };
        let second_project = dir.join("second-project");
        crate::sprite_project::export_range(
            &source_library(&first.assets, 14).unwrap(),
            start,
            1,
            &second_project,
        )
        .unwrap();
        let second_built = crate::sprite_project::build(&second_project, &dir.join("second-build"))
            .unwrap()
            .library
            .unwrap();
        let second = publish(
            &first.assets,
            &next,
            &second_project,
            &second_built,
            &request,
            &dir.join("second"),
        )
        .unwrap();
        assert_ne!(first.image, second.image);
        for lib in [14, 15, 16] {
            let one = ZlLibrary::open(source_library(&first.assets, lib).unwrap()).unwrap();
            let two = ZlLibrary::open(source_library(&second.assets, lib).unwrap()).unwrap();
            assert_eq!(
                one.raw(first.image as usize).unwrap().unwrap().payload,
                two.raw(first.image as usize).unwrap().unwrap().payload
            );
        }
        assert_eq!(std::fs::read(&dbpath).unwrap(), original_db);
        assert_eq!(
            crate::sprite_project::file_checksum(&source).unwrap(),
            checksum
        );
        assert!(publish(&assets, &db, &project, &built, &request, &dir.join("first")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn monster_publish_allocates_only_unreferenced_shape() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        let db = MirDb::load(assets.join("../Database/System.db")).unwrap();
        let monsters = db.collection("MonsterInfo").unwrap();
        let row = monsters
            .records
            .iter()
            .find(|r| monsters.int(r, "Image") == Some(33))
            .unwrap();
        let source_id = monsters.index(row);
        let (lib, shape) = monster_sprite(33).unwrap();
        let source = source_library(&assets, lib).unwrap();
        let dir = temporary("monster");
        let project = dir.join("project");
        crate::sprite_project::export_range(
            &source,
            usize::from(shape % 10) * 1000,
            1000,
            &project,
        )
        .unwrap();
        let version = ZlLibrary::open(&source).unwrap().version();
        let icon = sprite(version, 255, &dir.join("icon.zl"));
        let mut builder = ZlBuilder::new(version);
        for _ in 0..1000 {
            builder.push_raw(icon.raw(0).unwrap());
        }
        let built = dir.join("built.zl");
        std::fs::write(&built, builder.to_bytes()).unwrap();
        let (expected, lib, start) = monster_appearance(&db, 33).unwrap();
        let original = ZlLibrary::open(source_library(&assets, lib).unwrap()).unwrap();
        let result = publish(
            &assets,
            &db,
            &project,
            &built,
            &Request {
                kind: Kind::Monster,
                source_id,
                name: "Private monster regression".into(),
            },
            &dir.join("published"),
        )
        .unwrap();
        assert_eq!(result.image, expected);
        let next = MirDb::load(&result.database).unwrap();
        assert_eq!(
            format!(
                "{:?}",
                &next.collection("MonsterInfo").unwrap().records[..monsters.records.len()]
            ),
            format!("{:?}", monsters.records)
        );
        assert_ne!(monster_appearance(&next, 33).unwrap().0, result.image);
        let library = ZlLibrary::open(source_library(&result.assets, lib).unwrap()).unwrap();
        for slot in 0..original.len() {
            if !(start..start + 1000).contains(&slot) {
                assert_eq!(
                    original.raw(slot).unwrap().map(|r| r.payload),
                    library.raw(slot).unwrap().map(|r| r.payload)
                );
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn gate_publish_preserves_profile_art_and_rejects_default_contract() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        let mut db = MirDb::load(assets.join("../Database/System.db")).unwrap();
        // Stock DB has no gate records. Stage one privately using existing stats.
        let c = db.collection_mut("MonsterInfo").unwrap();
        let position = c
            .records
            .iter()
            .position(|r| c.int(r, "Image") == Some(33))
            .unwrap();
        let mut row = c.records[position].clone();
        let source_id = c.index(&row);
        assert!(c.set(&mut row, "Image", Value::Int(543)));
        assert!(c.set(&mut row, "FaceImage", Value::Int(1)));
        c.records[position] = row;
        let source = source_library(&assets, monster_sprite(543).unwrap().0).unwrap();
        let original = ZlLibrary::open(&source).unwrap();
        let dir = temporary("gate");
        let project = dir.join("project");
        crate::sprite_project::export_range(&source, 3000, 1000, &project).unwrap();
        let mut builder = ZlBuilder::new(original.version());
        for slot in 3000..4000 {
            builder.push_raw(original.raw(slot).unwrap());
        }
        let built = dir.join("built.zl");
        std::fs::write(&built, builder.to_bytes()).unwrap();
        let pack = ZlLibrary::open(&built).unwrap();
        validate_built(&pack, Kind::Monster, 543).unwrap();
        assert!(validate_built(&pack, Kind::Monster, 33).is_err());
        assert!(!mir_formats::monster_animation::is_gate(
            monster_appearance(&db, 33).unwrap().0
        ));
        let result = publish(
            &assets,
            &db,
            &project,
            &built,
            &Request {
                kind: Kind::Monster,
                source_id,
                name: "Private gate regression".into(),
            },
            &dir.join("published"),
        )
        .unwrap();
        assert!(mir_formats::monster_animation::is_gate(result.image));
        assert_ne!(result.image, 543);
        let next = MirDb::load(&result.database).unwrap();
        let c = next.collection("MonsterInfo").unwrap();
        let row = c
            .records
            .iter()
            .find(|r| c.index(r) == result.created_id)
            .unwrap();
        assert_eq!(c.int(row, "FaceImage"), Some(1));
        let (lib, shape) = monster_sprite(result.image).unwrap();
        let published = ZlLibrary::open(source_library(&result.assets, lib).unwrap()).unwrap();
        for slot in 0..1000 {
            assert_eq!(
                pack.raw(slot).unwrap().map(|r| r.payload),
                published
                    .raw(usize::from(shape % 10) * 1000 + slot)
                    .unwrap()
                    .map(|r| r.payload)
            );
        }
        // Exhaust gate appearances without exhausting default appearances.
        let mut exhausted = next;
        let c = exhausted.collection_mut("MonsterInfo").unwrap();
        for image in 540..=543 {
            c.push_record(vec![("Image", Value::Int(image))]);
        }
        assert!(monster_appearance(&exhausted, 543).is_err());
        assert!(monster_appearance(&exhausted, 33).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn installed_database_has_an_unused_mapped_appearance() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
            return;
        };
        let db = MirDb::load(assets.join("../Database/System.db")).unwrap();
        let (image, library, start) = monster_appearance(&db, 33).unwrap();
        println!(
            "Available authored monster Image={image},library={library},first_slot={start},path={}",
            library_path(library).unwrap()
        );
    }
}
