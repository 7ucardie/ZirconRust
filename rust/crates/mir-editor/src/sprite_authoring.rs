//! Sprite project identity and shared game-contract validation.
use crate::{
    sprite_contract::{self, Contract},
    sprite_project,
};
use anyhow::{ensure, Context, Result};
use mir_formats::zl::{SurfaceKind, ZlLibrary};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub kind: String,
    pub image: u16,
}

pub fn contract(assets: &Path, identity: &Identity) -> Result<Contract> {
    match identity.kind.as_str() {
        "monster" => sprite_contract::monster(identity.image, assets),
        "item" => sprite_contract::item(u32::from(identity.image), false, assets),
        kind if sprite_contract::armour_family(kind).is_some() => {
            sprite_contract::armour(kind, identity.image, assets)
        }
        kind if sprite_contract::equipment_family(kind).is_some() => {
            sprite_contract::equipment(kind, identity.image, assets)
        }
        _ => anyhow::bail!("Unknown sprite kind"),
    }
}
pub fn identity(project: &Path) -> Result<Identity> {
    let root = project.canonicalize()?;
    let path = root.join("identity.json").canonicalize()?;
    ensure!(path.starts_with(&root), "Sprite identity escapes project");
    ensure!(
        std::fs::metadata(&path)?.len() <= 4096,
        "Sprite identity is too large"
    );
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
pub fn export(assets: &Path, identity: &Identity, output: &Path) -> Result<sprite_project::Report> {
    let contract = contract(assets, identity)?;
    let path = assets.join(&contract.library_path);
    let report =
        sprite_project::export_range(&path, contract.source_start, contract.source_slots, output)?;
    let result = (|| {
        std::fs::write(
            output.join("identity.json"),
            serde_json::to_vec_pretty(identity)?,
        )?;
        std::fs::write(
            output.join("contract.json"),
            serde_json::to_vec_pretty(&contract)?,
        )?;
        sprite_contract::export_reference(&contract, assets, &output.join("reference"))?;
        std::fs::copy(
            output.join("reference/contact-sheet.png"),
            output.join("reference.png"),
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(output);
        return Err(error);
    }
    Ok(report)
}

/// Copy a complete hand-drawn body pose contract into a new editable project.
/// The project keeps its source item identity; only the frame artwork changes.
pub fn pose_from_reference(
    assets: &Path,
    identity: &Identity,
    donor_image: u16,
    output: &Path,
) -> Result<sprite_project::Report> {
    ensure!(
        sprite_contract::armour_family(&identity.kind).is_some() && donor_image != identity.image,
        "Pose transfer needs a different armour appearance in the same body family"
    );
    let source = contract(assets, identity)?;
    let donor = contract(
        assets,
        &Identity {
            kind: identity.kind.clone(),
            image: donor_image,
        },
    )?;
    ensure!(
        source.library_path == donor.library_path
            && source.source_slots == donor.source_slots
            && source.frames.len() == donor.frames.len()
            && source.frames.iter().zip(&donor.frames).all(|(a, b)| {
                a.action == b.action
                    && a.direction == b.direction
                    && a.frame == b.frame
                    && a.index - source.source_start == b.index - donor.source_start
            }),
        "Pose donor does not match the source animation contract"
    );
    export(assets, identity, output)?;
    let result = (|| -> Result<sprite_project::Report> {
        let mut manifest = sprite_project::load_manifest(output)?;
        let library = ZlLibrary::open(assets.join(&donor.library_path))?;
        for (local, slot) in manifest.slots.iter_mut().enumerate() {
            let index = donor.source_start + local;
            let info = library.info(index);
            ensure!(
                slot.present == info.is_some(),
                "Pose donor differs in slot coverage at {local}"
            );
            let Some(info) = info else { continue };
            slot.offset_x = info.offset_x;
            slot.offset_y = info.offset_y;
            slot.shadow_offset_x = info.shadow_offset_x;
            slot.shadow_offset_y = info.shadow_offset_y;
            slot.shadow_type = info.shadow_type;
            for kind in [
                SurfaceKind::Image,
                SurfaceKind::Shadow,
                SurfaceKind::Overlay,
            ] {
                let surface = match kind {
                    SurfaceKind::Image => &mut slot.image,
                    SurfaceKind::Shadow => &mut slot.shadow,
                    SurfaceKind::Overlay => &mut slot.overlay,
                };
                let (width, height) = info.surface_dims(kind);
                ensure!(
                    surface.is_some() == (width > 0 && height > 0),
                    "Pose donor differs in image layers at {local}"
                );
                let Some(surface) = surface else { continue };
                let art = sprite_project::pixels(&library, index, kind)?
                    .context("Pose donor image is missing")?;
                let path = output.join(&surface.path);
                std::fs::remove_file(&path)?;
                sprite_project::write_png(&path, width, height, &art)?;
                surface.width = width;
                surface.height = height;
            }
        }
        std::fs::write(
            output.join("project.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        validate(assets, output)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(output);
    }
    result
}
pub fn validate(assets: &Path, project: &Path) -> Result<sprite_project::Report> {
    let report = sprite_project::validate(project)?;
    let identity = identity(project)?;
    let contract = contract(assets, &identity)?;
    let manifest = sprite_project::load_manifest(project)?;
    ensure!(
        manifest.source_start == contract.source_start
            && manifest.slots.len() == contract.source_slots
            && manifest.source_checksum == contract.source_checksum,
        "Sprite project does not match its source identity"
    );
    sprite_contract::validate_project(&contract, project).context("Game animation contract")?;
    Ok(report)
}

#[cfg(test)]
mod pose_tests {
    use super::*;

    #[test]
    fn hand_drawn_body_poses_transfer_without_changing_source_identity() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let source_identity = Identity {
            kind: "armour-male".into(),
            image: 4,
        };
        let library = assets.join(&contract(&assets, &source_identity).unwrap().library_path);
        let before = sprite_project::file_checksum(&library).unwrap();
        let output = std::env::temp_dir().join(format!(
            "zircon-pose-transfer-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let report = pose_from_reference(&assets, &source_identity, 5, &output).unwrap();
        assert!(report.changed_slots > 0);
        let manifest = sprite_project::load_manifest(&output).unwrap();
        let frames = contract(&assets, &source_identity).unwrap();
        let local = frames.frames[0].index - frames.source_start;
        let slot = &manifest.slots[local];
        let (_, _, actual) =
            sprite_project::read_png(&output.join(&slot.image.as_ref().unwrap().path)).unwrap();
        let donor = ZlLibrary::open(&library).unwrap();
        let donor_frames = contract(
            &assets,
            &Identity {
                kind: "armour-male".into(),
                image: 5,
            },
        )
        .unwrap();
        let expected = sprite_project::pixels(
            &donor,
            donor_frames.source_start + local,
            SurfaceKind::Image,
        )
        .unwrap()
        .unwrap();
        assert_eq!(actual.len(), expected.len());
        assert!(
            actual == expected,
            "pose pixel transfer differs at local slot {local}"
        );
        assert_eq!(identity(&output).unwrap().image, 4);
        assert_eq!(sprite_project::file_checksum(&library).unwrap(), before);
        std::fs::remove_dir_all(output).unwrap();
    }
}
