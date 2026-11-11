//! Explicit, cached searches across a small reusable piece catalogue.
use crate::{
    palace_kit, piece_assembly,
    piece_candidates::{self, Candidate},
    piece_catalogue, wall_kit,
    wall_piece::{Kind, Piece},
    wood_corners,
};
use eframe::egui;
use std::path::Path;

#[derive(Default)]
pub struct Browser {
    pieces: Vec<Piece>,
    path: String,
    results: Option<Vec<Candidate>>,
}

impl Browser {
    pub fn invalidate(&mut self) {
        self.results = None;
    }
    pub fn autoload(&mut self, assets: &Path) -> anyhow::Result<Option<Piece>> {
        if let Ok(path) = std::env::var("ZIRCON_PIECE_CATALOGUE") {
            self.pieces = piece_catalogue::load(Path::new(&path), assets)?;
            self.path = path;
        } else if let Ok(kit) = std::env::var("ZIRCON_BUILTIN_KIT") {
            self.pieces = match kit.as_str() {
                "wood-corner" => wall_kit::wood_corner(assets)?,
                "wood-south-corner" => wood_corners::south_corner(assets)?,
                "palace-doorway" => vec![palace_kit::doorway(assets)?],
                "wood-entry" => vec![crate::wood_entry::entry(assets)?],
                "wood-wall-junction" => vec![crate::wood_wall_entry::entry(assets)?],
                "wood-wall-bodies" => crate::wood_wall_bodies::bodies(assets)?,
                "wood-perimeter-corners" => crate::wood_perimeter_kit::corners(assets)?,
                "wood-concave-corners" => crate::wood_concave::corners(assets)?,
                _ => anyhow::bail!("Unknown built-in piece kit: {kit}"),
            };
        }
        self.invalidate();
        if std::env::var_os("ZIRCON_ASSEMBLE_KIT").is_some() {
            return piece_assembly::assemble(&self.pieces, assets).map(Some);
        }
        Ok(self.pieces.first().cloned())
    }
    pub fn search(&mut self, current: &Piece, assets: &Path) -> anyhow::Result<()> {
        self.invalidate();
        self.results = Some(piece_candidates::enumerate(current, &self.pieces, assets)?);
        Ok(())
    }
    fn add_current(&mut self, current: &Piece, name: &str, kind: Kind) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.pieces.len() < piece_candidates::MAX_SOURCES,
            "A kit holds at most 16 pieces. Remove an entry first."
        );
        // Validate the edited metadata and connections before changing the kit.
        current.serialized(name, kind)?;
        let mut saved = current.clone();
        saved.name = name.trim().into();
        saved.kind = kind;
        self.pieces.push(saved);
        self.invalidate();
        Ok(())
    }
    fn remove(&mut self, index: usize) {
        self.pieces.remove(index);
        self.invalidate();
    }
    fn move_entry(&mut self, index: usize, up: bool) {
        let target = if up {
            index.checked_sub(1)
        } else {
            index.checked_add(1)
        };
        if let Some(target) = target {
            if index < self.pieces.len() && target < self.pieces.len() {
                self.pieces.swap(index, target);
                self.invalidate();
            }
        }
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        current: Option<&Piece>,
        name: &str,
        kind: Kind,
        assets: &Path,
    ) -> (Option<Piece>, Option<String>) {
        let mut picked = None;
        let mut status = None;
        egui::CollapsingHeader::new("Piece kit browser").default_open(true).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (label, kit) in [("Load Wood north kit", 0), ("Load Wood south kit", 1), ("Load palace doorway", 2), ("Wood entry", 3), ("Wood wall bodies", 4), ("Wood perimeter corners", 5), ("Wood wall junction", 6), ("Wood concave corners", 7)] {
                    if ui.button(label).clicked() {
                        let result = match kit { 0 => wall_kit::wood_corner(assets), 1 => wood_corners::south_corner(assets), 2 => palace_kit::doorway(assets).map(|p| vec![p]), 3 => crate::wood_entry::entry(assets).map(|p|vec![p]), 4 => crate::wood_wall_bodies::bodies(assets), 5 => crate::wood_perimeter_kit::corners(assets), 6 => crate::wood_wall_entry::entry(assets).map(|p|vec![p]), _ => crate::wood_concave::corners(assets) };
                        match result {
                            Ok(pieces) => { self.pieces = pieces; self.invalidate(); status = Some("Kit loaded. Choose a starting piece.".into()); },
                            Err(e) => status = Some(e.to_string()),
                        }
                    }
                }
            });
            ui.label("Wood and palace use separate connection profiles. Review artwork seams in Test Walk.");
            egui::CollapsingHeader::new("Kit folder").show(ui, |ui| {
                ui.text_edit_singleline(&mut self.path);
                if ui.button("Load folder").clicked() {
                    match piece_catalogue::load(Path::new(&self.path), assets) {
                        Ok(pieces) => { self.pieces = pieces; self.invalidate(); },
                        Err(e) => status = Some(e.to_string()),
                    }
                }
                if ui.add_enabled(!self.pieces.is_empty(), egui::Button::new("Save kit to new folder")).clicked() {
                    status = Some(match piece_catalogue::export(&self.pieces, Path::new(&self.path)) {
                        Ok(()) => "Kit saved; files reference your installed artwork.".into(), Err(e) => e.to_string(),
                    });
                }
            });
            if ui.add_enabled(current.is_some() && self.pieces.len() < piece_candidates::MAX_SOURCES, egui::Button::new("Add current piece to kit")).clicked() {
                status = Some(match self.add_current(current.unwrap(), name, kind) {
                    Ok(()) => "Added a copy with the current name, type and connections. Save the kit to a new folder to keep it.".into(),
                    Err(e) => e.to_string(),
                });
            }
            ui.label("The first piece starts assembly. Move entries to change the kit order.");
            ui.label("Assemble uses every entry once and requires one clear next join.");
            if ui.add_enabled(!self.pieces.is_empty(), egui::Button::new("Assemble whole kit")).clicked() {
                match piece_assembly::assemble(&self.pieces, assets) {
                    Ok(piece) => picked = Some(piece),
                    Err(error) => status = Some(format!("Assembly refused: {error:#}")),
                }
            }

            let mut remove = None;
            let mut moved = None;
            egui::ScrollArea::vertical().id_salt("kit_entries").max_height(180.0).show(ui, |ui| {
                for (i, p) in self.pieces.iter().enumerate() {
                    ui.push_id(i, |ui| { ui.horizontal_wrapped(|ui| {
                        ui.label(format!("{} ({} × {})", p.name, p.size().0, p.size().1));
                        if ui.small_button("Use piece").clicked() { picked = Some(p.clone()); }
                        if ui.add_enabled(i > 0, egui::Button::new("Up").small()).on_hover_text("Move up").clicked() { moved = Some((i, true)); }
                        if ui.add_enabled(i + 1 < self.pieces.len(), egui::Button::new("Down").small()).on_hover_text("Move down").clicked() { moved = Some((i, false)); }
                        if ui.small_button("Remove").clicked() { remove = Some(i); }
                    }); });
                }
            });
            if let Some(index) = remove { self.remove(index); }
            else if let Some((index, up)) = moved { self.move_entry(index, up); }
            if ui.add_enabled(current.is_some() && !self.pieces.is_empty(), egui::Button::new("Find compatible joins")).clicked() {
                if let Err(e) = self.search(current.unwrap(), assets) { status = Some(e.to_string()); }
            }
            if let Some(results) = &self.results {
                ui.label(format!("{} checked joins", results.len()));
                for c in results {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("{} · ports {} / {} · {} × {} · {} openings", self.pieces[c.source_index].name, c.current_port+1, c.candidate_port+1, c.size.0, c.size.1, c.remaining_ports));
                        if ui.small_button("Build").clicked() {
                            if let Some(current) = current {
                                match current.join(&self.pieces[c.source_index], c.current_port, c.candidate_port, assets) {
                                    Ok(p) => picked = Some(p), Err(e) => status = Some(format!("Join refused: {e}")),
                                }
                            }
                        }
                    });
                }
            }
        });
        if picked.is_some() {
            self.invalidate();
            status = Some("Piece ready. Place it on the map, then review in Test Walk.".into());
        }
        (picked, status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        piece_ports::{Port, Side},
        tile_doc::Stamp,
        wall_piece::Kind,
    };
    use mir_formats::map::MapFile;

    fn piece(side: Side) -> Piece {
        let mut map = MapFile::blank(4, 4);
        for x in 0..4 {
            for y in 0..4 {
                let cell = map.cell_mut(x, y).unwrap();
                cell.flag = 3;
                cell.back_file = 255;
            }
        }
        let mut piece = Piece::capture(
            &Stamp::capture(&map, (0, 0), (3, 3)),
            "Browser fixture".into(),
            Kind::Wall,
            Path::new("unused-assets"),
        )
        .unwrap();
        piece.ports.push(Port {
            side,
            ..Port::default()
        });
        piece
    }

    #[test]
    fn moving_entries_changes_the_start_and_invalidates_cached_source_indices() {
        let mut browser = Browser::default();
        for name in ["First", "Second", "Third"] {
            browser
                .add_current(&piece(Side::West), name, Kind::Wall)
                .unwrap();
        }
        browser
            .search(&piece(Side::East), Path::new("unused-assets"))
            .unwrap();
        assert_eq!(browser.results.as_ref().unwrap().len(), 3);
        browser.move_entry(1, true);
        assert!(browser.results.is_none());
        assert_eq!(
            browser
                .pieces
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["Second", "First", "Third"]
        );
        browser
            .search(&piece(Side::East), Path::new("unused-assets"))
            .unwrap();
        browser.move_entry(0, false);
        assert!(browser.results.is_none());
        assert_eq!(
            browser
                .pieces
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second", "Third"]
        );
    }

    #[test]
    fn boundary_moves_leave_order_and_valid_cached_results_unchanged() {
        let mut browser = Browser::default();
        browser.move_entry(0, true);
        browser.move_entry(0, false);
        browser.move_entry(usize::MAX, false);
        browser
            .add_current(&piece(Side::West), "Only", Kind::Wall)
            .unwrap();
        browser
            .search(&piece(Side::East), Path::new("unused-assets"))
            .unwrap();
        let results = browser.results.clone();
        for (index, up) in [(0, true), (0, false), (1, true), (usize::MAX, false)] {
            browser.move_entry(index, up);
            assert_eq!(browser.results, results);
            assert_eq!(browser.pieces.len(), 1);
            assert_eq!(browser.pieces[0].name, "Only");
        }
    }

    #[test]
    fn adding_current_keeps_an_independent_copy_of_edited_metadata() {
        let mut current = piece(Side::West);
        current.ports[0].offset = 2;
        current.ports[0].width = 2;
        current.ports[0].profile = "edited wood".into();
        let edited_ports = current.ports.clone();
        let mut browser = Browser {
            results: Some(vec![]),
            ..Browser::default()
        };
        browser
            .add_current(&current, "  Edited corner  ", Kind::Corner)
            .unwrap();
        assert!(browser.results.is_none());
        assert_eq!(current.name, "Browser fixture");
        assert_eq!(current.kind, Kind::Wall);
        current.ports[0].profile = "changed afterward".into();
        current.ports.clear();
        let saved = &browser.pieces[0];
        assert_eq!(saved.name, "Edited corner");
        assert_eq!(saved.kind, Kind::Corner);
        assert_eq!(saved.ports, edited_ports);
        assert_eq!(saved.size(), current.size());
    }

    #[test]
    fn refused_additions_preserve_the_catalogue_and_cached_joins() {
        let current = piece(Side::East);
        let mut browser = Browser {
            pieces: vec![piece(Side::West)],
            ..Browser::default()
        };
        browser
            .search(&current, Path::new("unused-assets"))
            .unwrap();
        let results = browser.results.clone();
        let before = browser.pieces[0]
            .serialized(&browser.pieces[0].name, browser.pieces[0].kind)
            .unwrap();
        assert!(browser.add_current(&current, "  ", Kind::Wall).is_err());
        let mut invalid = current.clone();
        invalid.ports[0].offset = 4;
        assert!(browser
            .add_current(&invalid, "Invalid opening", Kind::Doorway)
            .is_err());
        assert_eq!(browser.pieces.len(), 1);
        assert_eq!(browser.results, results);
        assert_eq!(
            browser.pieces[0]
                .serialized(&browser.pieces[0].name, browser.pieces[0].kind)
                .unwrap(),
            before
        );
    }

    #[test]
    fn full_catalogue_refuses_additions_until_removal_and_clears_stale_indices() {
        let current = piece(Side::East);
        let mut browser = Browser::default();
        for i in 0..piece_candidates::MAX_SOURCES {
            browser
                .add_current(&piece(Side::West), &format!("Entry {i}"), Kind::Wall)
                .unwrap();
        }
        browser
            .search(&current, Path::new("unused-assets"))
            .unwrap();
        let results = browser.results.clone();
        assert_eq!(
            results.as_ref().unwrap().len(),
            piece_candidates::MAX_SOURCES
        );
        assert!(browser
            .add_current(&current, "Overflow", Kind::Wall)
            .is_err());
        assert_eq!(browser.pieces.len(), piece_candidates::MAX_SOURCES);
        assert_eq!(browser.results, results);
        browser.remove(0);
        assert!(browser.results.is_none());
        assert_eq!(browser.pieces[0].name, "Entry 1");
        browser
            .add_current(&current, "Replacement", Kind::Wall)
            .unwrap();
        assert_eq!(browser.pieces.len(), piece_candidates::MAX_SOURCES);
        assert_eq!(browser.pieces.last().unwrap().name, "Replacement");
    }

    #[test]
    fn changed_connections_and_failed_searches_do_not_keep_old_suggestions() {
        let assets = Path::new("unused-assets");
        let mut current = piece(Side::East);
        let mut browser = Browser {
            pieces: vec![piece(Side::West)],
            ..Browser::default()
        };
        browser.search(&current, assets).unwrap();
        assert_eq!(browser.results.as_ref().unwrap().len(), 1);

        current.ports[0].profile = "different wall family".into();
        browser.invalidate();
        assert!(browser.results.is_none());
        browser.search(&current, assets).unwrap();
        assert!(browser.results.as_ref().unwrap().is_empty());

        current.ports[0].profile = "default".into();
        browser.search(&current, assets).unwrap();
        assert_eq!(browser.results.as_ref().unwrap().len(), 1);
        browser.pieces = (0..=piece_candidates::MAX_SOURCES)
            .map(|_| piece(Side::West))
            .collect();
        assert!(browser.search(&current, assets).is_err());
        assert!(browser.results.is_none());
        assert_eq!(current.ports[0].side, Side::East);
        assert_eq!(current.size(), (4, 4));
    }
}
