//! The ranking board (Zircon `RankingDialog`, served by `SEnvir.GetRanks`).
//!
//! Zircon keeps every character in one linked list ordered by level then
//! experience, both descending, and re-sorts a character whenever it gains
//! either. We hold the same order in a vector and rebuild it per request:
//! the board is read far less often than experience changes, so sorting on
//! read is both simpler and always current.
//!
//! Two rules from `GetRanks` that are easy to get wrong:
//!
//! * The rank number counts every character the **class** filter admits,
//!   whether or not it is online. Ticking "online only" hides rows but does
//!   not renumber the ones that remain.
//! * `total` counts the rows left **after** the online filter, because it
//!   sizes the client's scrollbar.

use std::collections::{HashMap, HashSet};

use mir_proto::{Class, Grid, RankEntry, RankInspection, EQUIPMENT_SIZE};

use crate::accounts::CharacterRecord;

/// Rows the client asks for at a time. Zircon sends 21 for 11 visible
/// lines, so a small scroll needs no round trip.
pub const PAGE: usize = 21;

/// How long a climb or fall stays on the board before the baseline moves
/// up to the present (Zircon `Config.RankChangeResetDelay`).
pub const CHANGE_RESET_MS: u64 = 24 * 60 * 60 * 1000;

/// What the index needs to know about a character. Built from the account
/// store, which holds offline characters too -- Zircon ranks those as well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankRow {
    pub character: u32,
    pub name: String,
    pub class: Class,
    pub level: i32,
    pub experience: u64,
    pub max_experience: u64,
    pub rebirth: i32,
}

/// Zircon's order: higher level first, then higher experience. Ties fall
/// back to the character id so the board never reshuffles between requests.
fn by_rank(a: &RankRow, b: &RankRow) -> std::cmp::Ordering {
    b.level
        .cmp(&a.level)
        .then(b.experience.cmp(&a.experience))
        .then(a.character.cmp(&b.character))
}

/// Sort rows into ranking order.
pub fn sort_rows(rows: &mut [RankRow]) {
    rows.sort_by(by_rank);
}

/// The baseline each character's rank is measured against, per class
/// filter, plus when that baseline is next replaced by the present.
#[derive(Debug, Default)]
pub struct Ranking {
    /// (class filter, character) -> rank when the baseline was taken.
    baseline: HashMap<(Option<Class>, u32), u32>,
    next_reset: u64,
}

/// One page of the board and the total number of rows behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankPage {
    pub total: u32,
    pub entries: Vec<RankEntry>,
}

impl Ranking {
    /// Build a page. `rows` need not be sorted. `online` holds the character
    /// ids currently in the world.
    pub fn page(
        &mut self,
        mut rows: Vec<RankRow>,
        filter: Option<Class>,
        online_only: bool,
        start: u32,
        online: &HashSet<u32>,
        now: u64,
    ) -> RankPage {
        sort_rows(&mut rows);

        // Past the reset the board forgets old movement: every rank becomes
        // its own baseline, so the whole column reads as a hold again.
        if now >= self.next_reset {
            self.baseline.clear();
            self.next_reset = now + CHANGE_RESET_MS;
        }

        let mut entries = Vec::new();
        let mut rank = 0u32;
        let mut total = 0u32;
        for row in rows {
            if filter.is_some_and(|c| c != row.class) {
                continue;
            }
            // Counted before the online filter: Zircon's rank numbers are
            // the same list whether or not offline players are shown.
            rank += 1;
            let was = *self.baseline.entry((filter, row.character)).or_insert(rank);
            let is_online = online.contains(&row.character);
            if online_only && !is_online {
                continue;
            }
            let index = total;
            total += 1;
            if index < start || entries.len() >= PAGE {
                continue;
            }
            entries.push(RankEntry {
                rank,
                character: row.character,
                name: row.name,
                class: row.class,
                level: row.level,
                experience: row.experience,
                max_experience: row.max_experience,
                online: is_online,
                rebirth: row.rebirth,
                // A smaller number is a better rank, so a fall in the
                // number is a climb on the board.
                change: was as i32 - rank as i32,
            });
        }
        RankPage { total, entries }
    }

    /// Zircon's name search ignores the board's current class/online filters.
    pub fn search(
        &mut self,
        mut rows: Vec<RankRow>,
        name: &str,
        online: &HashSet<u32>,
        now: u64,
    ) -> (Option<RankEntry>, u32) {
        sort_rows(&mut rows);
        let Some(start) = rows
            .iter()
            .position(|row| row.name.eq_ignore_ascii_case(name.trim()))
        else {
            return (None, 0);
        };
        let entry = self
            .page(rows, None, false, start as u32, online, now)
            .entries
            .into_iter()
            .next();
        (entry, start as u32)
    }
}

impl crate::world::World {
    /// Answer a `RankRequest`. `rows` comes from the account store, which
    /// is where offline characters live; the world only knows who is on.
    pub fn rank_request(
        &mut self,
        id: mir_proto::ObjectId,
        rows: Vec<RankRow>,
        filter: Option<Class>,
        online_only: bool,
        start: u32,
    ) {
        let online: HashSet<u32> = self
            .players()
            .filter_map(|o| o.player())
            .map(|p| p.character)
            .collect();
        let now = self.now;
        let page = self
            .ranking
            .page(rows, filter, online_only, start, &online, now);
        self.send_to(
            id,
            mir_proto::ServerMessage::Rankings {
                class: filter,
                online_only,
                start,
                total: page.total,
                entries: page.entries,
            },
        );
    }

    pub fn rank_search(&mut self, id: mir_proto::ObjectId, rows: Vec<RankRow>, name: &str) {
        let online: HashSet<u32> = self
            .players()
            .filter_map(|o| o.player())
            .map(|p| p.character)
            .collect();
        let (entry, start) = self.ranking.search(rows, name, &online, self.now);
        self.send_to(
            id,
            mir_proto::ServerMessage::RankSearchResult { entry, start },
        );
    }

    pub fn rank_inspect(&mut self, id: mir_proto::ObjectId, record: Option<CharacterRecord>) {
        let details = record.map(|record| {
            let online = self
                .players()
                .filter_map(|o| o.player())
                .find(|p| p.character == record.id);
            let (name, class, gender, level, hair, hair_colour, equipment) = if let Some(p) = online
            {
                (
                    p.name.clone(),
                    p.class,
                    p.gender,
                    p.level,
                    p.hair,
                    p.hair_colour,
                    p.bag
                        .equipment
                        .iter()
                        .enumerate()
                        .filter_map(|(slot, item)| {
                            item.as_ref().map(|item| (slot as u8, item.instance()))
                        })
                        .collect(),
                )
            } else {
                (
                    record.name.clone(),
                    record.class,
                    record.gender,
                    record.level,
                    record.hair,
                    record.hair_colour,
                    inspected_equipment(&record),
                )
            };
            let guild = self.guild_store.guilds.iter().find_map(|guild| {
                guild
                    .members
                    .iter()
                    .find(|member| member.character == record.id)
                    .map(|member| (guild.name.clone(), member.rank.clone()))
            });
            RankInspection {
                character: record.id,
                name,
                class,
                gender,
                level,
                hair,
                hair_colour,
                partner: record.partner_name,
                fame: record.fame,
                guild_name: guild
                    .as_ref()
                    .map_or_else(String::new, |(name, _)| name.clone()),
                guild_rank: guild.map_or_else(String::new, |(_, rank)| rank),
                equipment,
            }
        });
        self.send_to(id, mir_proto::ServerMessage::RankInspection(details));
    }
}

fn inspected_equipment(record: &CharacterRecord) -> Vec<(u8, mir_proto::ItemInstance)> {
    let mut equipment: Vec<_> = record
        .items
        .iter()
        .filter(|item| item.grid == Grid::Equipment && (item.slot as usize) < EQUIPMENT_SIZE)
        .map(|item| (item.slot, item.item.instance()))
        .collect();
    equipment.sort_by_key(|(slot, _)| *slot);
    equipment
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(character: u32, class: Class, level: i32, experience: u64) -> RankRow {
        RankRow {
            character,
            name: format!("C{character}"),
            class,
            level,
            experience,
            max_experience: 1000,
            rebirth: 0,
        }
    }

    fn rows() -> Vec<RankRow> {
        vec![
            row(1, Class::Warrior, 10, 50),
            row(2, Class::Wizard, 12, 0),
            row(3, Class::Warrior, 10, 90),
            row(4, Class::Taoist, 3, 400),
        ]
    }

    #[test]
    fn orders_by_level_then_experience() {
        let mut r = rows();
        sort_rows(&mut r);
        let order: Vec<u32> = r.iter().map(|x| x.character).collect();
        // Level 12 first, then the two level 10s by experience, then level 3.
        assert_eq!(order, vec![2, 3, 1, 4]);
    }

    #[test]
    fn ties_break_on_character_id() {
        let mut r = vec![row(7, Class::Warrior, 5, 5), row(2, Class::Wizard, 5, 5)];
        sort_rows(&mut r);
        assert_eq!(r[0].character, 2);
    }

    #[test]
    fn class_filter_renumbers_ranks() {
        let mut rank = Ranking::default();
        let page = rank.page(rows(), Some(Class::Warrior), false, 0, &HashSet::new(), 0);
        assert_eq!(page.total, 2);
        let seen: Vec<(u32, u32)> = page.entries.iter().map(|e| (e.rank, e.character)).collect();
        // Only warriors are counted, so they take ranks 1 and 2.
        assert_eq!(seen, vec![(1, 3), (2, 1)]);
    }

    #[test]
    fn online_filter_hides_rows_but_keeps_rank_numbers() {
        let mut rank = Ranking::default();
        let online: HashSet<u32> = [1u32, 4].into_iter().collect();
        let page = rank.page(rows(), None, true, 0, &online, 0);
        assert_eq!(page.total, 2);
        let seen: Vec<(u32, u32)> = page.entries.iter().map(|e| (e.rank, e.character)).collect();
        // Character 1 keeps rank 3 and character 4 rank 4, even though the
        // two above them are hidden.
        assert_eq!(seen, vec![(3, 1), (4, 4)]);
        assert!(page.entries.iter().all(|e| e.online));
    }

    #[test]
    fn start_index_pages_without_changing_ranks() {
        let mut rank = Ranking::default();
        let page = rank.page(rows(), None, false, 2, &HashSet::new(), 0);
        // Total still counts everyone; the page begins at the third row.
        assert_eq!(page.total, 4);
        let seen: Vec<u32> = page.entries.iter().map(|e| e.rank).collect();
        assert_eq!(seen, vec![3, 4]);
    }

    #[test]
    fn change_is_zero_until_the_board_moves() {
        let mut rank = Ranking::default();
        let page = rank.page(rows(), None, false, 0, &HashSet::new(), 0);
        assert!(page.entries.iter().all(|e| e.change == 0));
    }

    #[test]
    fn climbing_reports_a_positive_change() {
        let mut rank = Ranking::default();
        rank.page(rows(), None, false, 0, &HashSet::new(), 0);
        // Character 4 levels past everyone and takes rank 1 from rank 4.
        let mut moved = rows();
        moved[3].level = 40;
        let page = rank.page(moved, None, false, 0, &HashSet::new(), 1);
        let four = page.entries.iter().find(|e| e.character == 4).unwrap();
        assert_eq!((four.rank, four.change), (1, 3));
        // Everyone it passed slipped one place.
        let two = page.entries.iter().find(|e| e.character == 2).unwrap();
        assert_eq!((two.rank, two.change), (2, -1));
    }

    #[test]
    fn the_reset_makes_every_row_a_hold_again() {
        let mut rank = Ranking::default();
        rank.page(rows(), None, false, 0, &HashSet::new(), 0);
        let mut moved = rows();
        moved[3].level = 40;
        let page = rank.page(
            moved.clone(),
            None,
            false,
            0,
            &HashSet::new(),
            CHANGE_RESET_MS,
        );
        assert!(page.entries.iter().all(|e| e.change == 0));
    }

    #[test]
    fn a_page_is_capped() {
        let mut rank = Ranking::default();
        let many: Vec<RankRow> = (1..=60)
            .map(|i| row(i, Class::Warrior, i as i32, 0))
            .collect();
        let page = rank.page(many, None, false, 0, &HashSet::new(), 0);
        assert_eq!(page.total, 60);
        assert_eq!(page.entries.len(), PAGE);
        assert_eq!(page.entries[0].rank, 1);
    }

    #[test]
    fn search_finds_exact_name_independent_of_current_board_filters() {
        let mut rank = Ranking::default();
        let (found, start) = rank.search(rows(), " c1 ", &HashSet::new(), 0);
        assert_eq!(start, 2);
        assert_eq!(
            found.map(|entry| (entry.rank, entry.character)),
            Some((3, 1))
        );
        assert_eq!(rank.search(rows(), "C", &HashSet::new(), 0), (None, 0));
    }

    #[test]
    fn inspection_never_includes_inventory_or_storage() {
        use crate::items::{StoredItem, UserItem};

        let item = UserItem {
            id: 1,
            info: 100,
            count: 1,
            durability: 1000,
            max_durability: 1000,
            added: vec![],
            level: 0,
            refinable: true,
            non_refinable: false,
            bound: false,
            worthless: false,
            locked: false,
            special_repair_until: 0,
            weapon_experience_tenths: 0,
            accessory_experience: 0,
            expires_at: None,
            colour: 0,
        };
        let mut record = crate::accounts::test_character("Inspector");
        record.items = [Grid::Inventory, Grid::Storage, Grid::Equipment]
            .into_iter()
            .enumerate()
            .map(|(index, grid)| StoredItem {
                grid,
                slot: index as u8,
                item: UserItem {
                    id: index as u32 + 1,
                    ..item.clone()
                },
            })
            .collect();
        let equipment = inspected_equipment(&record);
        assert_eq!(equipment.len(), 1);
        assert_eq!(equipment[0].0, 2);
        assert_eq!(equipment[0].1.id, 3);
    }
}
