//! Reviewable, local monster variants. Only the monster and its stat rows are copied.
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
    pub changes: Vec<StatChange>,
    pub experience_before: f64,
    pub experience_after: f64,
    pub reference_count: usize,
    pub method: String,
    snapshot: Vec<u8>,
    experience_value: Value,
}

fn collection<'a>(db: &'a MirDb, name: &str, fields: &[(&str, &str)]) -> Result<&'a Collection> {
    let c = db
        .collection(name)
        .with_context(|| format!("Missing {name} collection"))?;
    for &(field, kind) in fields {
        let p = c
            .property_index(field)
            .with_context(|| format!("Missing {name}.{field}"))?;
        ensure!(
            c.mapping.properties[p].type_name == kind,
            "Unsupported {name}.{field} type"
        );
    }
    Ok(c)
}
fn schemas(db: &MirDb) -> Result<(&Collection, &Collection)> {
    Ok((
        collection(
            db,
            "MonsterInfo",
            &[
                ("Index", "System.Int32"),
                ("MonsterName", "System.String"),
                ("Level", "System.Int32"),
                ("AI", "System.Int32"),
                ("IsBoss", "System.Boolean"),
                ("Experience", "System.Decimal"),
            ],
        )?,
        collection(
            db,
            "MonsterInfoStat",
            &[
                ("Index", "System.Int32"),
                ("Monster", "System.Int32"),
                ("Stat", "System.Int32"),
                ("Amount", "System.Int32"),
            ],
        )?,
    ))
}
fn number(c: &Collection, row: &Record, field: &str) -> Result<i32> {
    let Some(Value::Int(n)) = c.get(row, field) else {
        anyhow::bail!("Invalid {}.{field}", c.short_name());
    };
    Ok(i32::try_from(*n)?)
}
fn label(stat: i32) -> String {
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
        _ => "Other",
    }
    .into()
}
fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}
/// Interpolate only within observed levels; no empirical extrapolation.
fn interpolate(curve: &BTreeMap<i32, Vec<f64>>, level: i32) -> Option<f64> {
    let (&lo, low) = curve.range(..=level).next_back()?;
    let (&hi, high) = curve.range(level..).next()?;
    let low = median(low.clone());
    if lo == hi {
        return Some(low);
    }
    Some(low + (median(high.clone()) - low) * f64::from(level - lo) / f64::from(hi - lo))
}
fn ratio(curve: &BTreeMap<i32, Vec<f64>>, source: i32, target: i32) -> (f64, bool) {
    match (interpolate(curve, source), interpolate(curve, target)) {
        (Some(a), Some(b)) if a > 0.0 && b > 0.0 => ((b / a).clamp(1.0 / 16.0, 16.0), true),
        _ => (
            (f64::from(target) / f64::from(source)).clamp(1.0 / 16.0, 16.0),
            false,
        ),
    }
}
fn decimal(value: f64) -> Result<Value> {
    // Draft XP is rounded to hundredths, bounded to a precisely representable range.
    ensure!(
        value.is_finite() && (0.0..=1_000_000_000_000.0).contains(&value),
        "Experience exceeds supported draft range"
    );
    let n = (value * 100.0).round() as u64;
    Ok(Value::Decimal(n as u32, (n >> 32) as u32, 0, 2 << 16))
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
    let (monsters, stats) = schemas(db)?;
    ensure!(
        !monsters.records.iter().any(|r| monsters
            .str_or(r, "MonsterName", "")
            .eq_ignore_ascii_case(name)),
        "A monster with this name already exists"
    );
    let source = monsters
        .records
        .iter()
        .find(|r| monsters.index(r) == source_id)
        .context("Source monster no longer exists")?;
    let source_level = number(monsters, source, "Level")?;
    ensure!(
        (1..=255).contains(&source_level),
        "Source monster needs a level between 1 and 255"
    );
    let ai = number(monsters, source, "AI")?;
    let boss = monsters.bool_or(source, "IsBoss", false);
    let mut amounts = BTreeMap::<i32, BTreeMap<i32, i32>>::new();
    for row in &stats.records {
        let monster = number(stats, row, "Monster")?;
        let stat = number(stats, row, "Stat")?;
        // Reference rows follow the server loader's last-row-wins semantics.
        // A draft source must itself be unambiguous.
        let previous = amounts
            .entry(monster)
            .or_default()
            .insert(stat, number(stats, row, "Amount")?);
        ensure!(
            monster != source_id || previous.is_none(),
            "Duplicate stat {stat} for source monster {monster}"
        );
    }
    let source_stats = amounts
        .get(&source_id)
        .context("Source monster has no stat rows")?;
    ensure!(
        source_stats.get(&2).copied().unwrap_or(0) > 0,
        "Source monster must have positive health"
    );
    let peers: Vec<_> = monsters
        .records
        .iter()
        .filter(|r| {
            monsters.int(r, "AI") == Some(i64::from(ai))
                && monsters.bool_or(r, "IsBoss", false) == boss
                && (1..=255).contains(&monsters.int_or(r, "Level", 0))
        })
        .collect();
    let curve_for = |stat: i32| {
        let mut curve = BTreeMap::<i32, Vec<f64>>::new();
        for r in &peers {
            let value = if stat == -1 {
                monsters.float_or(r, "Experience", 0.0)
            } else {
                f64::from(
                    amounts
                        .get(&monsters.index(r))
                        .and_then(|a| a.get(&stat))
                        .copied()
                        .unwrap_or(0),
                )
            };
            if value.is_finite() && value > 0.0 {
                curve
                    .entry(monsters.int_or(r, "Level", 0) as i32)
                    .or_default()
                    .push(value);
            }
        }
        curve
    };
    let mut empirical = 0;
    let mut fallback = 0;
    let mut changes = Vec::new();
    for (&stat, &before) in source_stats.range(2..=15) {
        // One factor for each min/max pair preserves its ordering.
        let basis = if (4..=13).contains(&stat) {
            stat | 1
        } else {
            stat
        };
        let (factor, observed) = ratio(&curve_for(basis), source_level, target_level);
        if observed {
            empirical += 1;
        } else {
            fallback += 1;
        }
        let after = if before > 0 {
            (f64::from(before) * factor)
                .round()
                .clamp(1.0, f64::from(i32::MAX)) as i32
        } else {
            before
        };
        changes.push(StatChange {
            stat,
            label: label(stat),
            before,
            after,
        });
    }
    let experience_before = monsters.float_or(source, "Experience", f64::NAN);
    let (factor, observed) = ratio(&curve_for(-1), source_level, target_level);
    if observed {
        empirical += 1;
    } else {
        fallback += 1;
    }
    let experience_value = if source_level == target_level {
        monsters.get(source, "Experience").unwrap().clone()
    } else {
        decimal(experience_before * factor)?
    };
    let experience_after = experience_value
        .as_f64()
        .context("Invalid source experience")?;
    ensure!(
        experience_after.is_finite() && experience_after >= 0.0,
        "Invalid source experience"
    );
    Ok(Draft {
        source_id,
        name: name.into(),
        source_level,
        target_level,
        changes,
        experience_before,
        experience_after,
        reference_count: peers
            .iter()
            .filter(|r| monsters.index(r) != source_id)
            .count(),
        method: format!(
            "Same AI and boss status: {empirical} fields use interpolated level medians; {fallback} use level ratio fallback. Factors limited to 1/16–16. Speeds and other stats preserved. Drops are not copied."
        ),
        snapshot: db.to_bytes()?,
        experience_value,
    })
}

impl Draft {
    pub fn apply(&self, db: &mut MirDb) -> Result<i32> {
        ensure!(
            db.to_bytes()? == self.snapshot,
            "Database changed since this preview; generate a new draft"
        );
        let mut next = MirDb::parse(&self.snapshot)?;
        let (monsters, stats) = schemas(&next)?;
        let mut record = monsters
            .records
            .iter()
            .find(|r| monsters.index(r) == self.source_id)
            .context("Source monster missing")?
            .clone();
        let mut rows: Vec<_> = stats
            .records
            .iter()
            .filter(|r| stats.int(r, "Monster") == Some(i64::from(self.source_id)))
            .cloned()
            .collect();
        // The allocator panics on exhausted IDs; validate capacity before using it.
        for (c, count) in [(monsters, 1usize), (stats, rows.len())] {
            let start = i64::from(c.next_index)
                .max(i64::from(c.records.iter().map(|r| c.index(r)).max().unwrap_or(0)) + 1);
            ensure!(
                start > 0 && start + count as i64 <= i64::from(i32::MAX),
                "{} identities exhausted",
                c.short_name()
            );
        }
        let monsters = next.collection_mut("MonsterInfo").unwrap();
        let id = monsters.allocate_index();
        for (field, value) in [
            ("Index", Value::Int(id.into())),
            ("MonsterName", Value::Str(self.name.clone())),
            ("Level", Value::Int(self.target_level.into())),
            ("Experience", self.experience_value.clone()),
        ] {
            ensure!(
                monsters.set(&mut record, field, value),
                "Invalid monster row"
            );
        }
        monsters.records.push(record);
        let stats = next.collection_mut("MonsterInfoStat").unwrap();
        for row in &mut rows {
            let stat = number(stats, row, "Stat")?;
            let index = stats.allocate_index();
            ensure!(
                stats.set(row, "Index", Value::Int(index.into()))
                    && stats.set(row, "Monster", Value::Int(id.into())),
                "Invalid stat row"
            );
            if let Some(change) = self.changes.iter().find(|s| s.stat == stat) {
                ensure!(
                    stats.set(row, "Amount", Value::Int(change.after.into())),
                    "Invalid stat amount"
                );
            }
            stats.records.push(row.clone());
        }
        // Validate encodability before replacing the caller's database.
        let encoded = next.to_bytes()?;
        *db = MirDb::parse(&encoded)?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    #[test]
    fn interpolated_ratios_use_brackets_and_preserve_pairs() {
        let curve = BTreeMap::from([(10, vec![100.0, 200.0, 900.0]), (20, vec![600.0])]);
        assert_eq!(interpolate(&curve, 15), Some(400.0));
        assert_eq!(ratio(&curve, 10, 15), (2.0, true));
        assert_eq!(ratio(&curve, 10, 30), (3.0, false));
        assert_eq!(ratio(&BTreeMap::new(), 1, 255), (16.0, false));
        assert!(decimal(f64::INFINITY).is_err());
        assert_eq!(decimal(12.345).unwrap().as_f64(), Some(12.35));
    }
    #[test]
    fn asset_draft_roundtrip_preserves_source_and_unrelated_records() {
        let Some(assets) = std::env::var_os("ZIRCON_ASSETS") else {
            return;
        };
        let mut db =
            MirDb::load(std::path::Path::new(&assets).join("../Database/System.db")).unwrap();
        let original = db.to_bytes().unwrap();
        let monsters = db.collection("MonsterInfo").unwrap();
        let source = monsters
            .records
            .iter()
            .find(|r| monsters.str_or(r, "MonsterName", "") == "Chicken")
            .unwrap();
        let source_id = monsters.index(source);
        let before = mir_server::data::GameData::from_db(&db).unwrap();
        let draft = propose(&db, source_id, 20, "Draft Test Chicken").unwrap();
        eprintln!(
            "Monster draft: {} -> {} level {}; {}",
            source_id, draft.name, draft.target_level, draft.method
        );
        assert_eq!(db.to_bytes().unwrap(), original);
        assert!(propose(&db, source_id, 0, "Invalid").is_err());
        assert!(propose(&db, source_id, 20, "\n").is_err());
        assert!(propose(&db, source_id, 20, "Chicken").is_err());
        let id = draft.apply(&mut db).unwrap();
        assert!(draft.apply(&mut db).is_err());
        let parsed = MirDb::parse(&db.to_bytes().unwrap()).unwrap();
        let loaded = mir_server::data::GameData::from_db(&parsed).unwrap();
        let copied = &loaded.monsters[&id];
        let source = &before.monsters[&source_id];
        assert_eq!(copied.name, "Draft Test Chicken");
        assert_eq!(copied.level, 20);
        assert_eq!(copied.image, source.image);
        assert_eq!(copied.ai, source.ai);
        assert_eq!(copied.move_delay, source.move_delay);
        assert_eq!(copied.attack_delay, source.attack_delay);
        for change in &draft.changes {
            assert_eq!(copied.stat(change.stat), change.after);
        }
        let original = MirDb::parse(&original).unwrap();
        for collection in &original.collections {
            let after = parsed.collection(collection.short_name()).unwrap();
            assert!(collection
                .records
                .iter()
                .zip(&after.records)
                .all(|(a, b)| a.values == b.values));
            if !["MonsterInfo", "MonsterInfoStat"].contains(&collection.short_name()) {
                assert_eq!(collection.records.len(), after.records.len());
                assert_eq!(collection.next_index, after.next_index);
            }
            let ids: BTreeSet<_> = after.records.iter().map(|r| after.index(r)).collect();
            assert_eq!(ids.len(), after.records.len());
        }
        let stats = parsed.collection("MonsterInfoStat").unwrap();
        let count = |monster| {
            stats
                .records
                .iter()
                .filter(|r| stats.int(r, "Monster") == Some(i64::from(monster)))
                .count()
        };
        assert_eq!(count(id), count(source_id));
        for row in stats
            .records
            .iter()
            .filter(|r| stats.int(r, "Monster") == Some(i64::from(source_id)))
        {
            let stat = number(stats, row, "Stat").unwrap();
            if !(2..=15).contains(&stat) {
                assert_eq!(copied.stat(stat), source.stat(stat));
            }
        }
    }
}
