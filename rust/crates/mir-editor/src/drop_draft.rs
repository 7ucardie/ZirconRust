//! Reviewable copies of authored monster drop tables; probabilities are never scaled.
use anyhow::{ensure, Context, Result};
use mir_formats::mirdb::{Collection, MirDb, Record, Value};

#[derive(Debug)]
pub struct Template {
    pub monster_id: i32,
    pub name: String,
    pub level: i32,
    pub row_count: usize,
    pub material_count: usize,
}

#[derive(Debug, Clone)]
pub struct DropRow {
    pub item_id: i32,
    pub item_name: String,
    pub category: String,
    /// Base one-in chance; zero means disabled.
    pub chance: i32,
    /// Authored amount, before the runtime's random quantity roll.
    pub amount: i32,
    pub restrictions: String,
    pub drop_set: i32,
    pub easter_event: bool,
    pub part_only: bool,
}

/// Base independent-roll expectation, before account progress or group rules.
#[derive(Default)]
pub struct YieldSummary {
    pub items: std::collections::BTreeMap<i32, ItemYield>,
    pub disabled: usize,
    pub part_only: usize,
}
#[derive(Default)]
pub struct ItemYield {
    pub name: String,
    pub ordinary: f64,
    pub conditional: f64,
}

pub fn yield_summary<'a>(rows: impl IntoIterator<Item = &'a DropRow>) -> YieldSummary {
    let mut summary = YieldSummary::default();
    for row in rows {
        if row.chance <= 0 {
            summary.disabled += 1;
        }
        if row.part_only {
            summary.part_only += 1;
        }
        if row.chance <= 0 || row.part_only {
            continue;
        }
        // Runtime: max(1, floor(amount / 2) + uniform integer [0, amount)).
        // Amount 1 is the only positive parameter affected by the clamp.
        let mean = if row.amount <= 1 {
            1.0
        } else {
            f64::from(row.amount / 2) + (f64::from(row.amount) - 1.0) / 2.0
        };
        let units = 100.0 * mean / f64::from(row.chance);
        let item = summary.items.entry(row.item_id).or_default();
        item.name.clone_from(&row.item_name);
        if row.drop_set != 0 || row.easter_event {
            item.conditional += units;
        } else {
            item.ordinary += units;
        }
    }
    summary
}

#[derive(Debug)]
pub struct Draft {
    pub target_id: i32,
    pub donor_id: i32,
    pub donor_name: String,
    pub rows: Vec<DropRow>,
    pub notes: String,
    snapshot: Vec<u8>,
}

/// Prefer matching boss status, then nearest level and matching AI. All donors
/// remain available so the ranking does not silently remove authored choices.
pub fn candidates(db: &MirDb, target_id: i32) -> Vec<Template> {
    let (Some(monsters), Some(drops)) = (db.collection("MonsterInfo"), db.collection("DropInfo"))
    else {
        return Vec::new();
    };
    let Some(target) = monsters
        .records
        .iter()
        .find(|r| monsters.index(r) == target_id)
    else {
        return Vec::new();
    };
    let material_items: std::collections::BTreeSet<_> = db
        .collection("ItemInfo")
        .into_iter()
        .flat_map(|c| {
            c.records
                .iter()
                .filter(|r| category(c.int_or(r, "ItemType", 0)) == "Materials")
                .map(|r| i64::from(c.index(r)))
        })
        .collect();
    let mut result: Vec<_> = monsters
        .records
        .iter()
        .filter_map(|r| {
            let monster_id = monsters.index(r);
            let rows: Vec<_> = drops
                .records
                .iter()
                .filter(|d| drops.int(d, "Monster") == Some(i64::from(monster_id)))
                .collect();
            let row_count = rows.len();
            let material_count = rows
                .iter()
                .filter(|d| {
                    drops
                        .int(d, "Item")
                        .is_some_and(|item| material_items.contains(&item))
                })
                .count();
            (monster_id != target_id && row_count > 0).then(|| {
                (
                    Template {
                        monster_id,
                        name: monsters.str_or(r, "MonsterName", "Unnamed").into(),
                        level: monsters.int_or(r, "Level", 0) as i32,
                        row_count,
                        material_count,
                    },
                    monsters.bool_or(r, "IsBoss", false)
                        != monsters.bool_or(target, "IsBoss", false),
                    monsters.int(r, "AI") != monsters.int(target, "AI"),
                )
            })
        })
        .collect();
    let level = monsters.int_or(target, "Level", 0);
    result.sort_by_key(|(t, boss, ai)| {
        (*boss, (i64::from(t.level) - level).abs(), *ai, t.monster_id)
    });
    result.into_iter().map(|(t, _, _)| t).collect()
}

fn number(c: &Collection, row: &Record, field: &str) -> Result<i32> {
    let Some(Value::Int(n)) = c.get(row, field) else {
        anyhow::bail!("Invalid {}.{field}", c.short_name());
    };
    Ok(i32::try_from(*n)?)
}

fn schema(db: &MirDb) -> Result<&Collection> {
    let c = db
        .collection("DropInfo")
        .context("Missing DropInfo collection")?;
    for (field, kind) in [
        ("Monster", "System.Int32"),
        ("Item", "System.Int32"),
        ("Chance", "System.Int32"),
        ("Amount", "System.Int32"),
        ("DropSet", "System.Int32"),
        ("PartOnly", "System.Boolean"),
        ("EasterEvent", "System.Boolean"),
    ] {
        let index = c
            .property_index(field)
            .with_context(|| format!("Missing DropInfo.{field}"))?;
        ensure!(
            c.mapping.properties[index].type_name == kind,
            "Unsupported DropInfo.{field} type"
        );
    }
    if let Some(index) = c.property_index("Index") {
        ensure!(
            c.mapping.properties[index].type_name == "System.Int32",
            "Unsupported DropInfo.Index type"
        );
    }
    Ok(c)
}

// ItemType values follow LibraryCore/Enum.cs; these groups are editor filters.
fn category(item_type: i64) -> &'static str {
    match item_type {
        1 | 14 | 15 | 19 | 20 | 31 | 35 | 36 => "Consumables",
        2..=11 | 16 | 18 | 21..=23 | 26..=30 | 32 | 33 => "Equipment",
        12 | 13 | 17 | 25 => "Materials",
        _ => "Other",
    }
}

pub fn propose(db: &MirDb, target_id: i32, donor_id: i32) -> Result<Draft> {
    ensure!(
        target_id > 0 && donor_id > 0 && target_id != donor_id,
        "Choose different existing target and donor monsters"
    );
    let monsters = db
        .collection("MonsterInfo")
        .context("Missing MonsterInfo collection")?;
    ensure!(
        monsters
            .records
            .iter()
            .any(|r| monsters.index(r) == target_id),
        "Target monster no longer exists"
    );
    let donor = monsters
        .records
        .iter()
        .find(|r| monsters.index(r) == donor_id)
        .context("Donor monster no longer exists")?;
    let c = schema(db)?;
    ensure!(
        !c.records
            .iter()
            .any(|r| c.int(r, "Monster") == Some(i64::from(target_id))),
        "Target already has drops; templates only fill empty drop tables"
    );
    let items = db
        .collection("ItemInfo")
        .context("Missing ItemInfo collection")?;
    let mut rows = Vec::new();
    for row in c
        .records
        .iter()
        .filter(|r| c.int(r, "Monster") == Some(i64::from(donor_id)))
    {
        let item_id = number(c, row, "Item")?;
        let item = items
            .records
            .iter()
            .find(|r| item_id > 0 && items.index(r) == item_id)
            .with_context(|| format!("Drop references missing item {item_id}"))?;
        let chance = number(c, row, "Chance")?;
        let amount = number(c, row, "Amount")?;
        ensure!(
            chance >= 0,
            "Drop chance must be nonnegative (zero disables a row)"
        );
        ensure!(
            amount > 0 && i64::from(amount) + i64::from(amount / 2) - 1 <= i64::from(i32::MAX),
            "Drop amount must be positive and fit the runtime quantity roll"
        );
        let set = number(c, row, "DropSet")?;
        let Some(Value::Bool(easter_event)) = c.get(row, "EasterEvent") else {
            anyhow::bail!("Invalid DropInfo.EasterEvent");
        };
        let mut flags = Vec::new();
        if set != 0 {
            flags.push(format!("DropSet mask {set:#010x} required"));
        }
        if *easter_event {
            flags.push("Easter-marked spawn required".into());
        }
        if chance == 0 {
            flags.push("Disabled (chance 0)".to_owned());
        }
        let Some(Value::Bool(part_only)) = c.get(row, "PartOnly") else {
            anyhow::bail!("Invalid DropInfo.PartOnly");
        };
        if *part_only {
            flags.push("PartOnly: skipped by Rust loot rolls".into());
        }
        rows.push(DropRow {
            item_id,
            item_name: items.str_or(item, "ItemName", "Unnamed").into(),
            category: category(items.int_or(item, "ItemType", 0)).into(),
            chance,
            amount,
            drop_set: set,
            easter_event: *easter_event,
            part_only: *part_only,
            restrictions: if flags.is_empty() {
                "None".into()
            } else {
                flags.join("; ")
            },
        });
    }
    ensure!(!rows.is_empty(), "Donor has no monster drop rows");
    Ok(Draft {
        target_id, donor_id, donor_name: monsters.str_or(donor, "MonsterName", "Unnamed").into(), rows,
        notes: "Copies selected donor monster DropInfo rows, with unchanged base odds and amount. Amount is a randomized quantity parameter, not a guaranteed count. Group sharing and account drop progress affect realized loot. Quest rewards/quest monster links, global/world drops, and rate settings are not copied. DropSet masks require the monster spawn's matching bits; EasterEvent rows require an Easter-marked spawn. PartOnly rows are skipped.".into(),
        snapshot: db.to_bytes()?,
    })
}

impl Draft {
    pub fn apply(&self, db: &mut MirDb) -> Result<()> {
        self.apply_selected(db, &(0..self.rows.len()).collect::<Vec<_>>())
    }

    /// Selection indexes the authored donor rows, not the editable preview fields.
    /// Copy in source order regardless of the order the UI selected rows.
    pub fn apply_selected(&self, db: &mut MirDb, indices: &[usize]) -> Result<()> {
        ensure!(
            db.to_bytes()? == self.snapshot,
            "Database changed since this preview; generate a new draft"
        );
        // Revalidate even if a caller changed the public preview fields.
        let validated = propose(db, self.target_id, self.donor_id)?;
        ensure!(!indices.is_empty(), "Select at least one drop row");
        let selected: std::collections::BTreeSet<_> = indices.iter().copied().collect();
        ensure!(
            selected.len() == indices.len(),
            "Duplicate drop row selection"
        );
        ensure!(
            selected.iter().all(|&index| index < validated.rows.len()),
            "Drop row selection is out of range"
        );
        let next = MirDb::parse(&self.snapshot)?;
        let c = schema(&next)?;
        let rows: Vec<_> = c
            .records
            .iter()
            .filter(|r| c.int(r, "Monster") == Some(i64::from(self.donor_id)))
            .enumerate()
            .filter(|(index, _)| selected.contains(index))
            .map(|(_, row)| row.clone())
            .collect();
        commit_rows(db, next, self.target_id, rows)
    }
}

fn commit_rows(db: &mut MirDb, mut next: MirDb, target_id: i32, rows: Vec<Record>) -> Result<()> {
    let c = schema(&next)?;
    let indexed = c.property_index("Index").is_some();
    if indexed {
        let start = i64::from(c.next_index)
            .max(i64::from(c.records.iter().map(|r| c.index(r)).max().unwrap_or(0)) + 1);
        ensure!(
            start > 0 && start + rows.len() as i64 <= i64::from(i32::MAX),
            "Drop identities exhausted"
        );
    }
    let c = next.collection_mut("DropInfo").unwrap();
    for mut row in rows {
        ensure!(
            c.set(&mut row, "Monster", Value::Int(target_id.into())),
            "Invalid drop row"
        );
        if indexed {
            let id = c.allocate_index();
            ensure!(
                c.set(&mut row, "Index", Value::Int(id.into())),
                "Invalid drop identity"
            );
        }
        c.records.push(row);
    }
    *db = MirDb::parse(&next.to_bytes()?)?;
    Ok(())
}
/// Build a reviewable tier template plus explicitly chosen biome materials.
/// The database does not define biome membership: the user chooses that donor.
pub fn tier_material_recipe(db: &MirDb, target: i32, tier: i32, materials: i32) -> Result<Recipe> {
    let tier = propose(db, target, tier)?;
    let materials = propose(db, target, materials)?;
    let tier_rows: Vec<_> = tier
        .rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| (r.category != "Materials").then_some(i))
        .collect();
    let material_rows: Vec<_> = materials
        .rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| (r.category == "Materials").then_some(i))
        .collect();
    ensure!(
        !tier_rows.is_empty(),
        "The tier donor has no non-material drops"
    );
    ensure!(
        !material_rows.is_empty(),
        "The material donor has no material drops"
    );
    let mut recipe = Recipe::new(&tier);
    recipe.add(&tier, &tier_rows)?;
    recipe.add(&materials, &material_rows)?;
    Ok(recipe)
}

/// One queued authored row. Repeated items remain separate, independent rolls.
pub struct RecipeRow {
    pub donor_name: String,
    pub row: DropRow,
    donor_id: i32,
    source_row: usize,
}

pub struct Recipe {
    target_id: i32,
    snapshot: Vec<u8>,
    entries: Vec<RecipeRow>,
}

impl Recipe {
    pub fn new(draft: &Draft) -> Self {
        Self {
            target_id: draft.target_id,
            snapshot: draft.snapshot.clone(),
            entries: Vec::new(),
        }
    }
    pub fn target_id(&self) -> i32 {
        self.target_id
    }
    pub fn entries(&self) -> &[RecipeRow] {
        &self.entries
    }
    pub fn remove(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries.remove(index);
        }
    }
    pub fn add(&mut self, draft: &Draft, indices: &[usize]) -> Result<()> {
        ensure!(
            draft.target_id == self.target_id,
            "This loot draft belongs to another monster; apply or discard it first"
        );
        ensure!(
            draft.snapshot == self.snapshot,
            "Database changed; discard the queued loot and preview again"
        );
        let db = MirDb::parse(&self.snapshot)?;
        let valid = propose(&db, self.target_id, draft.donor_id)?;
        let selected: std::collections::BTreeSet<_> = indices.iter().copied().collect();
        ensure!(!selected.is_empty(), "Select at least one row to queue");
        ensure!(selected.len() == indices.len(), "Duplicate row selection");
        ensure!(
            selected.iter().all(|i| *i < valid.rows.len()),
            "Row selection is out of range"
        );
        ensure!(
            !self
                .entries
                .iter()
                .any(|e| e.donor_id == draft.donor_id && selected.contains(&e.source_row)),
            "A selected donor row is already queued; remove it or deselect it before adding"
        );
        for index in selected {
            self.entries.push(RecipeRow {
                donor_name: valid.donor_name.clone(),
                row: valid.rows[index].clone(),
                donor_id: draft.donor_id,
                source_row: index,
            });
        }
        Ok(())
    }
    pub fn apply(&self, db: &mut MirDb) -> Result<()> {
        ensure!(!self.entries.is_empty(), "Queue at least one drop");
        ensure!(
            db.to_bytes()? == self.snapshot,
            "Database changed; discard the queued loot and preview again"
        );
        let mut donors = std::collections::BTreeMap::new();
        for entry in &self.entries {
            if let std::collections::btree_map::Entry::Vacant(slot) = donors.entry(entry.donor_id) {
                propose(db, self.target_id, entry.donor_id)?;
                let c = schema(db)?;
                slot.insert(
                    c.records
                        .iter()
                        .filter(|r| c.int(r, "Monster") == Some(i64::from(entry.donor_id)))
                        .cloned()
                        .collect::<Vec<_>>(),
                );
            }
        }
        let rows = self
            .entries
            .iter()
            .map(|e| donors[&e.donor_id][e.source_row].clone())
            .collect();
        commit_rows(db, MirDb::parse(&self.snapshot)?, self.target_id, rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Option<(MirDb, i32, i32)> {
        let assets = std::env::var_os("ZIRCON_ASSETS")?;
        let mut db =
            MirDb::load(std::path::Path::new(&assets).join("../Database/System.db")).unwrap();
        let c = db.collection("MonsterInfo").unwrap();
        let donor = c.index(
            c.records
                .iter()
                .find(|r| c.str_or(r, "MonsterName", "") == "Chicken")
                .unwrap(),
        );
        let target = crate::monster_draft::propose(&db, donor, 20, "Drop Draft Test")
            .unwrap()
            .apply(&mut db)
            .unwrap();
        let donor = candidates(&db, target)
            .into_iter()
            .find(|candidate| propose(&db, target, candidate.monster_id).is_ok())
            .expect("At least one supported authored drop table")
            .monster_id;
        Some((db, target, donor))
    }
    fn yield_row(amount: i32, chance: i32) -> DropRow {
        DropRow {
            item_id: 1,
            item_name: "Test item".into(),
            category: "Materials".into(),
            amount,
            chance,
            restrictions: "None".into(),
            drop_set: 0,
            easter_event: false,
            part_only: false,
        }
    }
    #[test]
    fn yield_mean_matches_runtime_quantity_enumeration() {
        for amount in 1..100 {
            let row = yield_row(amount, 5);
            let mean = (0..amount)
                .map(|n| f64::from((amount / 2 + n).max(1)))
                .sum::<f64>()
                / f64::from(amount);
            let summary = yield_summary([&row]);
            assert!((summary.items[&1].ordinary - mean * 20.0).abs() < 1e-9);
            assert_eq!(summary.items[&1].conditional, 0.0);
        }
        let row = yield_row(i32::MAX / 2, i32::MAX);
        assert!(yield_summary([&row]).items[&1].ordinary.is_finite());
    }
    #[test]
    fn yield_summary_groups_items_without_merging_rolls_or_gates() {
        let normal = yield_row(2, 2); // 75 units per 100 base rolls.
        let mut gated = normal.clone();
        gated.drop_set = i32::MIN;
        let mut event = normal.clone();
        event.easter_event = true;
        let mut disabled = normal.clone();
        disabled.chance = 0;
        let mut part = normal.clone();
        part.part_only = true;
        let mut other = normal.clone();
        other.item_id = 2;
        // Display labels must not control gate eligibility.
        gated.restrictions = "None".into();
        let summary = yield_summary([&normal, &normal, &gated, &event, &disabled, &part, &other]);
        assert_eq!(summary.items.len(), 2);
        assert_eq!(summary.items[&1].ordinary, 150.0);
        assert_eq!(summary.items[&1].conditional, 150.0);
        assert_eq!(summary.items[&2].ordinary, 75.0);
        assert_eq!((summary.disabled, summary.part_only), (1, 1));
        assert!(yield_summary([]).items.is_empty());
    }

    #[test]
    fn asset_tier_material_composition_selects_categories_and_preserves_values() {
        let Some((mut db, target, _)) = fixture() else {
            return;
        };
        let donors = candidates(&db, target);
        let mixed: Vec<_> = donors
            .iter()
            .filter(|t| t.material_count > 0 && t.material_count < t.row_count)
            .filter_map(|t| propose(&db, target, t.monster_id).ok())
            .take(2)
            .collect();
        assert_eq!(mixed.len(), 2, "Two mixed authored donors required");
        for t in &donors {
            if let Ok(draft) = propose(&db, target, t.monster_id) {
                assert_eq!(
                    t.material_count,
                    draft
                        .rows
                        .iter()
                        .filter(|r| r.category == "Materials")
                        .count()
                );
            }
        }
        let original = db.to_bytes().unwrap();
        let recipe =
            tier_material_recipe(&db, target, mixed[0].donor_id, mixed[1].donor_id).unwrap();
        let expected: Vec<_> = mixed[0]
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.category != "Materials")
            .map(|(i, _)| (mixed[0].donor_id, i))
            .chain(
                mixed[1]
                    .rows
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.category == "Materials")
                    .map(|(i, _)| (mixed[1].donor_id, i)),
            )
            .collect();
        assert_eq!(
            recipe
                .entries()
                .iter()
                .map(|e| (e.donor_id, e.source_row))
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            db.to_bytes().unwrap(),
            original,
            "Drafting must not mutate the DB"
        );
        recipe.apply(&mut db).unwrap();
        let c = db.collection("DropInfo").unwrap();
        let copied: Vec<_> = c
            .records
            .iter()
            .filter(|r| c.int(r, "Monster") == Some(target.into()))
            .collect();
        assert_eq!(copied.len(), expected.len());
        for ((donor, index), actual) in expected.into_iter().zip(copied) {
            let mut source = c
                .records
                .iter()
                .filter(|r| c.int(r, "Monster") == Some(donor.into()))
                .nth(index)
                .unwrap()
                .clone();
            c.set(&mut source, "Monster", Value::Int(target.into()));
            let id = c.property_index("Index").unwrap();
            source.values[id] = actual.values[id].clone();
            assert_eq!(source.values, actual.values);
        }
        assert!(recipe.apply(&mut db).is_err());
        let db = MirDb::parse(&original).unwrap();
        let same = tier_material_recipe(&db, target, mixed[0].donor_id, mixed[0].donor_id).unwrap();
        assert_eq!(same.entries().len(), mixed[0].rows.len());
        let unique: std::collections::BTreeSet<_> = same
            .entries()
            .iter()
            .map(|e| (e.donor_id, e.source_row))
            .collect();
        assert_eq!(unique.len(), same.entries().len());
    }

    #[test]
    fn asset_tier_material_composition_rejects_missing_categories_atomically() {
        let Some((mut db, target, donor)) = fixture() else {
            return;
        };
        let material = {
            let c = db.collection("ItemInfo").unwrap();
            c.index(
                c.records
                    .iter()
                    .find(|r| category(c.int_or(r, "ItemType", 0)) == "Materials")
                    .unwrap(),
            )
        };
        let c = db.collection_mut("DropInfo").unwrap();
        let item = c.property_index("Item").unwrap();
        let monster = c.property_index("Monster").unwrap();
        for row in &mut c.records {
            if row.values[monster].as_i64() == Some(donor.into()) {
                row.values[item] = Value::Int(material.into());
            }
        }
        let original = db.to_bytes().unwrap();
        let error = tier_material_recipe(&db, target, donor, donor)
            .err()
            .unwrap();
        assert!(error.to_string().contains("no non-material"));
        assert!(tier_material_recipe(&db, target, donor, i32::MAX).is_err());
        let non_material = {
            let c = db.collection("ItemInfo").unwrap();
            c.index(
                c.records
                    .iter()
                    .find(|r| category(c.int_or(r, "ItemType", 0)) == "Consumables")
                    .unwrap(),
            )
        };
        assert_eq!(db.to_bytes().unwrap(), original);
        let c = db.collection_mut("DropInfo").unwrap();
        for row in &mut c.records {
            if row.values[monster].as_i64() == Some(donor.into()) {
                row.values[item] = Value::Int(non_material.into());
            }
        }
        let original = db.to_bytes().unwrap();
        assert!(tier_material_recipe(&db, target, donor, donor)
            .err()
            .unwrap()
            .to_string()
            .contains("no material"));
        assert_eq!(db.to_bytes().unwrap(), original);
    }

    #[test]
    fn asset_recipe_composes_donors_and_preserves_source_records() {
        let Some((mut db, target, donor)) = fixture() else {
            return;
        };
        let other = candidates(&db, target)
            .iter()
            .find(|t| t.monster_id != donor && propose(&db, target, t.monster_id).is_ok())
            .unwrap()
            .monster_id;
        // Identical items with different conditions must remain distinct authored rolls.
        let c = db.collection_mut("DropInfo").unwrap();
        let source = c
            .records
            .iter()
            .find(|r| c.int(r, "Monster") == Some(donor.into()))
            .unwrap()
            .clone();
        let pos = c
            .records
            .iter()
            .position(|r| c.int(r, "Monster") == Some(other.into()))
            .unwrap();
        let item = c.property_index("Item").unwrap();
        c.records[pos].values[item] = source.values[item].clone();
        let mask = c.property_index("DropSet").unwrap();
        c.records[pos].values[mask] = Value::Int(7);
        let first = propose(&db, target, donor).unwrap();
        let second = propose(&db, target, other).unwrap();
        let original = db.to_bytes().unwrap();
        let mut recipe = Recipe::new(&first);
        recipe.add(&second, &[0]).unwrap();
        recipe.add(&first, &[0]).unwrap();
        assert_eq!(
            recipe.entries()[0].row.item_id,
            recipe.entries()[1].row.item_id
        );
        recipe.remove(0);
        recipe.add(&second, &[0]).unwrap();
        recipe.apply(&mut db).unwrap();
        let c = db.collection("DropInfo").unwrap();
        let copied: Vec<_> = c
            .records
            .iter()
            .filter(|r| c.int(r, "Monster") == Some(target.into()))
            .collect();
        assert_eq!(copied.len(), 2);
        for (donor, actual) in [donor, other].into_iter().zip(copied) {
            let mut expected = c
                .records
                .iter()
                .find(|r| c.int(r, "Monster") == Some(donor.into()))
                .unwrap()
                .clone();
            c.set(&mut expected, "Monster", Value::Int(target.into()));
            let index = c.property_index("Index").unwrap();
            assert_ne!(expected.values[index], actual.values[index]);
            expected.values[index] = actual.values[index].clone();
            assert_eq!(expected.values, actual.values);
        }
        let before = MirDb::parse(&original).unwrap();
        for (a, b) in before.collections.iter().zip(&db.collections) {
            assert!(a
                .records
                .iter()
                .zip(&b.records)
                .all(|(a, b)| a.values == b.values));
            if a.short_name() != "DropInfo" {
                assert_eq!(a.records.len(), b.records.len());
                assert_eq!(a.next_index, b.next_index);
            }
        }
    }

    #[test]
    fn asset_recipe_rejects_invalid_adds_and_stale_apply_atomically() {
        let Some((mut db, target, donor)) = fixture() else {
            return;
        };
        let draft = propose(&db, target, donor).unwrap();
        let mut recipe = Recipe::new(&draft);
        let original = db.to_bytes().unwrap();
        assert!(recipe.apply(&mut db).is_err());
        assert_eq!(db.to_bytes().unwrap(), original);
        recipe.add(&draft, &[0]).unwrap();
        for selection in [vec![], vec![0], vec![0, 0], vec![usize::MAX]] {
            assert!(recipe.add(&draft, &selection).is_err());
            assert_eq!(recipe.entries().len(), 1);
        }
        let mut wrong = propose(&db, target, donor).unwrap();
        wrong.target_id = donor;
        assert!(recipe.add(&wrong, &[0]).is_err());
        assert_eq!(recipe.entries().len(), 1);
        db.collection_mut("MonsterInfo").unwrap().next_index += 1;
        let changed = db.to_bytes().unwrap();
        let newer = propose(&db, target, donor).unwrap();
        assert!(recipe.add(&newer, &[0]).is_err());
        assert!(recipe.apply(&mut db).is_err());
        assert_eq!(db.to_bytes().unwrap(), changed);
        assert_eq!(recipe.entries().len(), 1);
    }

    #[test]
    fn asset_template_roundtrip_preserves_authored_rows_and_server_loot() {
        let Some((mut db, target, donor)) = fixture() else {
            return;
        };
        let original = db.to_bytes().unwrap();
        let templates = candidates(&db, target);
        assert!(templates.iter().any(|t| t.monster_id == donor));
        let draft = propose(&db, target, donor).unwrap();
        assert!(!draft.rows.is_empty());
        assert_eq!(db.to_bytes().unwrap(), original);
        draft.apply(&mut db).unwrap();
        let before = MirDb::parse(&original).unwrap();
        for c in &before.collections {
            let after = db.collection(c.short_name()).unwrap();
            if c.short_name() != "DropInfo" || c.property_index("Index").is_none() {
                assert_eq!(c.next_index, after.next_index);
            }
            assert!(c
                .records
                .iter()
                .zip(&after.records)
                .all(|(a, b)| a.values == b.values));
            assert_eq!(
                after.records.len(),
                c.records.len()
                    + if c.short_name() == "DropInfo" {
                        draft.rows.len()
                    } else {
                        0
                    }
            );
        }
        let c = db.collection("DropInfo").unwrap();
        if c.property_index("Index").is_some() {
            let ids: std::collections::BTreeSet<_> = c.records.iter().map(|r| c.index(r)).collect();
            assert_eq!(ids.len(), c.records.len());
        }
        let source: Vec<_> = c
            .records
            .iter()
            .filter(|r| c.int(r, "Monster") == Some(donor.into()))
            .collect();
        let copied: Vec<_> = c
            .records
            .iter()
            .filter(|r| c.int(r, "Monster") == Some(target.into()))
            .collect();
        for (a, b) in source.iter().zip(copied) {
            let mut expected = (*a).clone();
            c.set(&mut expected, "Monster", Value::Int(target.into()));
            if let Some(index) = c.property_index("Index") {
                assert_ne!(expected.values[index], b.values[index]);
                expected.values[index] = b.values[index].clone();
            }
            assert_eq!(expected.values, b.values);
        }
        let loaded = mir_server::data::GameData::from_db(&db).unwrap();
        let source: Vec<_> = loaded.drops.iter().filter(|r| r.monster == donor).collect();
        let copied: Vec<_> = loaded
            .drops
            .iter()
            .filter(|r| r.monster == target)
            .collect();
        assert_eq!(source.len(), copied.len());
        for (a, b) in source.into_iter().zip(copied) {
            assert_eq!(
                (
                    a.item,
                    a.chance,
                    a.amount,
                    a.drop_set,
                    a.part_only,
                    a.easter_event
                ),
                (
                    b.item,
                    b.chance,
                    b.amount,
                    b.drop_set,
                    b.part_only,
                    b.easter_event
                )
            );
        }
        let applied = db.to_bytes().unwrap();
        assert!(draft.apply(&mut db).is_err());
        assert!(propose(&db, target, donor).is_err());
        assert_eq!(db.to_bytes().unwrap(), applied);
    }
    #[test]
    fn asset_invalid_rows_and_stale_previews_are_atomic() {
        let Some((db, target, donor)) = fixture() else {
            return;
        };
        let original = db.to_bytes().unwrap();
        let draft = propose(&db, target, donor).unwrap();
        for (field, value) in [
            ("Item", Value::Int(i32::MAX.into())),
            ("Chance", Value::Int(-1)),
            ("Amount", Value::Int(0)),
            ("Amount", Value::Int(i32::MAX.into())),
        ] {
            let mut changed = MirDb::parse(&original).unwrap();
            let c = changed.collection_mut("DropInfo").unwrap();
            let pos = c
                .records
                .iter()
                .position(|r| c.int(r, "Monster") == Some(donor.into()))
                .unwrap();
            let index = c.property_index(field).unwrap();
            c.records[pos].values[index] = value;
            let bytes = changed.to_bytes().unwrap();
            assert!(propose(&changed, target, donor).is_err());
            assert!(draft.apply(&mut changed).is_err());
            assert_eq!(changed.to_bytes().unwrap(), bytes);
        }
        assert!(propose(&db, donor, donor).is_err());
        assert!(propose(&db, i32::MAX, donor).is_err());
    }
    #[test]
    fn asset_selection_copies_only_original_rows_in_authored_order() {
        let Some((mut db, target, _)) = fixture() else {
            return;
        };
        let donor = candidates(&db, target)
            .into_iter()
            .find(|t| t.row_count >= 3 && propose(&db, target, t.monster_id).is_ok())
            .expect("Authored table with at least three rows")
            .monster_id;
        let original = db.to_bytes().unwrap();
        let mut draft = propose(&db, target, donor).unwrap();
        let last = draft.rows.len() - 1;
        let c = db.collection("DropInfo").unwrap();
        let expected: Vec<_> = c
            .records
            .iter()
            .filter(|r| c.int(r, "Monster") == Some(donor.into()))
            .enumerate()
            .filter(|(i, _)| *i == 0 || *i == last)
            .map(|(_, r)| r.clone())
            .collect();
        // Preview changes must never alter authored values copied into the DB.
        draft.rows[0].amount = 9999;
        draft.rows[0].item_name = "Changed preview".into();
        draft.apply_selected(&mut db, &[last, 0]).unwrap();
        let c = db.collection("DropInfo").unwrap();
        let copied: Vec<_> = c
            .records
            .iter()
            .filter(|r| c.int(r, "Monster") == Some(target.into()))
            .collect();
        assert_eq!(copied.len(), 2);
        for (mut expected, actual) in expected.into_iter().zip(copied) {
            c.set(&mut expected, "Monster", Value::Int(target.into()));
            if let Some(index) = c.property_index("Index") {
                expected.values[index] = actual.values[index].clone();
            }
            assert_eq!(expected.values, actual.values);
        }
        let before = MirDb::parse(&original).unwrap();
        let c_before = before.collection("DropInfo").unwrap();
        assert!(c_before
            .records
            .iter()
            .zip(&c.records)
            .all(|(a, b)| a.values == b.values));
    }

    #[test]
    fn asset_invalid_selections_are_atomic() {
        let Some((mut db, target, donor)) = fixture() else {
            return;
        };
        let original = db.to_bytes().unwrap();
        let draft = propose(&db, target, donor).unwrap();
        for selection in [vec![], vec![0, 0], vec![draft.rows.len()], vec![usize::MAX]] {
            assert!(draft.apply_selected(&mut db, &selection).is_err());
            assert_eq!(db.to_bytes().unwrap(), original);
        }
        let c = db.collection_mut("MonsterInfo").unwrap();
        c.next_index += 1;
        let changed = db.to_bytes().unwrap();
        assert!(draft.apply_selected(&mut db, &[0]).is_err());
        assert_eq!(db.to_bytes().unwrap(), changed);
    }

    #[test]
    fn category_groups_match_item_types_and_keep_unknowns_visible() {
        assert_eq!(category(1), "Consumables");
        assert_eq!(category(2), "Equipment");
        assert_eq!(category(13), "Materials");
        for value in [0, 24, 34, 255, -1] {
            assert_eq!(category(value), "Other");
        }
    }

    #[test]
    fn asset_restricted_and_disabled_rows_are_preserved() {
        let Some((db, target, donor)) = fixture() else {
            return;
        };
        let original = db.to_bytes().unwrap();
        for (field, value) in [
            ("DropSet", Value::Int(7)),
            ("EasterEvent", Value::Bool(true)),
        ] {
            let mut changed = MirDb::parse(&original).unwrap();
            let c = changed.collection_mut("DropInfo").unwrap();
            let pos = c
                .records
                .iter()
                .position(|r| c.int(r, "Monster") == Some(donor.into()))
                .unwrap();
            let index = c.property_index(field).unwrap();
            c.records[pos].values[index] = value;
            let bytes = changed.to_bytes().unwrap();
            let draft = propose(&changed, target, donor).unwrap();
            assert_eq!(changed.to_bytes().unwrap(), bytes);
            assert!(draft.rows[0].restrictions.contains(if field == "DropSet" {
                "mask 0x00000007 required"
            } else {
                "Easter-marked spawn required"
            }));
            draft.apply(&mut changed).unwrap();
            let loaded = mir_server::data::GameData::from_db(&changed).unwrap();
            let copied = loaded.drops.iter().find(|r| r.monster == target).unwrap();
            if field == "DropSet" {
                assert_eq!(copied.drop_set, 7);
            } else {
                assert!(copied.easter_event);
            }
        }
        let mut db = db;
        let c = db.collection_mut("DropInfo").unwrap();
        let pos = c
            .records
            .iter()
            .position(|r| c.int(r, "Monster") == Some(donor.into()))
            .unwrap();
        for (field, value) in [("Chance", Value::Int(0)), ("PartOnly", Value::Bool(true))] {
            let index = c.property_index(field).unwrap();
            c.records[pos].values[index] = value;
        }
        let draft = propose(&db, target, donor).unwrap();
        for word in ["Disabled", "PartOnly"] {
            assert!(draft.rows[0].restrictions.contains(word));
        }
        draft.apply(&mut db).unwrap();
        let c = db.collection("DropInfo").unwrap();
        let copied = c
            .records
            .iter()
            .find(|r| c.int(r, "Monster") == Some(target.into()))
            .unwrap();
        assert_eq!(c.int(copied, "Chance"), Some(0));
        assert!(c.bool_or(copied, "PartOnly", false));
    }
}
