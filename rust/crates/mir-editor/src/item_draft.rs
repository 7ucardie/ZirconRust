//! Local equipment variants, with bounded estimates from comparable existing items.
use anyhow::{ensure, Context, Result};
use mir_formats::mirdb::{Collection, MirDb, Record, Value};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct StatChange {
    pub stat: i32,
    pub label: String,
    pub before: i32,
    pub after: i32,
}
#[derive(Debug)]
pub struct Draft {
    pub source_id: i32,
    pub name: String,
    pub source_level: i32,
    pub target_level: i32,
    pub slot: String,
    pub changes: Vec<StatChange>,
    pub budget_before: f64,
    pub budget_after: f64,
    pub reference_count: usize,
    pub method: String,
    snapshot: Vec<u8>,
}
fn schemas(db: &MirDb) -> Result<(&Collection, &Collection)> {
    let items = db.collection("ItemInfo").context("Missing ItemInfo")?;
    let stats = db
        .collection("ItemInfoStat")
        .context("Missing ItemInfoStat")?;
    for (c, fields) in [
        (
            items,
            &[
                "Index",
                "ItemName",
                "ItemType",
                "RequiredType",
                "RequiredAmount",
                "RequiredClass",
                "Rarity",
                "StartItem",
            ][..],
        ),
        (stats, &["Index", "Item", "Stat", "Amount"][..]),
    ] {
        for field in fields {
            ensure!(
                c.property_index(field).is_some(),
                "Missing {}.{field}",
                c.short_name()
            );
        }
    }
    Ok((items, stats))
}
fn number(c: &Collection, r: &Record, field: &str) -> Result<i32> {
    i32::try_from(
        c.int(r, field)
            .with_context(|| format!("Invalid {}.{field}", c.short_name()))?,
    )
    .context("Integer out of range")
}
fn slot(kind: i32) -> Option<&'static str> {
    Some(match kind {
        2 => "Weapon",
        3 => "Armour",
        5 => "Helmet",
        6 => "Necklace",
        7 => "Bracelet",
        8 => "Ring",
        9 => "Shoes",
        18 => "Horse armour",
        26 => "Emblem",
        27 => "Shield",
        _ => return None,
    })
}
fn label(stat: i32) -> &'static str {
    match stat {
        2 => "Health",
        3 => "Mana",
        4 => "Minimum AC",
        5 => "Maximum AC",
        6 => "Minimum MR",
        7 => "Maximum MR",
        8 => "Minimum DC",
        9 => "Maximum DC",
        10 => "Minimum MC",
        11 => "Maximum MC",
        12 => "Minimum SC",
        13 => "Maximum SC",
        14 => "Accuracy",
        15 => "Agility",
        _ => "Other (preserved)",
    }
}
fn budget(values: &BTreeMap<i32, i32>) -> f64 {
    values
        .range(2..=15)
        .map(|(&stat, &value)| f64::from(value.max(0)) / if stat <= 3 { 10.0 } else { 1.0 })
        .sum()
}
fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n.is_multiple_of(2) {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    } else {
        v[n / 2]
    }
}
fn interpolate(curve: &BTreeMap<i32, Vec<f64>>, level: i32) -> Option<f64> {
    let (&lo, a) = curve.range(..=level).next_back()?;
    let (&hi, b) = curve.range(level..).next()?;
    let a = median(a);
    Some(if lo == hi {
        a
    } else {
        a + (median(b) - a) * f64::from(level - lo) / f64::from(hi - lo)
    })
}
fn factor(curve: &BTreeMap<i32, Vec<f64>>, source: i32, target: i32) -> (f64, bool) {
    match (interpolate(curve, source), interpolate(curve, target)) {
        (Some(a), Some(b)) if a > 0.0 && b > 0.0 => ((b / a).clamp(0.25, 4.0), true),
        _ => (
            (f64::from(target) / f64::from(source)).clamp(0.25, 4.0),
            false,
        ),
    }
}
pub fn propose(db: &MirDb, source_id: i32, target_level: i32, name: &str) -> Result<Draft> {
    ensure!(
        (1..=255).contains(&target_level),
        "Target level must be between 1 and 255"
    );
    let name = name.trim();
    ensure!(
        !name.is_empty() && name.chars().count() <= 40 && !name.chars().any(char::is_control),
        "Use a name of 1–40 characters without control characters"
    );
    let (items, stats) = schemas(db)?;
    ensure!(
        !items
            .records
            .iter()
            .any(|r| items.str_or(r, "ItemName", "").eq_ignore_ascii_case(name)),
        "An item with this name already exists"
    );
    let source = items
        .records
        .iter()
        .find(|r| items.index(r) == source_id)
        .context("Source item missing")?;
    let kind = number(items, source, "ItemType")?;
    let slot = slot(kind).context("Select a weapon, armour or combat accessory")?;
    ensure!(number(items, source, "RequiredType")? == 0, "Source must use a minimum level requirement; stat, maximum-level and rebirth requirements are preserved by ordinary cloning only");
    let source_level = number(items, source, "RequiredAmount")?;
    ensure!(
        (1..=255).contains(&source_level),
        "Source needs a minimum level between 1 and 255"
    );
    let class = number(items, source, "RequiredClass")?;
    let rarity = number(items, source, "Rarity")?;
    let mut amounts = BTreeMap::<i32, BTreeMap<i32, i32>>::new();
    for row in &stats.records {
        let item = number(stats, row, "Item")?;
        let stat = number(stats, row, "Stat")?;
        let value = number(stats, row, "Amount")?;
        // Match the current Rust loader for reference rows. Reject duplicate source stats
        // because the C# reference sums them while the Rust loader keeps the last row.
        let item_stats = amounts.entry(item).or_default();
        ensure!(
            item != source_id || !item_stats.contains_key(&stat),
            "Duplicate source stat {stat}"
        );
        item_stats.insert(stat, value);
    }
    let source_stats = amounts
        .get(&source_id)
        .context("Source item has no stats")?;
    let budget_before = budget(source_stats);
    ensure!(
        budget_before > 0.0,
        "Source needs positive health, mana, defence, attack, accuracy or agility stats"
    );
    for min in [4, 6, 8, 10, 12] {
        ensure!(
            source_stats.get(&min).copied().unwrap_or(0)
                <= source_stats.get(&(min + 1)).copied().unwrap_or(0),
            "Source has inverted {} / {}",
            label(min),
            label(min + 1)
        );
    }
    let mut curve = BTreeMap::<i32, Vec<f64>>::new();
    let mut reference_count = 0;
    for r in &items.records {
        let level = items.int_or(r, "RequiredAmount", 0) as i32;
        if items.int(r, "ItemType") != Some(i64::from(kind))
            || items.int(r, "RequiredType") != Some(0)
            || items.int(r, "RequiredClass") != Some(i64::from(class))
            || items.int(r, "Rarity") != Some(i64::from(rarity))
            || !(1..=255).contains(&level)
        {
            continue;
        }
        let value = amounts.get(&items.index(r)).map(budget).unwrap_or(0.0);
        if value > 0.0 {
            curve.entry(level).or_default().push(value);
            reference_count += usize::from(items.index(r) != source_id);
        }
    }
    let (scale, observed) = factor(&curve, source_level, target_level);
    let changes: Vec<_> = source_stats
        .iter()
        .map(|(&stat, &before)| {
            let after = if (2..=15).contains(&stat) && before > 0 {
                (f64::from(before) * scale)
                    .round()
                    .clamp(1.0, f64::from(i32::MAX)) as i32
            } else {
                before
            };
            StatChange {
                stat,
                label: label(stat).into(),
                before,
                after,
            }
        })
        .collect();
    let budget_after = budget(&changes.iter().map(|c| (c.stat, c.after)).collect());
    Ok(Draft { source_id, name: name.into(), source_level, target_level, slot: slot.into(), changes, budget_before, budget_after, reference_count,
        method: format!("{}; factor {scale:.3}, limited to ¼–4. Budget is positive core stat points (health/mana ÷10), not measured combat power. One factor preserves the source profile; rounding changes the final total. Speed, special/negative stats, appearance, price, durability and restrictions stay unchanged. Automatic starting-item grants are disabled for the new variant. No recipes or drops copied.", if observed { "Interpolated median budgets for the same slot, class requirement and rarity" } else { "Insufficient level brackets: level-ratio estimate" }), snapshot: db.to_bytes()? })
}
impl Draft {
    pub fn apply(&self, db: &mut MirDb) -> Result<i32> {
        ensure!(
            db.to_bytes()? == self.snapshot,
            "Database changed since preview; generate a new draft"
        );
        // Recompute so mutable public preview fields cannot smuggle invalid stat values into the database.
        let validated = propose(db, self.source_id, self.target_level, &self.name)?;
        let mut next = MirDb::parse(&self.snapshot)?;
        let (items, stats) = schemas(&next)?;
        let mut record = items
            .records
            .iter()
            .find(|r| items.index(r) == self.source_id)
            .context("Source item missing")?
            .clone();
        let mut rows: Vec<_> = stats
            .records
            .iter()
            .filter(|r| stats.int(r, "Item") == Some(i64::from(self.source_id)))
            .cloned()
            .collect();
        for (c, count) in [(items, 1), (stats, rows.len())] {
            let start = i64::from(c.next_index)
                .max(i64::from(c.records.iter().map(|r| c.index(r)).max().unwrap_or(0)) + 1);
            ensure!(
                start > 0 && start + count as i64 <= i64::from(i32::MAX),
                "{} identities exhausted",
                c.short_name()
            );
        }
        let items = next.collection_mut("ItemInfo").unwrap();
        let id = items.allocate_index();
        for (field, value) in [
            ("Index", Value::Int(id.into())),
            ("ItemName", Value::Str(validated.name)),
            ("RequiredAmount", Value::Int(validated.target_level.into())),
            ("StartItem", Value::Bool(false)),
        ] {
            ensure!(items.set(&mut record, field, value), "Invalid item row");
        }
        items.records.push(record);
        let stats = next.collection_mut("ItemInfoStat").unwrap();
        for row in &mut rows {
            let stat = number(stats, row, "Stat")?;
            let index = stats.allocate_index();
            let amount = validated
                .changes
                .iter()
                .find(|c| c.stat == stat)
                .context("Missing draft stat")?
                .after;
            ensure!(
                stats.set(row, "Index", Value::Int(index.into()))
                    && stats.set(row, "Item", Value::Int(id.into()))
                    && stats.set(row, "Amount", Value::Int(amount.into())),
                "Invalid item stat row"
            );
            stats.records.push(row.clone());
        }
        *db = MirDb::parse(&next.to_bytes()?)?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn assets() -> Option<MirDb> {
        std::env::var_os("ZIRCON_ASSETS")
            .map(|p| MirDb::load(std::path::Path::new(&p).join("../Database/System.db")).unwrap())
    }
    fn draft(db: &MirDb) -> Draft {
        let c = db.collection("ItemInfo").unwrap();
        c.records
            .iter()
            .find_map(|r| propose(db, c.index(r), 30, "Item Draft Fixture").ok())
            .expect("At least one draftable source")
    }
    #[test]
    fn budgets_interpolate_with_bounded_fallback() {
        let curve = BTreeMap::from([(10, vec![10.0, 20.0, 900.0]), (20, vec![60.0])]);
        assert_eq!(interpolate(&curve, 15), Some(40.0));
        assert_eq!(factor(&curve, 10, 15), (2.0, true));
        assert_eq!(factor(&curve, 10, 255), (4.0, false));
        assert_eq!(factor(&curve, 255, 1), (0.25, false));
        assert_eq!(
            budget(&BTreeMap::from([
                (2, 100),
                (3, 50),
                (9, 4),
                (14, -2),
                (16, 100)
            ])),
            19.0
        );
    }
    #[test]
    fn asset_item_variant_roundtrips_and_preserves_source_and_unrelated_records() {
        let Some(mut db) = assets() else {
            return;
        };
        let original = db.to_bytes().unwrap();
        let draft = draft(&db);
        let source_id = draft.source_id;
        assert_eq!(db.to_bytes().unwrap(), original);
        let id = draft.apply(&mut db).unwrap();
        let saved = db.to_bytes().unwrap();
        if let Some(path) = std::env::var_os("ZIRCON_ITEM_QA_DIR") {
            let path = std::path::PathBuf::from(path);
            std::fs::create_dir(&path).expect("Use a new QA directory");
            std::fs::write(path.join("Preview.db"), &original).unwrap();
            std::fs::write(path.join("System.db"), &saved).unwrap();
            let items = db.collection("ItemInfo").unwrap();
            let source = items
                .records
                .iter()
                .find(|r| items.index(r) == source_id)
                .unwrap();
            eprintln!(
                "Item QA: source {} {:?}, created {}; {}",
                source_id,
                items.str_or(source, "ItemName", ""),
                id,
                path.display()
            );
        }
        assert!(draft.apply(&mut db).is_err());
        assert_eq!(db.to_bytes().unwrap(), saved);
        let before = MirDb::parse(&original).unwrap();
        let data = mir_server::data::GameData::from_db(&db).unwrap();
        assert_eq!(data.items[&id].required_amount, 30);
        let items = db.collection("ItemInfo").unwrap();
        let source = items
            .records
            .iter()
            .find(|r| items.index(r) == source_id)
            .unwrap();
        let created = items.records.iter().find(|r| items.index(r) == id).unwrap();
        assert!(
            items.bool_or(source, "StartItem", false),
            "Fixture source must be starting equipment"
        );
        assert!(!items.bool_or(created, "StartItem", true));
        for (p, (a, b)) in items
            .mapping
            .properties
            .iter()
            .zip(source.values.iter().zip(&created.values))
        {
            if !["Index", "ItemName", "RequiredAmount", "StartItem"].contains(&p.name.as_str()) {
                assert_eq!(a, b, "{}", p.name);
            }
        }
        for c in &before.collections {
            let after = db.collection(c.short_name()).unwrap();
            assert!(c
                .records
                .iter()
                .zip(&after.records)
                .all(|(a, b)| a.values == b.values));
            if !["ItemInfo", "ItemInfoStat"].contains(&c.short_name()) {
                assert_eq!(c.records.len(), after.records.len());
                assert_eq!(c.next_index, after.next_index);
            }
            let ids: std::collections::BTreeSet<_> =
                after.records.iter().map(|r| after.index(r)).collect();
            assert_eq!(ids.len(), after.records.len());
        }
        let stats = db.collection("ItemInfoStat").unwrap();
        for change in &draft.changes {
            let row = stats
                .records
                .iter()
                .find(|r| {
                    stats.int(r, "Item") == Some(i64::from(id))
                        && stats.int(r, "Stat") == Some(i64::from(change.stat))
                })
                .unwrap();
            assert_eq!(stats.int(row, "Amount"), Some(i64::from(change.after)));
        }
    }
    #[test]
    fn asset_peer_budget_matches_last_row_wins_runtime() {
        let Some(mut db) = assets() else {
            return;
        };
        let source_id = draft(&db).source_id;
        let items = db.collection_mut("ItemInfo").unwrap();
        let mut source = items
            .records
            .iter()
            .find(|r| items.index(r) == source_id)
            .unwrap()
            .clone();
        items.set(&mut source, "RequiredAmount", Value::Int(10));
        let mut peer = source.clone();
        let peer_id = items.allocate_index();
        items.set(&mut peer, "Index", Value::Int(peer_id.into()));
        items.set(&mut peer, "RequiredAmount", Value::Int(20));
        items.records = vec![source, peer];
        let stats = db.collection_mut("ItemInfoStat").unwrap();
        let item_field = stats.property_index("Item").unwrap();
        stats
            .records
            .retain(|r| r.values[item_field].as_i64() == Some(i64::from(source_id)));
        let source_rows = stats.records.clone();
        for mut row in source_rows {
            let id = stats.allocate_index();
            stats.set(&mut row, "Index", Value::Int(id.into()));
            stats.set(&mut row, "Item", Value::Int(peer_id.into()));
            stats.records.push(row);
        }
        let before = propose(&db, source_id, 20, "Before Duplicate").unwrap();
        let stats = db.collection_mut("ItemInfoStat").unwrap();
        let mut duplicate = stats
            .records
            .iter()
            .find(|r| {
                stats.int(r, "Item") == Some(i64::from(peer_id))
                    && (2..=15).contains(&stats.int_or(r, "Stat", 0))
                    && stats.int_or(r, "Amount", 0) > 0
            })
            .unwrap()
            .clone();
        let id = stats.allocate_index();
        stats.set(&mut duplicate, "Index", Value::Int(id.into()));
        stats.records.push(duplicate);
        let after = propose(&db, source_id, 20, "After Duplicate").unwrap();
        assert_eq!(before.budget_after, after.budget_after);
        assert_eq!(
            before.changes.iter().map(|c| c.after).collect::<Vec<_>>(),
            after.changes.iter().map(|c| c.after).collect::<Vec<_>>()
        );
    }
    #[test]
    fn asset_invalid_requirements_duplicate_stats_and_exhaustion_are_atomic() {
        let Some(mut db) = assets() else {
            return;
        };
        let source_id = draft(&db).source_id;
        assert!(propose(&db, source_id, 0, "Bad").is_err());
        assert!(propose(&db, source_id, 30, "\n").is_err());
        let original = db.to_bytes().unwrap();
        let items = db.collection_mut("ItemInfo").unwrap();
        let ri = items
            .records
            .iter()
            .position(|r| items.index(r) == source_id)
            .unwrap();
        let p = items.property_index("RequiredType").unwrap();
        items.records[ri].values[p] = Value::Int(2);
        assert!(propose(&db, source_id, 30, "Bad Requirement").is_err());
        db = MirDb::parse(&original).unwrap();
        let stats = db.collection_mut("ItemInfoStat").unwrap();
        let mut row = stats
            .records
            .iter()
            .find(|r| stats.int(r, "Item") == Some(i64::from(source_id)))
            .unwrap()
            .clone();
        let id = stats.allocate_index();
        stats.set(&mut row, "Index", Value::Int(id.into()));
        stats.records.push(row);
        assert!(propose(&db, source_id, 30, "Duplicate").is_err());
        db = MirDb::parse(&original).unwrap();
        db.collection_mut("ItemInfoStat").unwrap().next_index = i32::MAX;
        let draft = propose(&db, source_id, 30, "Exhausted").unwrap();
        let before = db.to_bytes().unwrap();
        assert!(draft.apply(&mut db).is_err());
        assert_eq!(db.to_bytes().unwrap(), before);
    }
}
