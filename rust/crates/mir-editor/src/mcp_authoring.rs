//! Headless authoring operations. Inputs are read-only; each edit creates a new revision.
use crate::{
    generate, item_draft, monster_draft, piece_assembly,
    tile_doc::{Document, Layer, Stamp, Tile},
    wall_kit,
    wall_piece::{Kind, Piece, Run},
    wood_corners,
};
use anyhow::{ensure, Context, Result};
use mir_formats::{map::MapFile, mirdb::MirDb, navigation::Navigation};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Component, Path, PathBuf},
};

pub struct Workspace {
    root: PathBuf,
    assets: PathBuf,
    database: PathBuf,
}
#[derive(Default)]
struct Metadata {
    layout: Option<(generate::Layout, i32)>,
    navigation: Option<Navigation>,
    room_doors: Option<mir_formats::room_door::RoomDoors>,
    private_assets: Option<PathBuf>,
}
impl Workspace {
    pub fn open(root: &Path, assets: &Path, database: &Path) -> Result<Self> {
        let assets = assets.canonicalize()?;
        let database = database.canonicalize()?;
        let root = if root.exists() {
            root.canonicalize()?
        } else {
            let parent = root
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            parent
                .canonicalize()?
                .join(root.file_name().context("Project directory needs a name")?)
        };
        ensure!(
            !root.starts_with(&assets),
            "Choose an authoring project outside the installed asset directory."
        );
        if !root.exists() {
            std::fs::create_dir(&root)?;
        }
        ensure!(root.is_dir(), "Project must be a directory.");
        Ok(Self {
            root,
            assets,
            database,
        })
    }
    fn input(&self, text: &str) -> Result<PathBuf> {
        let (kind, relative) = text
            .split_once(':')
            .context("Use asset:MapName.map or project:revision/file.map")?;
        let root = match kind {
            "asset" => self.assets.join("Map"),
            "project" => self.root.clone(),
            _ => anyhow::bail!("Unknown input prefix"),
        }
        .canonicalize()?;
        ensure!(
            !relative.is_empty()
                && Path::new(relative)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_))),
            "Input must be a relative path without traversal."
        );
        let file = root.join(relative).canonicalize()?;
        ensure!(
            file.starts_with(&root) && file.is_file(),
            "Input escapes the selected root or is not a file."
        );
        Ok(file)
    }
    fn db(&self, relative: Option<&str>) -> Result<MirDb> {
        let path = match relative {
            Some(p) => {
                ensure!(
                    p.starts_with("project:"),
                    "Database revisions must use project: paths."
                );
                self.input(p)?
            }
            None => self.database.clone(),
        };
        ensure!(
            std::fs::metadata(&path)?.len() <= 256 * 1024 * 1024,
            "Database exceeds 256 MiB."
        );
        Ok(MirDb::load(path)?)
    }
    fn map(&self, name: &str) -> Result<(PathBuf, MapFile)> {
        let path = self.input(name)?;
        ensure!(
            std::fs::metadata(&path)?.len() <= 16 * 1024 * 1024,
            "Map exceeds 16 MiB."
        );
        let map = MapFile::load(&path)?;
        ensure!(
            map.width <= 512 && map.height <= 512,
            "Authoring supports maps up to 512 cells per side."
        );
        Ok((path, map))
    }
    fn metadata(&self, path: &Path, map: &MapFile) -> Result<Metadata> {
        let parent = path
            .parent()
            .context("Map needs a parent directory")?
            .canonicalize()?;
        let read = |extension: &str| -> Result<Option<Vec<u8>>> {
            let sidecar = path.with_extension(extension);
            match std::fs::symlink_metadata(&sidecar) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
                Ok(_) => {}
            }
            let sidecar = sidecar.canonicalize()?;
            ensure!(
                sidecar.starts_with(&parent),
                "Map metadata escapes its map directory."
            );
            let mut file = std::fs::File::open(sidecar)?;
            ensure!(file.metadata()?.is_file(), "Map metadata must be a file.");
            let mut bytes = Vec::new();
            use std::io::Read;
            (&mut file).take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 1024 * 1024, "Map metadata exceeds 1 MiB.");
            Ok(Some(bytes))
        };
        let metadata = Metadata {
            layout: read("cave.json")?
                .map(|bytes| serde_json::from_slice(&bytes))
                .transpose()?,
            navigation: read("navigation.json")?
                .map(|bytes| serde_json::from_slice(&bytes))
                .transpose()?,
            room_doors: mir_formats::room_door::RoomDoors::load(path, map)?,
            private_assets: parent
                .ancestors()
                .find(|directory| directory.parent() == Some(self.root.as_path()))
                .map(|revision| revision.join("Client"))
                .filter(|assets| assets.is_dir()),
        };
        if let Some(doors) = &metadata.room_doors {
            for door in &doors.doors {
                crate::wood_doorway::inspect(map, (door.center[0], door.center[1]))?;
            }
        }
        if let Some((layout, _)) = &metadata.layout {
            let expected = layout.navigation(map)?;
            if let Some(navigation) = &metadata.navigation {
                ensure!(
                    *navigation == expected,
                    "Layout and navigation metadata disagree."
                );
            }
        }
        if let Some(navigation) = &metadata.navigation {
            navigation.validate(map)?;
        }
        Ok(metadata)
    }
    fn output(&self, name: &str) -> Result<PathBuf> {
        ensure!(
            !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "Output revision needs 1–64 letters, digits, underscores or hyphens."
        );
        let path = self.root.join(name);
        ensure!(
            !path.exists(),
            "Revision already exists. Choose a new output name."
        );
        Ok(path)
    }
    fn write_revision(&self, name: &str, files: Vec<(&str, Vec<u8>)>) -> Result<Value> {
        let folder = self.output(name)?;
        std::fs::create_dir(&folder)?;
        let mut paths = Vec::new();
        for (file, bytes) in files {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(folder.join(file))
                .context("Revision write failed; the new directory may contain partial results.")?;
            f.write_all(&bytes)
                .context("Revision write failed; the new directory may contain partial results.")?;
            f.sync_all()?;
            paths.push(format!("project:{name}/{file}"));
        }
        Ok(json!({"revision":name,"files":paths,"directory":folder}))
    }
    fn save_map(&self, name: &str, map: &MapFile, metadata: Metadata) -> Result<Value> {
        let private_assets = metadata.private_assets.clone();
        let mut files = vec![("Map.map", map.to_bytes())];
        if let Some((layout, monster)) = metadata.layout {
            let nav = layout.navigation(map)?;
            files.push((
                "Map.cave.json",
                serde_json::to_vec_pretty(&(layout, monster))?,
            ));
            files.push(("Map.navigation.json", serde_json::to_vec_pretty(&nav)?));
        } else if let Some(navigation) = metadata.navigation {
            let navigation = Navigation::new(map, navigation.portals)?;
            files.push((
                "Map.navigation.json",
                serde_json::to_vec_pretty(&navigation)?,
            ));
        }
        if let Some(doors) = metadata.room_doors {
            doors.validate(map)?;
            for door in &doors.doors {
                crate::wood_doorway::inspect(map, (door.center[0], door.center[1]))?;
            }
            files.push(("Map.doors.json", serde_json::to_vec_pretty(&doors)?));
        }
        let mut result = self.write_revision(name, files)?;
        if let Some(source) = private_assets {
            let destination = self.root.join(name).join("Client");
            if let Err(error) =
                crate::sprite_publish::asset_view(&source, &source, &destination, &BTreeMap::new())
            {
                let _ = std::fs::remove_dir_all(self.root.join(name));
                return Err(error);
            }
            result["assets"] = json!(destination);
        }
        Ok(result)
    }
    fn publish_assets(&self, reference: Option<&str>) -> Result<PathBuf> {
        Ok(if let Some(reference) = reference {
            let relative = reference
                .strip_prefix("project:")
                .context("Assets must use project: paths")?;
            ensure!(
                !relative.is_empty()
                    && Path::new(relative)
                        .components()
                        .all(|c| matches!(c, Component::Normal(_))),
                "Invalid private asset path"
            );
            let path = self.root.join(relative).canonicalize()?;
            ensure!(
                path.starts_with(&self.root) && path.is_dir(),
                "Private assets escape project"
            );
            path
        } else {
            self.assets.clone()
        })
    }
    fn private_assets_for_database(
        &self,
        database: Option<&str>,
        assets: Option<&str>,
    ) -> Result<Option<PathBuf>> {
        if let Some(assets) = assets {
            return self.publish_assets(Some(assets)).map(Some);
        }
        let Some(database) = database else {
            return Ok(None);
        };
        let sibling = self.input(database)?.with_file_name("Client");
        if !sibling.is_dir() {
            return Ok(None);
        }
        let assets = sibling.canonicalize()?;
        ensure!(
            assets.starts_with(&self.root),
            "Private assets escape project"
        );
        Ok(Some(assets))
    }
    fn private_assets_for_map_and_database(
        &self,
        map_assets: Option<PathBuf>,
        database: Option<&str>,
        assets: Option<&str>,
    ) -> Result<Option<PathBuf>> {
        if assets.is_some() {
            return self.private_assets_for_database(database, assets);
        }
        let database_assets = self.private_assets_for_database(database, None)?;
        match (map_assets, database_assets) {
            (Some(map), Some(db)) if map.canonicalize()? != db.canonicalize()? => {
                anyhow::bail!("Map and database have different private Client views; pass an explicit combined assets path.")
            }
            (Some(map), _) => Ok(Some(map)),
            (_, db) => Ok(db),
        }
    }
    fn inherit_private_assets(&self, source: &Path, folder: &Path) -> Result<PathBuf> {
        self.private_asset_view(source, folder, &BTreeMap::new())
    }
    fn private_asset_view(
        &self,
        source: &Path,
        folder: &Path,
        overrides: &BTreeMap<PathBuf, Vec<u8>>,
    ) -> Result<PathBuf> {
        let destination = folder.join("Client");
        crate::sprite_publish::asset_view(source, source, &destination, overrides)?;
        for (relative, bytes) in overrides {
            let path = destination.join(relative);
            std::fs::create_dir_all(path.parent().context("Missing private art parent")?)?;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?
                .write_all(bytes)?;
        }
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(folder.join("README.txt"))?,
            "Set ZIRCON_ASSETS={} for private art.",
            destination.display()
        )?;
        Ok(destination)
    }
    fn write_database_revision(
        &self,
        name: &str,
        bytes: Vec<u8>,
        private_assets: Option<&Path>,
    ) -> Result<Value> {
        let mut result = self.write_revision(name, vec![("System.db", bytes)])?;
        if let Some(source) = private_assets {
            let folder = self.root.join(name);
            match self.inherit_private_assets(source, &folder) {
                Ok(assets) => result["assets"] = json!(assets),
                Err(error) => {
                    let _ = std::fs::remove_dir_all(folder);
                    return Err(error);
                }
            }
        }
        Ok(result)
    }

    fn sprite_project(&self, name: &str) -> Result<PathBuf> {
        ensure!(
            name.starts_with("project:"),
            "Sprite projects must use project: paths"
        );
        let path = self.input(name)?;
        ensure!(
            path.file_name().is_some_and(|n| n == "project.json"),
            "Select the sprite project.json manifest"
        );
        Ok(path
            .parent()
            .context("Missing project directory")?
            .to_path_buf())
    }
    pub fn call(&self, name: &str, arguments: Value) -> Result<Value> {
        match name {
            "sprite_export_project" | "sprite_reference" => {
                let a: SpriteExport = serde_json::from_value(arguments)?;
                let output = self.output(&a.output)?;
                Ok(serde_json::to_value(crate::sprite_authoring::export(
                    &self.assets,
                    &crate::sprite_authoring::Identity {
                        kind: a.kind,
                        image: a.source_image,
                    },
                    &output,
                )?)?)
            }
            "sprite_pose_from_reference" => {
                let a: SpritePoseReference = serde_json::from_value(arguments)?;
                Ok(serde_json::to_value(
                    crate::sprite_authoring::pose_from_reference(
                        &self.assets,
                        &crate::sprite_authoring::Identity {
                            kind: a.kind,
                            image: a.source_image,
                        },
                        a.donor_image,
                        &self.output(&a.output)?,
                    )?,
                )?)
            }
            "sprite_validate_project" => {
                let a: SpriteInput = serde_json::from_value(arguments)?;
                Ok(serde_json::to_value(crate::sprite_authoring::validate(
                    &self.assets,
                    &self.sprite_project(&a.project)?,
                )?)?)
            }
            "sprite_build_pack" | "sprite_preview_sheet" => {
                let a: SpriteOutput = serde_json::from_value(arguments)?;
                let project = self.sprite_project(&a.project)?;
                let output = self.output(&a.output)?;
                crate::sprite_authoring::validate(&self.assets, &project)?;
                if name == "sprite_build_pack" {
                    Ok(serde_json::to_value(crate::sprite_project::build(
                        &project, &output,
                    )?)?)
                } else {
                    let identity = crate::sprite_authoring::identity(&project)?;
                    let contract = crate::sprite_authoring::contract(&self.assets, &identity)?;
                    std::fs::create_dir(&output)?;
                    let result = crate::sprite_contract::preview_project(
                        &contract,
                        &project,
                        &output.join("preview.png"),
                    );
                    if let Err(error) = result {
                        let _ = std::fs::remove_dir_all(&output);
                        return Err(error);
                    }
                    Ok(json!({"output":output,"image":output.join("preview.png")}))
                }
            }
            "sprite_import_image" => {
                let a: SpriteImage = serde_json::from_value(arguments)?;
                let project = self.sprite_project(&a.project)?;
                ensure!(
                    a.image.starts_with("project:"),
                    "Artwork must use a project: path"
                );
                let image = self.input(&a.image)?;
                let output = self.output(&a.output)?;
                let options = crate::sprite_project::ImportOptions {
                    fit: a.fit,
                    binary_alpha: a.binary_alpha,
                    trim_transparent: a.trim_transparent,
                };
                let report = if let Some(canvas) = a.canvas {
                    ensure!(a.layer == "image", "New canvas requires the image layer");
                    let offsets = a
                        .offsets
                        .context("A new canvas requires explicit offsets")?;
                    let (w, h, _) = crate::sprite_project::read_png(&image)?;
                    crate::sprite_project::import_sheet_canvas(
                        &project,
                        &image,
                        w,
                        h,
                        &[crate::sprite_project::SheetCell {
                            slot: a.slot,
                            layer: a.layer,
                            column: 0,
                            row: 0,
                        }],
                        &output,
                        crate::sprite_project::CanvasSheetOptions {
                            options,
                            canvas,
                            offsets,
                            replace_layers: false,
                        },
                    )?
                } else {
                    ensure!(a.offsets.is_none(), "Offsets require a new canvas");
                    crate::sprite_project::import_image_options(
                        &project, a.slot, &a.layer, &image, &output, options,
                    )?
                };
                Ok(serde_json::to_value(report)?)
            }
            "sprite_import_sheet" => {
                let a: SpriteSheet = serde_json::from_value(arguments)?;
                let project = self.sprite_project(&a.project)?;
                ensure!(
                    a.image.starts_with("project:"),
                    "Artwork must use a project: path"
                );
                let image = self.input(&a.image)?;
                let output = self.output(&a.output)?;
                let options = crate::sprite_project::ImportOptions {
                    fit: a.fit,
                    binary_alpha: a.binary_alpha,
                    trim_transparent: a.trim_transparent,
                };
                let report = if let Some(canvas) = a.canvas {
                    crate::sprite_project::import_sheet_canvas(
                        &project,
                        &image,
                        a.cell_width,
                        a.cell_height,
                        &a.mapping,
                        &output,
                        crate::sprite_project::CanvasSheetOptions {
                            options,
                            canvas,
                            offsets: a.offsets,
                            replace_layers: a.replace_layers,
                        },
                    )?
                } else {
                    ensure!(
                        a.offsets == [0, 0] && !a.replace_layers,
                        "Offsets and layer replacement require an explicit canvas"
                    );
                    crate::sprite_project::import_sheet_options(
                        &project,
                        &image,
                        a.cell_width,
                        a.cell_height,
                        &a.mapping,
                        &output,
                        options,
                    )?
                };
                Ok(serde_json::to_value(report)?)
            }
            "castle_gate_preview" | "castle_gate_place" => {
                let a: CastleGate = serde_json::from_value(arguments)?;
                let mut db = self.db(a.database.as_deref())?;
                if name == "castle_gate_preview" {
                    Ok(serde_json::to_value(
                        crate::castle_gate_authoring::preview(&db, &self.assets, &a.placement)?,
                    )?)
                } else {
                    let output = a
                        .output
                        .context("Gate placement requires output revision")?;
                    self.output(&output)?;
                    let preview =
                        crate::castle_gate_authoring::apply(&mut db, &self.assets, &a.placement)?;
                    let mut result =
                        self.write_revision(&output, vec![("System.db", db.to_bytes()?)])?;
                    result["gate"] = serde_json::to_value(preview)?;
                    Ok(result)
                }
            }
            "sprite_build_batch" => {
                let a: SpriteBatch = serde_json::from_value(arguments)?;
                ensure!(
                    !a.projects.is_empty() && a.projects.len() <= 32,
                    "Batch needs 1–32 projects"
                );
                let projects = a
                    .projects
                    .iter()
                    .map(|p| self.sprite_project(p))
                    .collect::<Result<Vec<_>>>()?;
                ensure!(
                    projects
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        == projects.len(),
                    "Duplicate project in batch"
                );
                for project in &projects {
                    crate::sprite_authoring::validate(&self.assets, project)?;
                }
                let output = self.output(&a.output)?;
                std::fs::create_dir(&output)?;
                let result = (|| -> Result<Value> {
                    let mut entries = Vec::new();
                    for (index, project) in projects.iter().enumerate() {
                        let built = crate::sprite_project::build(
                            project,
                            &output.join(format!("pack-{index}")),
                        )?;
                        entries.push(json!({"source":a.projects[index],"identity":crate::sprite_authoring::identity(project)?,"pack":format!("pack-{index}/Library.Zl"),"changed_slots":built.changed_slots}));
                    }
                    let manifest = json!({"version":1,"packs":entries});
                    std::fs::write(
                        output.join("project-set.json"),
                        serde_json::to_vec_pretty(&manifest)?,
                    )?;
                    Ok(json!({"output":output,"manifest":manifest}))
                })();
                if result.is_err() {
                    let _ = std::fs::remove_dir_all(&output);
                }
                result
            }
            "sprite_tint" => {
                let a: SpriteTint = serde_json::from_value(arguments)?;
                Ok(serde_json::to_value(crate::sprite_project::tint(
                    &self.sprite_project(&a.project)?,
                    &self.output(&a.output)?,
                    &a.layer,
                    a.rgb,
                )?)?)
            }
            "sprite_paint" => {
                let a: SpritePaint = serde_json::from_value(arguments)?;
                Ok(serde_json::to_value(crate::sprite_project::paint(
                    &self.sprite_project(&a.project)?,
                    &self.output(&a.output)?,
                    a.padding,
                    &a.polygons,
                )?)?)
            }
            "sprite_paint_motion" => {
                let a: SpritePaintMotion = serde_json::from_value(arguments)?;
                Ok(serde_json::to_value(crate::sprite_project::paint_motion(
                    &self.sprite_project(&a.project)?,
                    &self.output(&a.output)?,
                    a.padding,
                    &a.slots,
                    &a.keyframes,
                )?)?)
            }
            "sprite_auto_motion" => {
                let a: SpriteAutoMotion = serde_json::from_value(arguments)?;
                Ok(serde_json::to_value(
                    crate::sprite_project::paint_auto_motion(
                        &self.sprite_project(&a.project)?,
                        &self.output(&a.output)?,
                        a.padding,
                        &a.slots,
                        &a.seed,
                        a.strength,
                    )?,
                )?)
            }
            "wearable_publish" => {
                let a: WearablePublish = serde_json::from_value(arguments)?;
                let projects = a
                    .projects
                    .iter()
                    .map(|p| self.sprite_project(p))
                    .collect::<Result<Vec<_>>>()?;
                crate::wearable_publish::publish(
                    &self.publish_assets(a.assets.as_deref())?,
                    &self.db(a.database.as_deref())?,
                    &projects,
                    a.source_id,
                    &a.name,
                    &self.output(&a.output)?,
                )
            }
            "equipment_publish" => {
                let a: WearablePublish = serde_json::from_value(arguments)?;
                let projects = a
                    .projects
                    .iter()
                    .map(|p| self.sprite_project(p))
                    .collect::<Result<Vec<_>>>()?;
                crate::wearable_publish::publish_equipment(
                    &self.publish_assets(a.assets.as_deref())?,
                    &self.db(a.database.as_deref())?,
                    &projects,
                    a.source_id,
                    &a.name,
                    &self.output(&a.output)?,
                )
            }
            "sprite_publish" => {
                let a: SpritePublish = serde_json::from_value(arguments)?;
                let project = self.sprite_project(&a.project)?;
                let output = self.output(&a.output)?;
                crate::sprite_authoring::validate(&self.assets, &project)?;
                let identity = crate::sprite_authoring::identity(&project)?;
                ensure!(
                    identity.kind == a.kind,
                    "Content kind differs from sprite project"
                );
                let kind = match a.kind.as_str() {
                    "item" => crate::sprite_publish::Kind::Item,
                    "monster" => crate::sprite_publish::Kind::Monster,
                    _ => anyhow::bail!("kind must be item or monster"),
                };
                let publish_assets = self.publish_assets(a.assets.as_deref())?;
                let db = self.db(a.database.as_deref())?;
                let temporary = self.root.join(format!(
                    ".sprite-build-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_nanos()
                ));
                let result = (|| {
                    let built = crate::sprite_project::build(&project, &temporary)?;
                    let library = built.library.context("Build returned no library")?;
                    Ok(serde_json::to_value(crate::sprite_publish::publish(
                        &publish_assets,
                        &db,
                        &project,
                        &library,
                        &crate::sprite_publish::Request {
                            kind,
                            source_id: a.source_id,
                            name: a.name,
                        },
                        &output,
                    )?)?)
                })();
                let _ = std::fs::remove_dir_all(&temporary);
                result
            }
            "content_search" => {
                let a: Search = serde_json::from_value(arguments)?;
                ensure!((1..=100).contains(&a.limit), "Limit must be 1–100.");
                ensure!(a.filter.len() <= 100, "Filter too long.");
                let db = self.db(a.database.as_deref())?;
                let (table, field) = match a.kind.as_str() {
                    "items" => ("ItemInfo", "ItemName"),
                    "monsters" => ("MonsterInfo", "MonsterName"),
                    "maps" => ("MapInfo", "FileName"),
                    _ => anyhow::bail!("kind must be items, monsters or maps"),
                };
                let c = db.collection(table).context("Collection missing")?;
                let filter = a.filter.to_lowercase();
                let matches: Vec<_> = c
                    .records
                    .iter()
                    .filter(|r| c.str_or(r, field, "").to_lowercase().contains(&filter))
                    .collect();
                Ok(
                    json!({"total":matches.len(),"entries":matches.iter().take(a.limit).map(|r| json!({"id":c.index(r),"name":c.str_or(r,field,""),"level":c.int_or(r,"Level",c.int_or(r,"RequiredAmount",0)),"image":c.int_or(r,"Image",0),"shape":c.int_or(r,"Shape",0),"item_type":c.int_or(r,"ItemType",0),"required_class":c.int_or(r,"RequiredClass",0),"required_gender":c.int_or(r,"RequiredGender",0)})).collect::<Vec<_>>()}),
                )
            }
            "draft_variant" => {
                let a: Variant = serde_json::from_value(arguments)?;
                self.output(&a.output)?;
                let mut db = self.db(a.database.as_deref())?;
                let (id, method) = match a.kind.as_str() {
                    "monster" => {
                        let d = monster_draft::propose(&db, a.source_id, a.level, &a.name)?;
                        (d.apply(&mut db)?, d.method)
                    }
                    "item" => {
                        let d = item_draft::propose(&db, a.source_id, a.level, &a.name)?;
                        (d.apply(&mut db)?, d.method)
                    }
                    _ => anyhow::bail!("kind must be monster or item"),
                };
                let private_assets =
                    self.private_assets_for_database(a.database.as_deref(), None)?;
                let mut result = self.write_database_revision(
                    &a.output,
                    db.to_bytes()?,
                    private_assets.as_deref(),
                )?;
                result["created_id"] = json!(id);
                result["method"] = json!(method);
                Ok(result)
            }
            "item_exterior_effect" => {
                let a: ItemExteriorEffect = serde_json::from_value(arguments)?;
                ensure!(
                    matches!(a.effect, 0..=3 | 10..=18 | 50..=53 | 100..=106 | 110..=112 | 120..=121 | 140 | 160 | 200..=203),
                    "Unsupported exterior effect."
                );
                let mut db = self.db(Some(&a.database))?;
                let items = db.collection_mut("ItemInfo").context("ItemInfo missing")?;
                let row = items
                    .records
                    .iter()
                    .position(|row| items.index(row) == a.item_id)
                    .context("Item ID missing")?;
                ensure!(
                    matches!(
                        items.int_or(&items.records[row], "ItemType", 0),
                        2 | 3 | 26 | 27
                    ),
                    "Exterior effects require armour, emblem, weapon or shield."
                );
                let mut record = items.records[row].clone();
                ensure!(
                    items.set(
                        &mut record,
                        "ExteriorEffect",
                        mir_formats::mirdb::Value::Int(i64::from(a.effect))
                    ),
                    "ItemInfo lacks ExteriorEffect."
                );
                items.records[row] = record;
                let private_assets = self.private_assets_for_database(Some(&a.database), None)?;
                self.write_database_revision(&a.output, db.to_bytes()?, private_assets.as_deref())
            }
            "map_inspect" => {
                let a: Inspect = serde_json::from_value(arguments)?;
                let (_, map) = self.map(&a.map)?;
                ensure!(a.points.len() <= 256, "Inspect at most 256 cells.");
                let cells: Result<Vec<_>>=a.points.iter().map(|&(x,y)| {let c=map.cell(x,y).context("Cell outside map")?; Ok(json!({"x":x,"y":y,"walkable":c.is_walkable(),"flag":c.flag,"back":[u32::from(c.back_file),u32::from(c.back_image)],"middle":[u32::from(c.middle_file),u32::from(c.middle_image)],"front":[u32::from(c.front_file),u32::from(c.front_image)]}))}).collect();
                Ok(
                    json!({"width":map.width,"height":map.height,"walkable_cells":map.walkable_count(),"cells":cells?}),
                )
            }
            "map_create" => {
                let a: Create = serde_json::from_value(arguments)?;
                self.output(&a.output)?;
                ensure!(
                    (2..=256).contains(&a.width)
                        && (2..=256).contains(&a.height)
                        && a.width.is_multiple_of(2)
                        && a.height.is_multiple_of(2),
                    "Dimensions must be even, 2–256."
                );
                let mut doc = Document::blank(a.width, a.height);
                if let Some(kit) = a.kit {
                    ensure!(
                        a.x % 2 == 0 && a.y % 2 == 0,
                        "Kit coordinates must be even."
                    );
                    let p = self.kit(&kit)?;
                    p.place(&mut doc, (a.x, a.y), None, Run::default())?;
                }
                self.save_map(&a.output, &doc.map, Metadata::default())
            }
            "map_copy" => {
                let a: CopyMap = serde_json::from_value(arguments)?;
                let (path, map) = self.map(&a.map)?;
                self.save_map(&a.output, &map, self.metadata(&path, &map)?)
            }
            "map_collision" => {
                let a: Collision = serde_json::from_value(arguments)?;
                let (path, map) = self.map(&a.map)?;
                let metadata = self.metadata(&path, &map)?;
                ensure!(
                    !a.points.is_empty() && a.points.len() <= 512,
                    "Edit 1–512 cells."
                );
                ensure!(
                    a.points.iter().all(|&(x, y)| map.cell(x, y).is_some()),
                    "Cell outside map."
                );
                let mut doc = Document::open(&path)?;
                doc.paint(
                    &a.points.into_iter().collect::<BTreeSet<_>>(),
                    Layer::Collision,
                    Tile {
                        file: 255,
                        image: 0,
                        animation: 0,
                    },
                    a.walkable,
                    0,
                    None,
                );
                self.save_map(&a.output, &doc.map, metadata)
            }
            "map_stamp" => {
                let a: StampKit = serde_json::from_value(arguments)?;
                ensure!(
                    a.x % 2 == 0 && a.y % 2 == 0,
                    "Kit coordinates must be even."
                );
                let (path, map) = self.map(&a.map)?;
                let mut doc = Document::open(&path)?;
                let metadata = self.metadata(&path, &map)?;
                self.kit(&a.kit)?.place(
                    &mut doc,
                    (a.x, a.y),
                    metadata.layout.as_ref().map(|(l, _)| l),
                    Run::default(),
                )?;
                self.save_map(&a.output, &doc.map, metadata)
            }
            "room_door_place" => {
                let a: RoomDoorPlace = serde_json::from_value(arguments)?;
                let output = self.output(&a.output)?;
                ensure!(
                    a.map.starts_with("project:") && a.database.starts_with("project:"),
                    "Room doors require a private map and database bundle."
                );
                let (map_path, map) = self.map(&a.map)?;
                let db_path = self.input(&a.database)?;
                ensure!(
                    map_path.parent().and_then(Path::parent) == db_path.parent(),
                    "Map and database must belong to the same bundle revision."
                );
                self.metadata(&map_path, &map)?;
                let db = self.db(Some(&a.database))?;
                let stem = map_path
                    .file_stem()
                    .context("Map needs a filename")?
                    .to_string_lossy();
                ensure!(
                    db.collection("MapInfo").is_some_and(|table| table
                        .records
                        .iter()
                        .any(|row| table.str_or(row, "FileName", "") == stem)),
                    "Map is not registered in this database."
                );
                ensure!(
                    db.collection("MonsterInfo").is_some_and(|table| table
                        .records
                        .iter()
                        .any(|row| table.index(row) == a.source_monster)),
                    "Room door source monster is absent from this database."
                );
                let mut door = crate::wood_doorway::inspect(&map, (a.center[0], a.center[1]))?;
                let mut layout = mir_formats::room_door::RoomDoors::load(&map_path, &map)?
                    .unwrap_or_else(mir_formats::room_door::RoomDoors::empty);
                layout.doors.push(mir_formats::room_door::RoomDoor {
                    center: a.center,
                    source_monster: a.source_monster,
                });
                layout.validate(&map)?;
                let inherited_assets = map_path.parent().unwrap().parent().unwrap().join("Client");
                let asset_source = if inherited_assets.is_dir() {
                    inherited_assets.as_path()
                } else {
                    self.assets.as_path()
                };
                let door_art = crate::wood_door_art::build(asset_source)?;
                let temporary =
                    self.root
                        .join(format!(".room-door-{}-{}", a.output, std::process::id()));
                std::fs::create_dir(&temporary)?;
                let result = (|| -> Result<()> {
                    std::fs::create_dir(temporary.join("Map"))?;
                    std::fs::copy(
                        &map_path,
                        temporary.join("Map").join(map_path.file_name().unwrap()),
                    )?;
                    for extension in ["cave.json", "navigation.json"] {
                        let source = map_path.with_extension(extension);
                        if source.exists() {
                            std::fs::copy(
                                &source,
                                temporary.join("Map").join(source.file_name().unwrap()),
                            )?;
                        }
                    }
                    std::fs::write(
                        temporary
                            .join("Map")
                            .join(map_path.with_extension("doors.json").file_name().unwrap()),
                        serde_json::to_vec_pretty(&layout)?,
                    )?;
                    std::fs::copy(&db_path, temporary.join("System.db"))?;
                    let override_path = PathBuf::from(crate::wood_door_art::LIBRARY);
                    let overrides = BTreeMap::from([(override_path.clone(), door_art)]);
                    crate::sprite_publish::asset_view(
                        asset_source,
                        asset_source,
                        &temporary.join("Client"),
                        &overrides,
                    )?;
                    std::fs::write(
                        temporary.join("Client").join(&override_path),
                        &overrides[&override_path],
                    )?;
                    std::fs::write(temporary.join("README.txt"), format!("Interactive Wood room door at ({}, {}). Use System.db and this revision's Map directory. Set ZIRCON_ASSETS={} to render its private Wood door art. Doors start closed and toggle by clicking within two cells.\n", a.center[0], a.center[1], output.join("Client").display()))?;
                    std::fs::rename(&temporary, &output)?;
                    Ok(())
                })();
                if result.is_err() {
                    let _ = std::fs::remove_dir_all(&temporary);
                }
                result?;
                door.interactive = true;
                Ok(
                    json!({"revision":a.output,"directory":output,"database":format!("project:{}/System.db",a.output),"map":format!("project:{}/Map/{}",a.output,map_path.file_name().unwrap().to_string_lossy()),"assets":output.join("Client"),"doorway":door,"doors":layout.doors.len()}),
                )
            }
            "map_copy_area" => {
                let a: CopyArea = serde_json::from_value(arguments)?;
                self.output(&a.output)?;
                ensure!(
                    a.x % 2 == 0 && a.y % 2 == 0,
                    "Target coordinates must be even."
                );
                let (_, source) = self.map(&a.source)?;
                let (path, target) = self.map(&a.map)?;
                let metadata = self.metadata(&path, &target)?;
                let [left, top, right, bottom] = a.bounds;
                ensure!(
                    (1..=64).contains(&(i64::from(right) - i64::from(left)))
                        && (1..=64).contains(&(i64::from(bottom) - i64::from(top))),
                    "Copy at most 64 cells per side."
                );
                crate::cave_art::validate_selection(
                    &source,
                    &self.assets,
                    (left, top, right, bottom),
                )?;
                let piece = Piece::capture(
                    &Stamp::capture(&source, (left, top), (right - 1, bottom - 1)),
                    "Copied area".into(),
                    Kind::Formation,
                    &self.assets,
                )?;
                let mut doc = Document::open(&path)?;
                piece.place(
                    &mut doc,
                    (a.x, a.y),
                    metadata.layout.as_ref().map(|(layout, _)| layout),
                    Run::default(),
                )?;
                self.save_map(&a.output, &doc.map, metadata)
            }
            "world_spawn" => self.spawn(serde_json::from_value(arguments)?),
            "world_from_brief" => {
                let a: WorldBrief = serde_json::from_value(arguments)?;
                let plan = parse_world_brief(&a.brief)?;
                let db = self.db(a.database.as_deref())?;
                let monsters = db
                    .collection("MonsterInfo")
                    .context("MonsterInfo missing")?;
                let requested = plan.monster.to_lowercase();
                let mut matches = monsters.records.iter().filter(|record| {
                    let name = monsters.str_or(record, "MonsterName", "").to_lowercase();
                    name == requested || format!("{name}s") == requested
                });
                let monster = matches.next().context(
                    "Monster name not found; use content_search for an exact installed name",
                )?;
                ensure!(
                    matches.next().is_none(),
                    "Monster name is ambiguous; use content_search and world_generate with an ID"
                );
                let monster_id = monsters.index(monster);
                let source = if plan.mode == "chamber" {
                    "asset:D1006.map"
                } else {
                    "asset:D1008.map"
                };
                let mut result = self.call(
                    "world_generate",
                    json!({
                        "source": source, "mode": plan.mode, "rooms": plan.rooms,
                        "width": a.width, "height": a.height, "seed": a.seed,
                        "monster_id": monster_id, "output": a.output,
                        "database": a.database, "assets": a.assets,
                    }),
                )?;
                result["interpreted"] = json!({"mode":plan.mode,"rooms":plan.rooms,"monster":monsters.str_or(monster,"MonsterName",""),"monster_id":monster_id,"source":source});
                Ok(result)
            }
            "world_generate" => {
                let a: Generate = serde_json::from_value(arguments)?;
                ensure!(
                    a.rooms.is_none() || a.mode == "rooms",
                    "rooms applies only to room dungeons"
                );
                ensure!(
                    !a.rooms.is_some_and(|rooms| !(2..=12).contains(&rooms)),
                    "Choose 2–12 rooms"
                );
                let folder = self.output(&a.output)?;
                ensure!(
                    (32..=256).contains(&a.width) && (32..=256).contains(&a.height),
                    "Generator dimensions must be 32–256."
                );
                let (source_path, source) = self.map(&a.source)?;
                let db = self.db(a.database.as_deref())?;
                let private_assets = self.private_assets_for_map_and_database(
                    self.metadata(&source_path, &source)?.private_assets,
                    a.database.as_deref(),
                    a.assets.as_deref(),
                )?;
                let source_assets = private_assets.as_deref().unwrap_or(&self.assets);
                let (doc, layout, wall_art) = match a.mode.as_str() {
                    "chamber" => {
                        let (doc, layout) = generate::enclosed_chamber(
                            &source,
                            a.width,
                            a.height,
                            a.seed,
                            source_assets,
                        )?;
                        (doc, layout, None)
                    }
                    "organic" => {
                        let (d, l, _, art, walls) = generate::cave_with_full_art(
                            &source,
                            a.width,
                            a.height,
                            a.seed,
                            source_assets,
                        )?;
                        (d, l, Some((art, walls)))
                    }
                    "rooms" => {
                        let (d, l, _, art, walls) = crate::dungeon::generate_full_art(
                            &source,
                            a.width,
                            a.height,
                            a.seed,
                            a.rooms.unwrap_or(6),
                            2,
                            source_assets,
                        )?;
                        (d, l, Some((art, walls)))
                    }
                    _ => anyhow::bail!("mode must be chamber, organic or rooms"),
                };
                let wall_cells = wall_art.as_ref().map(|(_, count)| *count);
                let result = (|| -> Result<Value> {
                    generate::export(&folder, &doc.map, &db, &layout, a.monster_id)?;
                    let assets = if let Some((library, _)) = wall_art {
                        let overrides =
                            BTreeMap::from([(PathBuf::from(crate::wood_branch::LIBRARY), library)]);
                        let assets = self.private_asset_view(source_assets, &folder, &overrides)?;
                        let built = mir_formats::zl::ZlLibrary::open(
                            assets.join(crate::wood_branch::LIBRARY),
                        )?;
                        let source = mir_formats::zl::ZlLibrary::open(
                            source_assets.join(crate::wood_branch::LIBRARY),
                        )?;
                        ensure!(
                            built.len() >= source.len() + crate::wood_branch::NEW_IMAGES + 2,
                            "Private freeform wall art is incomplete"
                        );
                        Some(assets)
                    } else {
                        private_assets
                            .as_deref()
                            .map(|source| self.inherit_private_assets(source, &folder))
                            .transpose()?
                    };
                    Ok(
                        json!({"revision":a.output,"directory":folder,"database":format!("project:{}/System.db",a.output),"assets":assets,"start":layout.start,"spawn_cells":layout.spawns,"wall_cells":wall_cells,"note":"Read README.txt in the bundle for server/client launch settings."}),
                    )
                })();
                if result.is_err() {
                    let _ = std::fs::remove_dir_all(&folder);
                }
                result
            }
            "world_branch" => {
                let a: BranchWorld = serde_json::from_value(arguments)?;
                let folder = self.output(&a.output)?;
                let db = self.db(a.database.as_deref())?;
                let source_assets = self
                    .private_assets_for_database(a.database.as_deref(), a.assets.as_deref())?
                    .unwrap_or_else(|| self.assets.clone());
                let built = if let (Some(nodes), Some(links)) = (&a.nodes, &a.links) {
                    ensure!(
                        a.shape.is_none() && a.arms.is_none(),
                        "Graph nodes/links cannot be combined with shape/arms."
                    );
                    crate::wood_branch::generate_grid_graph(
                        &source_assets,
                        a.seed,
                        nodes,
                        links,
                        a.organic,
                    )?
                } else {
                    ensure!(
                        a.nodes.is_none() && a.links.is_none(),
                        "Graph needs both nodes and links."
                    );
                    let shape = a.shape.unwrap_or(crate::wood_branch::Shape::T);
                    let defaults = [5; 4];
                    let arms = a.arms.as_deref().unwrap_or(
                        &defaults[..if shape == crate::wood_branch::Shape::Cross {
                            4
                        } else {
                            3
                        }],
                    );
                    if shape == crate::wood_branch::Shape::T && !a.organic {
                        if let Some(arms) = &a.arms {
                            let arms: [u8; 3] = arms
                                .as_slice()
                                .try_into()
                                .context("T shape needs three arms.")?;
                            crate::wood_branch::generate_with_arms(&source_assets, a.seed, arms)?
                        } else {
                            crate::wood_branch::generate(&source_assets, a.seed)?
                        }
                    } else {
                        crate::wood_branch::generate_shape(
                            &source_assets,
                            a.seed,
                            shape,
                            arms,
                            a.organic,
                        )?
                    }
                };
                let result = (|| -> Result<Value> {
                    generate::export(&folder, &built.doc.map, &db, &built.layout, a.monster_id)?;
                    let overrides = BTreeMap::from([(
                        PathBuf::from(crate::wood_branch::LIBRARY),
                        built.library,
                    )]);
                    let private_assets =
                        self.private_asset_view(&source_assets, &folder, &overrides)?;
                    ensure!(
                        mir_formats::zl::ZlLibrary::open(
                            private_assets.join(crate::wood_branch::LIBRARY)
                        )?
                        .len()
                            >= usize::from(built.first_new_image) + crate::wood_branch::NEW_IMAGES,
                        "Private Wood wall art is incomplete."
                    );
                    Ok(json!({"revision":a.output,"directory":folder,
                        "database":format!("project:{}/System.db",a.output),
                        "assets":private_assets,"arms":built.arms,
                        "shape":a.shape,"organic":a.organic,"nodes":a.nodes,"links":a.links,
                        "centers":built.centers,
                        "first_new_image":built.first_new_image,
                        "start":built.layout.start,"width":built.doc.map.width,
                        "height":built.doc.map.height}))
                })();
                if result.is_err() {
                    let _ = std::fs::remove_dir_all(&folder);
                }
                result
            }
            "world_perimeter" => {
                let a: Perimeter = serde_json::from_value(arguments)?;
                let folder = self.output(&a.output)?;
                let db = self.db(a.database.as_deref())?;
                let private_assets =
                    self.private_assets_for_database(a.database.as_deref(), a.assets.as_deref())?;
                let source_assets = private_assets.as_deref().unwrap_or(&self.assets);
                let (doc, layout, necks) = if let Some(repeats) = &a.repeats {
                    ensure!(
                        a.nw.is_none() && a.ne.is_none(),
                        "Use repeats or nw/ne, not both."
                    );
                    ensure!(
                        a.rooms
                            .is_none_or(|rooms| usize::from(rooms) == repeats.len()),
                        "rooms must match repeats length."
                    );
                    crate::wood_connected::generate_varied_with_necks(
                        source_assets,
                        repeats,
                        a.seed,
                    )?
                } else {
                    let nw = a.nw.context("nw is required without repeats")?;
                    let ne = a.ne.context("ne is required without repeats")?;
                    match a.rooms.unwrap_or(1) {
                        1 => {
                            let (doc, layout) =
                                crate::wood_perimeter::generate(source_assets, nw, ne, a.seed)?;
                            (doc, layout, vec![])
                        }
                        rooms @ 2..=3 => crate::wood_connected::generate_rooms_with_necks(
                            source_assets,
                            nw,
                            ne,
                            rooms,
                            a.seed,
                        )?,
                        _ => anyhow::bail!("rooms must be 1, 2 or 3"),
                    }
                };
                let doorways = necks
                    .into_iter()
                    .map(|neck| crate::wood_doorway::inspect(&doc.map, neck))
                    .collect::<Result<Vec<_>>>()?;
                let result = (|| -> Result<Value> {
                    generate::export(&folder, &doc.map, &db, &layout, a.monster_id)?;
                    let assets = private_assets
                        .as_deref()
                        .map(|source| self.inherit_private_assets(source, &folder))
                        .transpose()?;
                    Ok(json!({"revision":a.output,"directory":folder,
                    "database":format!("project:{}/System.db",a.output),
                    "assets":assets,
                    "width":doc.map.width,"height":doc.map.height,
                    "wall_repeats":layout.wall_repeats,"wall_room_repeats":layout.wall_room_repeats,
                    "wall_rooms":layout.wall_rooms,"enclosed":layout.enclosed,"doorways":doorways,
                    "start":layout.start,"spawn_cells":layout.spawns}))
                })();
                if result.is_err() {
                    let _ = std::fs::remove_dir_all(&folder);
                }
                result
            }
            "world_populate" => {
                let a: Populate = serde_json::from_value(arguments)?;
                let folder = self.output(&a.output)?;
                let (path, map) = self.map(&a.map)?;
                let db = self.db(a.database.as_deref())?;
                let metadata = self.metadata(&path, &map)?;
                let room_doors = metadata.room_doors;
                let private_assets = self.private_assets_for_map_and_database(
                    metadata.private_assets,
                    a.database.as_deref(),
                    a.assets.as_deref(),
                )?;
                if let Some(doors) = &room_doors {
                    let monsters = db
                        .collection("MonsterInfo")
                        .context("Room-door database lacks MonsterInfo")?;
                    for door in &doors.doors {
                        ensure!(
                            monsters
                                .records
                                .iter()
                                .any(|row| monsters.index(row) == door.source_monster),
                            "Room door source monster is absent from this database."
                        );
                    }
                }
                let layout = match metadata.layout {
                    Some((layout, _)) => layout,
                    None => {
                        ensure!(metadata.navigation.is_none(), "Map has standalone navigation. Provide matching layout metadata before population to preserve its portals.");
                        generate::layout(&map, a.seed)?
                    }
                };
                let result = (|| -> Result<()> {
                    generate::export(&folder, &map, &db, &layout, a.monster_id)?;
                    if let Some(doors) = room_doors {
                        doors.validate(&map)?;
                        let map_path = std::fs::read_dir(folder.join("Map"))?
                            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                            .find(|path| path.extension().is_some_and(|ext| ext == "map"))
                            .context("Exported map is missing")?;
                        std::fs::write(
                            map_path.with_extension("doors.json"),
                            serde_json::to_vec_pretty(&doors)?,
                        )?;
                    }
                    if let Some(source) = private_assets {
                        self.inherit_private_assets(&source, &folder)?;
                    }
                    Ok(())
                })();
                if result.is_err() {
                    let _ = std::fs::remove_dir_all(&folder);
                }
                result?;
                Ok(
                    json!({"revision":a.output,"directory":folder,"database":format!("project:{}/System.db",a.output),"assets":folder.join("Client").is_dir().then(|| folder.join("Client")),"start":layout.start,"spawn_cells":layout.spawns}),
                )
            }
            _ => anyhow::bail!("Unknown authoring tool: {name}"),
        }
    }
    fn kit(&self, name: &str) -> Result<Piece> {
        let pieces = match name {
            "wood-north" => wall_kit::wood_corner(&self.assets)?,
            "wood-south" => wood_corners::south_corner(&self.assets)?,
            "palace-doorway" => vec![crate::palace_kit::doorway(&self.assets)?],
            "wood-entry" => vec![crate::wood_entry::entry(&self.assets)?],
            "wood-wall-junction" => vec![crate::wood_wall_entry::entry(&self.assets)?],
            _ => anyhow::bail!("Unknown kit"),
        };
        piece_assembly::assemble(&pieces, &self.assets)
    }

    fn spawn(&self, a: Spawn) -> Result<Value> {
        use mir_formats::mirdb::Value as DbValue;
        self.output(&a.output)?;
        ensure!(
            (1..=128).contains(&a.points.len()),
            "Choose 1–128 spawn points."
        );
        ensure!((1..=100).contains(&a.count), "Spawn count must be 1–100.");
        ensure!(
            (1..=3600).contains(&a.delay_seconds),
            "Spawn delay must be 1–3600 seconds."
        );
        let (path, map) = self.map(&a.map)?;
        let metadata = self.metadata(&path, &map)?;
        ensure!(
            a.points.iter().all(|&(x, y)| map.is_walkable(x, y)),
            "Spawn points must be walkable cells inside the map."
        );
        let points: BTreeSet<_> = a.points.into_iter().collect();
        let mut db = self.db(Some(&a.database))?;
        for (table, fields) in [
            ("MapInfo", &["Index", "FileName"][..]),
            ("MonsterInfo", &["Index"][..]),
            (
                "MapRegion",
                &["Index", "Map", "Description", "PointRegion", "BitRegion"][..],
            ),
            (
                "RespawnInfo",
                &["Index", "Region", "Monster", "Count", "Delay"][..],
            ),
            ("SafeZoneInfo", &["Region", "BindRegion"][..]),
            ("MovementInfo", &["SourceRegion", "DestinationRegion"][..]),
        ] {
            let collection = db
                .collection(table)
                .with_context(|| format!("Missing {table}"))?;
            for field in fields {
                ensure!(
                    collection.property_index(field).is_some(),
                    "Missing {table}.{field}"
                );
            }
        }
        let maps = db.collection("MapInfo").unwrap();
        let record = maps
            .records
            .iter()
            .find(|row| maps.index(row) == a.map_id)
            .context("Map ID is not registered in this database")?;
        let file = path
            .file_stem()
            .and_then(|name| name.to_str())
            .context("Map filename is not UTF-8")?;
        ensure!(
            maps.str_or(record, "FileName", "")
                .eq_ignore_ascii_case(file),
            "Map ID filename does not match the supplied map."
        );
        let monsters = db.collection("MonsterInfo").unwrap();
        ensure!(
            monsters
                .records
                .iter()
                .any(|row| monsters.index(row) == a.monster_id),
            "Monster ID is not registered in this database."
        );
        let mut protected = BTreeSet::new();
        if let Some((layout, _)) = &metadata.layout {
            protected.extend(layout.safe.iter().copied());
            protected.extend(layout.exits);
            protected.extend(layout.landings);
        }
        if let Some(navigation) = &metadata.navigation {
            for portal in &navigation.portals {
                protected.insert(portal.position);
                protected.insert(portal.destination);
            }
        }
        let mut protected_regions = BTreeSet::new();
        for (table, fields) in [
            ("SafeZoneInfo", ["Region", "BindRegion"]),
            ("MovementInfo", ["SourceRegion", "DestinationRegion"]),
        ] {
            let collection = db.collection(table).unwrap();
            for row in &collection.records {
                for field in fields {
                    protected_regions.insert(collection.int_or(row, field, -1));
                }
            }
        }
        let regions = db.collection("MapRegion").unwrap();
        for row in &regions.records {
            if regions.int_or(row, "Map", -1) != i64::from(a.map_id)
                || !protected_regions.contains(&i64::from(regions.index(row)))
            {
                continue;
            }
            for &(x, y) in &points {
                let contained = match regions.get(row, "BitRegion") {
                    Some(DbValue::BitArray(Some(bits))) => {
                        let index = (y * i32::from(map.width) + x) as usize;
                        bits.get(index / 8)
                            .is_some_and(|byte| byte & (1 << (index % 8)) != 0)
                    }
                    _ => {
                        matches!(regions.get(row,"PointRegion"), Some(DbValue::PointArray(Some(cells))) if cells.contains(&(x,y)))
                    }
                };
                if contained {
                    protected.insert((x, y));
                }
            }
        }
        ensure!(
            points.is_disjoint(&protected),
            "Spawn points overlap a safe zone, portal trigger or landing."
        );
        for table in ["MapRegion", "RespawnInfo"] {
            let collection = db.collection(table).unwrap();
            let next = collection
                .records
                .iter()
                .map(|row| collection.index(row))
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .context("Database identities exhausted")?
                .max(collection.next_index);
            ensure!(next < i32::MAX, "Database identities exhausted.");
        }
        let region = db.collection_mut("MapRegion").unwrap().push_record(vec![
            ("Map", DbValue::Int(a.map_id.into())),
            (
                "Description",
                DbValue::Str(format!("Authored spawn {}", a.output)),
            ),
            (
                "PointRegion",
                DbValue::PointArray(Some(points.iter().copied().collect())),
            ),
        ]);
        let spawn = db.collection_mut("RespawnInfo").unwrap().push_record(vec![
            ("Region", DbValue::Int(region.into())),
            ("Monster", DbValue::Int(a.monster_id.into())),
            ("Count", DbValue::Int(a.count.into())),
            ("Delay", DbValue::Int(a.delay_seconds.into())),
        ]);
        let private_assets = self.private_assets_for_map_and_database(
            metadata.private_assets,
            Some(&a.database),
            a.assets.as_deref(),
        )?;
        let mut result =
            self.write_database_revision(&a.output, db.to_bytes()?, private_assets.as_deref())?;
        let database = self.root.join(&a.output).join("System.db");
        result["database"] = json!(format!("project:{}/System.db", a.output));
        result["map"] = json!(a.map);
        result["map_id"] = json!(a.map_id);
        result["region_id"] = json!(region);
        result["spawn_id"] = json!(spawn);
        result["points"] = json!(points);
        let mut launch = json!({"server_args":["--db",database.to_str().context("Project path is not UTF-8")?],
            "ZIRCON_MAP_DIR":path.parent(),"client_args":["--map",file],
            "note":"Use this database snapshot with the existing map directory; no map was copied or changed."});
        if let Some(assets) = result.get("assets") {
            launch["ZIRCON_ASSETS"] = assets.clone();
        }
        result["launch"] = launch;
        Ok(result)
    }
}
fn limit() -> usize {
    50
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Search {
    kind: String,
    #[serde(default)]
    filter: String,
    #[serde(default = "limit")]
    limit: usize,
    #[serde(default)]
    database: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Variant {
    kind: String,
    source_id: i32,
    level: i32,
    name: String,
    output: String,
    #[serde(default)]
    database: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemExteriorEffect {
    database: String,
    item_id: i32,
    effect: u8,
    output: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inspect {
    map: String,
    #[serde(default)]
    points: Vec<(i32, i32)>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    output: String,
    width: u16,
    height: u16,
    #[serde(default)]
    kit: Option<String>,
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyMap {
    map: String,
    output: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Collision {
    map: String,
    output: String,
    points: Vec<(i32, i32)>,
    walkable: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StampKit {
    map: String,
    output: String,
    kit: String,
    x: i32,
    y: i32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Generate {
    source: String,
    mode: String,
    width: u16,
    height: u16,
    seed: u64,
    monster_id: i32,
    output: String,
    #[serde(default)]
    rooms: Option<usize>,
    #[serde(default)]
    database: Option<String>,
    #[serde(default)]
    assets: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldBrief {
    brief: String,
    output: String,
    #[serde(default)]
    seed: u64,
    #[serde(default = "default_brief_size")]
    width: u16,
    #[serde(default = "default_brief_size")]
    height: u16,
    #[serde(default)]
    database: Option<String>,
    #[serde(default)]
    assets: Option<String>,
}
fn default_brief_size() -> u16 {
    80
}
struct WorldPlan {
    mode: &'static str,
    rooms: Option<usize>,
    monster: String,
}
fn parse_world_brief(brief: &str) -> Result<WorldPlan> {
    ensure!(
        !brief.trim().is_empty() && brief.len() <= 4000,
        "Brief must be 1–4000 nonblank characters"
    );
    let words: Vec<_> = brief
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    let separator = words.iter().position(|word| word == "with" || word == "featuring")
        .context("Name a monster after 'with' or 'featuring', e.g. 'six-room Wood dungeon with chickens'")?;
    let (shape, tail) = words.split_at(separator);
    ensure!(
        !tail[1..].iter().any(|word| matches!(word.as_str(), "and" | "plus" | "including")),
        "Brief supports one monster and no extra content; use the create_world prompt for richer briefs"
    );
    let monster = tail[1..].join(" ");
    ensure!(
        !monster.is_empty(),
        "Name a monster after 'with' or 'featuring'"
    );
    let room_mode = shape
        .iter()
        .any(|word| matches!(word.as_str(), "dungeon" | "maze" | "room" | "rooms"));
    let cave_mode = shape
        .iter()
        .any(|word| matches!(word.as_str(), "cave" | "cavern"));
    let chamber_mode = shape
        .iter()
        .any(|word| matches!(word.as_str(), "chamber" | "arena"));
    ensure!(
        u8::from(room_mode) + u8::from(cave_mode) + u8::from(chamber_mode) == 1,
        "Choose one supported shape: room dungeon, organic cave, or chamber"
    );
    let mut rooms = None;
    for (index, word) in shape.iter().enumerate() {
        if matches!(word.as_str(), "room" | "rooms") && index > 0 {
            rooms = match shape[index - 1].as_str() {
                "two" => Some(2),
                "three" => Some(3),
                "four" => Some(4),
                "five" => Some(5),
                "six" => Some(6),
                "seven" => Some(7),
                "eight" => Some(8),
                "nine" => Some(9),
                "ten" => Some(10),
                "eleven" => Some(11),
                "twelve" => Some(12),
                other => other.parse::<usize>().ok(),
            };
        }
    }
    ensure!(
        !rooms.is_some_and(|count| !(2..=12).contains(&count)),
        "Choose 2–12 rooms"
    );
    ensure!(
        rooms.is_none() || room_mode,
        "Room count requires a room dungeon"
    );
    for (index, word) in shape.iter().enumerate() {
        let count = index + 1 < shape.len()
            && matches!(shape[index + 1].as_str(), "room" | "rooms")
            && rooms.is_some();
        ensure!(
            count
                || matches!(
                    word.as_str(),
                    "a" | "an"
                        | "the"
                        | "create"
                        | "make"
                        | "build"
                        | "generate"
                        | "please"
                        | "playable"
                        | "map"
                        | "wood"
                        | "wooden"
                        | "room"
                        | "rooms"
                        | "dungeon"
                        | "maze"
                        | "cave"
                        | "cavern"
                        | "organic"
                        | "chamber"
                        | "arena"
                        | "enclosed"
                ),
            "Unsupported map detail '{word}'; use the create_world prompt for richer briefs"
        );
    }
    ensure!(
        !shape.iter().any(|word| word == "organic") || cave_mode,
        "'organic' currently describes caves only"
    );
    Ok(WorldPlan {
        mode: if room_mode {
            "rooms"
        } else if cave_mode {
            "organic"
        } else {
            "chamber"
        },
        rooms,
        monster,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BranchWorld {
    seed: u64,
    monster_id: i32,
    output: String,
    #[serde(default)]
    shape: Option<crate::wood_branch::Shape>,
    #[serde(default)]
    arms: Option<Vec<u8>>,
    #[serde(default)]
    organic: bool,
    #[serde(default)]
    nodes: Option<Vec<[u8; 2]>>,
    #[serde(default)]
    links: Option<Vec<[usize; 2]>>,
    #[serde(default)]
    database: Option<String>,
    #[serde(default)]
    assets: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Populate {
    map: String,
    seed: u64,
    monster_id: i32,
    output: String,
    #[serde(default)]
    database: Option<String>,
    #[serde(default)]
    assets: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Perimeter {
    rooms: Option<u8>,
    nw: Option<u16>,
    ne: Option<u16>,
    repeats: Option<Vec<[u16; 2]>>,
    seed: u64,
    monster_id: i32,
    output: String,
    #[serde(default)]
    database: Option<String>,
    #[serde(default)]
    assets: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoomDoorPlace {
    map: String,
    database: String,
    center: [i32; 2],
    source_monster: i32,
    output: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyArea {
    source: String,
    map: String,
    bounds: [i32; 4],
    x: i32,
    y: i32,
    output: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spawn {
    map: String,
    map_id: i32,
    database: String,
    monster_id: i32,
    points: Vec<(i32, i32)>,
    count: u16,
    delay_seconds: u16,
    output: String,
    #[serde(default)]
    assets: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CastleGate {
    placement: crate::castle_gate_authoring::Request,
    database: Option<String>,
    output: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpriteBatch {
    projects: Vec<String>,
    output: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpriteTint {
    project: String,
    layer: String,
    rgb: [u8; 3],
    output: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpritePaint {
    project: String,
    output: String,
    #[serde(default)]
    padding: [u8; 4],
    polygons: Vec<crate::sprite_project::PaintPolygon>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpritePaintMotion {
    project: String,
    output: String,
    #[serde(default)]
    padding: [u8; 4],
    slots: Vec<usize>,
    keyframes: Vec<crate::sprite_project::MotionKeyframe>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpriteAutoMotion {
    project: String,
    output: String,
    #[serde(default)]
    padding: [u8; 4],
    slots: Vec<usize>,
    seed: crate::sprite_project::MotionKeyframe,
    strength: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WearablePublish {
    projects: Vec<String>,
    source_id: i32,
    name: String,
    output: String,
    database: Option<String>,
    assets: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpriteExport {
    kind: String,
    source_image: u16,
    output: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpritePoseReference {
    kind: String,
    source_image: u16,
    donor_image: u16,
    output: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpriteInput {
    project: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpriteOutput {
    project: String,
    output: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpriteImage {
    #[serde(default)]
    trim_transparent: bool,
    #[serde(default)]
    canvas: Option<[u16; 2]>,
    #[serde(default)]
    offsets: Option<[i16; 2]>,
    project: String,
    image: String,
    slot: usize,
    layer: String,
    #[serde(default)]
    fit: bool,
    #[serde(default)]
    binary_alpha: bool,
    output: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpriteSheet {
    #[serde(default)]
    trim_transparent: bool,
    #[serde(default)]
    fit: bool,
    #[serde(default)]
    binary_alpha: bool,
    #[serde(default)]
    canvas: Option<[u16; 2]>,
    #[serde(default)]
    offsets: [i16; 2],
    #[serde(default)]
    replace_layers: bool,
    project: String,
    image: String,
    cell_width: u16,
    cell_height: u16,
    mapping: Vec<crate::sprite_project::SheetCell>,
    output: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpritePublish {
    #[serde(default)]
    assets: Option<String>,
    project: String,
    kind: String,
    source_id: i32,
    name: String,
    output: String,
    #[serde(default)]
    database: Option<String>,
}

pub fn tools() -> Vec<Value> {
    let string = json!({"type":"string"});
    let integer = json!({"type":"integer"});
    let points = json!({"type":"array","maxItems":512,"items":{"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2}});
    let mut result = vec![];
    let mut add = |name: &str, description: &str, props: Value, required: Vec<&str>, read: bool| {
        result.push(json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":props,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":read,"destructiveHint":false,"idempotentHint":read,"openWorldHint":false}}))
    };
    add("content_search","Search maps, monsters or items by name. Optional database is a project: revision path; default is the read-only source database.",json!({"kind":{"enum":["maps","monsters","items"]},"filter":string,"limit":{"type":"integer","minimum":1,"maximum":100},"database":string}),vec!["kind"],true);
    add("draft_variant","Create an item or monster variant using the editor's level estimator; writes a NEW database revision. Retains source appearance. Monster drops are not copied.",json!({"kind":{"enum":["monster","item"]},"source_id":integer,"level":integer,"name":string,"output":string,"database":string}),vec!["kind","source_id","level","name","output"],false);
    add("item_exterior_effect","Set an armour, emblem, weapon or shield exterior effect in a NEW database revision. Use a project: System.db from a drafted or published item. Effect IDs follow C# ExteriorEffect; 0 removes the effect.",json!({"database":string,"item_id":integer,"effect":{"type":"integer","minimum":0,"maximum":203},"output":string}),vec!["database","item_id","effect","output"],false);
    add("map_inspect","Inspect dimensions, walkability and optional cell references. Use asset:filename.map or project:revision/Map.map.",json!({"map":string,"points":points}),vec!["map"],true);
    add("map_create","Create an even-sized editable map, optionally stamp an assembled authored kit. Wood wall junction is a static passable threshold with four wall seams.",json!({"output":string,"width":integer,"height":integer,"kit":{"enum":["wood-north","wood-south","wood-entry","wood-wall-junction","palace-doorway"]},"x":integer,"y":integer}),vec!["output","width","height"],false);
    add("map_copy","Copy an asset or project map into a NEW revision, retaining valid generated layout/navigation metadata.",json!({"map":string,"output":string}),vec!["map","output"],false);
    add("map_collision","Change walkability at explicit points and write a NEW revision. Existing generated layout invariants must remain valid.",json!({"map":string,"output":string,"points":points,"walkable":{"type":"boolean"}}),vec!["map","output","points","walkable"],false);
    add("map_stamp","Place an assembled source-art kit on a map and write a NEW revision. Preserves generated navigation and validates placement.",json!({"map":string,"output":string,"kit":{"enum":["wood-north","wood-south","wood-entry","wood-wall-junction","palace-doorway"]},"x":integer,"y":integer}),vec!["map","output","kit","x","y"],false);
    add("room_door_place","Add a clickable Wood room door at a checked connected-room threshold. Copies a private map, database and Wood door art into one new bundle. Doors start closed and toggle when clicked within two cells.",json!({"map":string,"database":string,"center":{"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2},"source_monster":integer,"output":string}),vec!["map","database","center","source_monster","output"],false);
    add("map_copy_area","Copy a complete source-art selection to a target map and create a NEW revision. bounds=[left,top,right,bottom] uses half-open even coordinates, at most 64 cells per side; x/y are even target coordinates. Validates full sprite frames and target navigation; review larger visual motifs in the editor.",json!({"source":string,"map":string,"bounds":{"type":"array","items":{"type":"integer"},"minItems":4,"maxItems":4},"x":integer,"y":integer,"output":string}),vec!["source","map","bounds","x","y","output"],false);
    add("world_from_brief","Create a private playable map from a simple English brief: 'six-room Wood dungeon with chickens', 'organic Wood cave with chickens', or 'Wood chamber with chickens'. Monster must match an installed name (simple plural accepted). Default size 80x80 and seed 0. Rejects unsupported details; use the create_world prompt for richer requests.",json!({"brief":string,"output":string,"seed":integer,"width":{"type":"integer","minimum":32,"maximum":256},"height":{"type":"integer","minimum":32,"maximum":256},"database":string,"assets":string}),vec!["brief","output"],false);
    add("world_generate","Generate a playable map bundle with database, safe zone, portals and eight selected-monster spawns. Chamber retains authored source walls; organic caves and room dungeons export complete private Wood boundary art, with passable room threshold strips. Optional rooms=2–12 for room dungeons; default 6.",json!({"source":string,"mode":{"enum":["chamber","organic","rooms"]},"width":integer,"height":integer,"seed":integer,"monster_id":integer,"rooms":{"type":"integer","minimum":2,"maximum":12},"output":string,"database":string,"assets":string}),vec!["source","mode","width","height","seed","monster_id","output"],false);
    add("world_branch","Generate a playable Wood map with private wall art. Presets: t, y or cross, optional arms of length 2–8. For a connected 2–12-room graph on a 5×5 grid, provide unique nodes=[column,row] and cardinal links=[node_index,node_index]. organic=true indents exposed room edges. Optional private assets follow a sibling Client beside database or explicit project:assets path.",json!({"seed":integer,"monster_id":integer,"output":string,"database":string,"assets":string,"shape":{"enum":["t","y","cross"]},"arms":{"type":"array","minItems":3,"maxItems":4,"items":{"type":"integer","minimum":2,"maximum":8}},"organic":{"type":"boolean"},"nodes":{"type":"array","minItems":2,"maxItems":12,"items":{"type":"array","items":{"type":"integer","minimum":0,"maximum":4},"minItems":2,"maxItems":2}},"links":{"type":"array","minItems":1,"maxItems":20,"items":{"type":"array","items":{"type":"integer","minimum":0,"maximum":11},"minItems":2,"maxItems":2}}}),vec!["seed","monster_id","output"],false);
    add("world_perimeter","Generate one Wood room with nw/ne, or two to three connected rooms using uniform nw/ne or varied repeats [[nw,ne],...]. Complete source walls frame static passable thresholds; doorways report their checked footprints. No open/close interaction. At most 64 wall placements; each repeat 3–10. Writes a NEW playable bundle.",json!({"rooms":{"type":"integer","enum":[1,2,3]},"nw":{"type":"integer","minimum":3,"maximum":10},"ne":{"type":"integer","minimum":3,"maximum":10},"repeats":{"type":"array","minItems":2,"maxItems":3,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"integer","minimum":3,"maximum":10}}},"seed":integer,"monster_id":integer,"output":string,"database":string,"assets":string}),vec!["seed","monster_id","output"],false);
    add("world_populate","Register a map in a NEW playable database bundle with safe zone, paired portals and eight spawns of the selected monster. Retains existing generated layout; seed applies when no layout exists. Requires enough connected floor.",json!({"map":string,"seed":integer,"monster_id":integer,"output":string,"database":string,"assets":string}),vec!["map","seed","monster_id","output"],false);
    add("world_spawn","Add a monster spawn region at explicit walkable points to a NEW database revision. Requires a project database and matching registered map ID/filename; rejects safe zones and portal cells. count is the total population over the region, not per point. Retains private art and returns existing map directory launch settings.",json!({"map":string,"map_id":integer,"database":string,"monster_id":integer,"points":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2}},"count":{"type":"integer","minimum":1,"maximum":100},"delay_seconds":{"type":"integer","minimum":1,"maximum":3600},"output":string,"assets":string}),vec!["map","map_id","database","monster_id","points","count","delay_seconds","output"],false);
    for name in ["sprite_export_project", "sprite_reference"] {
        add(name,"Export selected sprite as an editable PNG project with source animation contract and reference sheet. Source remains unchanged. Equipped kinds use ItemInfo.Shape as source_image.",json!({"kind":{"enum":["item","monster","armour-male","armour-female","armour-assassin-male","armour-assassin-female","weapon-male","weapon-female","helmet-male","helmet-female","helmet-assassin-male","helmet-assassin-female","shield-male","shield-female","shield-assassin-male","shield-assassin-female"]},"source_image":integer,"output":string}),vec!["kind","source_image","output"],false);
    }
    add("sprite_pose_from_reference","Create a complete editable armour animation by transferring hand-drawn full-body poses from another original appearance in the same family. The source item identity remains unchanged; edit the returned PNGs before publication.",json!({"kind":{"enum":["armour-male","armour-female","armour-assassin-male","armour-assassin-female"]},"source_image":integer,"donor_image":integer,"output":string}),vec!["kind","source_image","donor_image","output"],false);
    add(
        "sprite_validate_project",
        "Validate PNGs, frame slots, source identity and the game animation contract.",
        json!({"project":string}),
        vec!["project"],
        true,
    );
    for name in ["sprite_build_pack", "sprite_preview_sheet"] {
        add(
            name,
            "Validate sprite project and create a new built library or preview sheet revision.",
            json!({"project":string,"output":string}),
            vec!["project", "output"],
            false,
        );
    }
    add("sprite_import_image","Import a project PNG into a frame in a NEW revision. fit=false requires exact dimensions; fit=true aspect-fits within the existing canvas and preserves offsets. binary_alpha=true explicitly thresholds alpha at128 for DXT1 packs. trim_transparent=true fits visible artwork instead of empty margins (requires fit). Optional canvas and offsets define a new image canvas.",json!({"project":string,"image":string,"slot":integer,"layer":{"enum":["image","shadow","overlay"]},"fit":{"type":"boolean"},"binary_alpha":{"type":"boolean","default":false},"trim_transparent":{"type":"boolean","default":false},"canvas":{"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2},"offsets":{"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2},"output":string}),vec!["project","image","slot","layer","output"],false);
    add("sprite_import_sheet","Import explicit sprite-sheet cells into a NEW revision. Each mapping identifies slot, layer, column and row; cells must match target canvases unless fit=true. Optional canvas=[w,h] and offsets=[x,y] can create missing body frames; replace_layers explicitly clears old shadow/overlay surfaces.",json!({"project":string,"image":string,"cell_width":integer,"cell_height":integer,"fit":{"type":"boolean"},"binary_alpha":{"type":"boolean"},"trim_transparent":{"type":"boolean","default":false},"canvas":{"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2},"offsets":{"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2},"replace_layers":{"type":"boolean"},"mapping":{"type":"array","minItems":1,"maxItems":4096,"items":{"type":"object","properties":{"slot":integer,"layer":{"enum":["image","shadow","overlay"]},"column":integer,"row":integer},"required":["slot","layer","column","row"],"additionalProperties":false}},"output":string}),vec!["project","image","cell_width","cell_height","mapping","output"],false);
    for name in ["castle_gate_preview", "castle_gate_place"] {
        add(name, "Preview or create/relocate a genuine castle gate. Full closed footprint must be walkable, in bounds and nonoverlapping. Placement writes a private database revision; preview is read-only.", json!({"placement":{"type":"object","properties":{"gate":integer,"castle":integer,"monster":integer,"gate_image":{"type":"integer","minimum":540,"maximum":543},"x":integer,"y":integer,"repair_cost":integer},"required":["x","y","repair_cost"],"additionalProperties":false},"database":string,"output":string}),vec!["placement"],name=="castle_gate_preview");
    }
    add("sprite_build_batch", "Validate and build 1–32 sprite projects atomically into a new pack set with a shared project-set.json manifest. No content database publication.",json!({"projects":{"type":"array","items":string,"minItems":1,"maxItems":32},"output":string}),vec!["projects","output"],false);
    add("sprite_tint", "Tint all frames of one image/overlay layer consistently into a new revision. RGB multipliers0–255 preserve alpha, anchors and other layers;255 is neutral.", json!({"project":string,"layer":{"enum":["image","overlay"]},"rgb":{"type":"array","items":{"type":"integer","minimum":0,"maximum":255},"minItems":3,"maxItems":3},"output":string}),vec!["project","layer","rgb","output"],false);
    add("sprite_paint", "Draw opaque or erase polygons on selected sprite frames in a new revision. Points use original image pixel coordinates; padding=[left,top,right,bottom] expands the canvas and preserves the ground anchor. under=true fills only transparent pixels, useful for silhouettes behind source art.",json!({"project":string,"output":string,"padding":{"type":"array","items":{"type":"integer","minimum":0,"maximum":64},"minItems":4,"maxItems":4},"polygons":{"type":"array","minItems":1,"maxItems":4096,"items":{"type":"object","properties":{"slot":integer,"points":{"type":"array","minItems":3,"maxItems":64,"items":{"type":"array","items":{"type":"integer","minimum":-32768,"maximum":32767},"minItems":2,"maxItems":2}},"color":{"type":"array","items":{"type":"integer","minimum":0,"maximum":255},"minItems":4,"maxItems":4},"under":{"type":"boolean"}},"required":["slot","points","color"],"additionalProperties":false}}}),vec!["project","output","polygons"],false);
    add("sprite_paint_motion", "Paint a garment polygon across one animation direction from ordered first/last keyframes. Intermediate frames interpolate points and RGB; padding expands the canvas. Creates a private project revision.",json!({"project":string,"output":string,"padding":{"type":"array","items":{"type":"integer","minimum":0,"maximum":64},"minItems":4,"maxItems":4},"slots":{"type":"array","minItems":2,"maxItems":4096,"items":integer},"keyframes":{"type":"array","minItems":2,"maxItems":4096,"items":{"type":"object","properties":{"slot":integer,"points":{"type":"array","minItems":3,"maxItems":64,"items":{"type":"array","items":integer,"minItems":2,"maxItems":2}},"color":{"type":"array","items":integer,"minItems":4,"maxItems":4},"under":{"type":"boolean"}},"required":["slot","points","color"],"additionalProperties":false}}}),vec!["project","output","slots","keyframes"],false);
    add("sprite_auto_motion", "Design cloth sway and recoil for one action/direction from a single polygon pose. The top edge stays anchored; 1–12 strength controls lower-vertex motion. Creates a private sprite revision.",json!({"project":string,"output":string,"padding":{"type":"array","items":{"type":"integer","minimum":0,"maximum":64},"minItems":4,"maxItems":4},"slots":{"type":"array","minItems":2,"maxItems":4096,"items":integer},"seed":{"type":"object","properties":{"slot":integer,"points":{"type":"array","minItems":3,"maxItems":64,"items":{"type":"array","items":integer,"minItems":2,"maxItems":2}},"color":{"type":"array","items":integer,"minItems":4,"maxItems":4},"under":{"type":"boolean"}},"required":["slot","points","color"],"additionalProperties":false},"strength":{"type":"integer","minimum":1,"maximum":12}}),vec!["project","output","slots","seed","strength"],false);
    add("wearable_publish", "Publish all required gender/class body projects as one new armour Shape in a private asset view. Every required family must match the source armour.", json!({"projects":{"type":"array","items":string,"minItems":1,"maxItems":4},"source_id":integer,"name":string,"output":string,"database":string,"assets":string}), vec!["projects","source_id","name","output"], false);
    add("equipment_publish", "Publish required equipped weapon, helmet or shield art families as one new unused Shape and private asset/database revision. Export source projects with sprite_export_project first.", json!({"projects":{"type":"array","items":string,"minItems":1,"maxItems":4},"source_id":integer,"name":string,"output":string,"database":string,"assets":string}), vec!["projects","source_id","name","output"], false);
    add("sprite_publish","Build authored sprite into a private Client asset view and new database record with an unused appearance. Validates saved source identity; leaves all existing appearances unchanged.",json!({"project":string,"kind":{"enum":["item","monster"]},"source_id":integer,"name":string,"output":string,"database":string,"assets":string}),vec!["project","kind","source_id","name","output"],false);
    result
}

#[cfg(test)]
#[path = "mcp_authoring_tests.rs"]
mod tests;
