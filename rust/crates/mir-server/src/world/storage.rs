//! Account storage (Zircon `Storage`): `Grid::Storage`, reachable only in a
//! safe zone, capped by the account's storage size; no weight checks.

use super::*;
use crate::items::{StoredItem, STORAGE_SIZE};

impl World {
    /// Load the account's storage into the player's bag (on entry).
    pub fn set_storage(&mut self, id: ObjectId, items: &[StoredItem], size: u32) {
        let Some(p) = self.objects.get_mut(&id).and_then(|o| o.player_mut()) else {
            return;
        };
        let size = size.max(STORAGE_SIZE);
        p.storage_size = size;
        p.bag.storage = (0..size).map(|_| None).collect();
        for s in items {
            if s.grid != Grid::Storage {
                continue;
            }
            if let Some(cell) = p.bag.storage.get_mut(s.slot as usize) {
                if cell.is_none() {
                    *cell = Some(s.item.clone());
                }
            }
        }
        self.send_storage(id);
    }

    /// The account's storage as persisted.
    pub fn storage_of(&self, id: ObjectId) -> Option<(Vec<StoredItem>, u32)> {
        let p = self.objects.get(&id)?.player()?;
        let items = p
            .bag
            .storage
            .iter()
            .enumerate()
            .filter_map(|(i, it)| {
                it.as_ref().map(|item| StoredItem {
                    grid: Grid::Storage,
                    slot: i as u8,
                    item: item.clone(),
                })
            })
            .collect();
        Some((items, p.storage_size))
    }

    pub(super) fn send_storage(&mut self, id: ObjectId) {
        let Some(p) = self.objects.get(&id).and_then(|o| o.player()) else {
            return;
        };
        let items = p
            .bag
            .storage
            .iter()
            .enumerate()
            .filter_map(|(i, it)| it.as_ref().map(|it| (i as u8, it.instance())))
            .collect();
        let size = p.storage_size;
        self.send_to(id, ServerMessage::Storage { size, items });
    }

    /// Zircon `InSafeZone`: inside any `SafeZoneInfo` region of the map.
    pub(super) fn in_safe_zone(&self, map: i32, loc: Point) -> bool {
        let Some(width) = self.maps.get(&map).map(|m| m.file.width as i32) else {
            return false;
        };
        self.data.safe_zones.iter().any(|sz| {
            self.data
                .regions
                .get(&sz.region)
                .filter(|r| r.map == map)
                .is_some_and(|r| r.points(width).contains(&(loc.x, loc.y)))
        })
    }

    /// Send only the current map's safe cells when a player enters that map.
    pub(super) fn send_safe_zones(&mut self, id: ObjectId, map: i32) {
        if self.objects.get(&id).and_then(|o| o.player()).is_none() {
            return;
        }
        let Some(width) = self.maps.get(&map).map(|m| m.file.width as i32) else {
            return;
        };
        let mut cells: Vec<Point> = self
            .data
            .safe_zones
            .iter()
            .filter_map(|safe| self.data.regions.get(&safe.region))
            .filter(|region| region.map == map)
            .flat_map(|region| region.points(width))
            .map(|(x, y)| Point::new(x, y))
            .collect();
        cells.sort_unstable_by_key(|point| (point.y, point.x));
        cells.dedup();
        self.send_to(id, ServerMessage::SafeZones { cells });
    }

    /// Tell the player where storage becomes usable on this map.
    pub(super) fn storage_safe_zone_error(&self, map: i32, loc: Point) -> String {
        let reason = "You cannot access storage outside of a safe zone.";
        let Some(loaded) = self.maps.get(&map) else {
            return reason.into();
        };
        let width = loaded.file.width as i32;
        let nearest = self
            .data
            .safe_zones
            .iter()
            .filter_map(|safe| self.data.regions.get(&safe.region))
            .filter(|region| region.map == map)
            .flat_map(|region| region.points(width))
            .filter(|(x, y)| loaded.file.is_walkable(*x, *y))
            .min_by_key(|(x, y)| (x - loc.x).abs() + (y - loc.y).abs());
        let Some((x, y)) = nearest else {
            return format!("{reason} No safe zone on this map.");
        };
        let name = self
            .data
            .maps
            .get(&map)
            .map(|map| map.description.as_str())
            .unwrap_or("this map");
        format!("{reason} Nearest safe zone: {name} ({x}, {y}).")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_denial_points_to_safe_zone_near_taoist_mentor() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
            return;
        };
        let assets = std::path::PathBuf::from(assets);
        let data = crate::data::GameData::load(assets.join("../Database/System.db")).unwrap();
        let mut world = World::new(data, assets.join("Map"), None);
        let player = world
            .add_player(1, 1, &crate::accounts::test_character("StorageGuide"))
            .unwrap();
        assert!(world.outgoing.iter().any(|message| matches!(
            message,
            Outgoing::To(_, ServerMessage::SafeZones { cells }) if !cells.is_empty()
        )));
        world.outgoing.clear();
        world.test_change_map(player, 5, Point::new(164, 214));
        assert!(!world.test_in_safe_zone(player));
        assert!(world.outgoing.iter().any(|message| matches!(
            message,
            Outgoing::To(_, ServerMessage::SafeZones { cells })
                if cells.len() == 165
                    && cells.contains(&Point::new(172, 188))
                    && !cells.contains(&Point::new(164, 214))
        )));
        let potion = world
            .data
            .items
            .values()
            .find(|item| item.name == "Healing Potion")
            .unwrap()
            .index;
        world.test_give_item(player, potion, 1);
        let slot = world.test_slot_of(player, potion).unwrap();
        world.outgoing.clear();
        world.item_move(player, Grid::Inventory, slot, Grid::Storage, 0);
        assert!(world.outgoing.iter().any(|message| matches!(
            message,
            Outgoing::To(_, ServerMessage::Chat { text })
                if text.contains("Lost Paradise (172, 188)")
        )));
        assert!(world.test_storage(player).is_empty());
        world.outgoing.clear();
        world.test_change_map(player, 141, Point::new(134, 13));
        assert!(world.outgoing.iter().any(|message| matches!(
            message,
            Outgoing::To(_, ServerMessage::SafeZones { cells }) if cells.is_empty()
        )));
        world.outgoing.clear();
        world.item_move(player, Grid::Inventory, slot, Grid::Storage, 0);
        assert!(world.outgoing.iter().any(|message| matches!(
            message,
            Outgoing::To(_, ServerMessage::Chat { text })
                if text.contains("No safe zone on this map")
        )));
    }
}
