//! Inventory of installed equipment appearance mappings and source sprites.
//! Run: cargo run -q -p mir-formats --example equipment_audit -- System.db Client > equipment_appearance_audit.csv

use mir_formats::mirdb::MirDb;
use mir_formats::monster_table::library_path;
use mir_formats::wearable_animation::{
    armour_library, helmet_library, horse_armour_library, shield_library, weapon_library,
    weapon_right_library,
};
use mir_formats::zl::ZlLibrary;
use std::collections::HashMap;
use std::path::Path;

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

// PlayerObject's ArmourList, HelmetList, WeaponList, ShieldList and HorseList keys.
// Kept separate from the Rust runtime mapping so the report flags drift.
fn csharp_library(kind: i64, shape: u16, female: bool, assassin: bool) -> Option<u16> {
    match kind {
        2 => {
            let male = match shape / 10 {
                0..=6 => 83 + shape / 10,
                10..=16 => 90 + shape / 10 - 10,
                110..=112 => 117 + shape / 10 - 110,
                113 => 119,
                114..=116 => 120 + shape / 10 - 114,
                120 => 111,
                122 => 112,
                126 => 113,
                _ => return None,
            };
            if assassin && shape == 1263 {
                return Some(if female { 128 } else { 116 });
            }
            Some(if female {
                male + if (83..=96).contains(&male) { 14 } else { 12 }
            } else {
                male
            })
        }
        3 => match shape / 11 {
            0..=3 => Some(
                if assassin {
                    if female {
                        59
                    } else {
                        53
                    }
                } else if female {
                    42
                } else {
                    31
                } + shape / 11,
            ),
            4 if !assassin => Some(if female { 46 } else { 35 }),
            10..=13 if !assassin => Some(if female { 47 } else { 36 } + shape / 11 - 10),
            20 => Some(match (assassin, female) {
                (false, false) => 40,
                (false, true) => 51,
                (true, false) => 57,
                (true, true) => 63,
            }),
            _ => None,
        },
        5 if shape > 0 => match (shape - 1) / 10 {
            key @ 0..=3 => Some(
                if assassin {
                    if female {
                        164
                    } else {
                        159
                    }
                } else if female {
                    149
                } else {
                    139
                } + key,
            ),
            4 if !assassin => Some(if female { 153 } else { 143 }),
            key @ 10..=13 if !assassin => Some(if female { 154 } else { 144 } + key - 10),
            20 => Some(match (assassin, female) {
                (false, false) => 148,
                (false, true) => 158,
                (true, false) => 163,
                (true, true) => 168,
            }),
            _ => None,
        },
        27 => match shape / 10 {
            0 => Some(if female { 137 } else { 135 }),
            1 => Some(if female { 138 } else { 136 }),
            _ => None,
        },
        18 => match shape {
            1 => Some(73),
            2 => Some(74),
            3 => Some(75),
            4 => Some(76),
            5 => Some(77),
            6 => Some(79),
            7 => Some(81),
            _ => None,
        },
        _ => None,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: equipment_audit <System.db> <Client root>".into());
    }
    let db = MirDb::load(&args[1])?;
    let items = db.collection("ItemInfo").ok_or("ItemInfo missing")?;
    let mut libraries: HashMap<u16, Option<ZlLibrary>> = HashMap::new();
    println!("id,name,type,shape,exterior_effect,required_class,required_gender,gender,class,csharp_library,rust_library,frame,status");
    for item in &items.records {
        let kind = items.int_or(item, "ItemType", -1);
        if ![2, 3, 5, 9, 18, 27].contains(&kind) {
            continue;
        }
        let shape = items.int_or(item, "Shape", 0);
        let name = items.str_or(item, "ItemName", "");
        let effect = items.int_or(item, "ExteriorEffect", 0);
        let required_class = items.int_or(item, "RequiredClass", 0);
        let required_gender = items.int_or(item, "RequiredGender", 0);
        for (female, gender) in [(false, "male"), (true, "female")] {
            for (assassin, class) in [(false, "other"), (true, "assassin")] {
                let mapping = if !(0..=u16::MAX as i64).contains(&shape) {
                    None
                } else {
                    let shape = shape as u16;
                    match kind {
                        2 if assassin && shape == 1263 => weapon_right_library(shape, female),
                        2 => weapon_library(shape, female),
                        3 => armour_library(shape, female, assassin),
                        5 => helmet_library(shape, female, assassin),
                        18 => horse_armour_library(shape),
                        27 => shield_library(shape, female),
                        _ => None, // C# has no world boots layer.
                    }
                };
                let frame = if mapping.is_some() {
                    match kind {
                        2 => ((shape - if shape >= 1000 { 1000 } else { 0 }) % 10) * 5000,
                        3 => (shape % 11) * if assassin { 3000 } else { 5000 },
                        5 => ((shape - 1) % 10) * if assassin { 3000 } else { 5000 },
                        18 => 2240,
                        27 => (shape % 10) * if assassin { 3000 } else { 5000 },
                        _ => 0,
                    }
                } else {
                    0
                };
                let source = (0..=u16::MAX as i64)
                    .contains(&shape)
                    .then(|| csharp_library(kind, shape as u16, female, assassin))
                    .flatten();
                let eligible = required_gender & if female { 2 } else { 1 } != 0
                    && required_class & if assassin { 8 } else { 7 } != 0;
                let status = if !eligible {
                    "class_or_gender_ineligible"
                } else if source != mapping {
                    "mapping_mismatch"
                } else if kind == 9 {
                    "no_world_layer"
                } else if mapping.is_none() {
                    "no_source_mapping"
                } else {
                    let lib = mapping.unwrap();
                    let source = libraries.entry(lib).or_insert_with(|| {
                        let path = library_path(lib)?;
                        ZlLibrary::open(Path::new(&args[2]).join(path)).ok()
                    });
                    match source {
                        Some(source)
                            if source.info(frame as usize).is_some_and(|info| {
                                info.width > 0 && info.height > 0 && info.position > 0
                            }) =>
                        {
                            "source_frame_present"
                        }
                        Some(_) => "source_frame_missing",
                        None => "source_library_missing",
                    }
                };
                println!(
                    "{},{},{},{},{},{},{},{},{},{},{},{},{}",
                    items.index(item),
                    quoted(name),
                    kind,
                    shape,
                    effect,
                    required_class,
                    required_gender,
                    gender,
                    class,
                    source.map_or(String::new(), |n| n.to_string()),
                    mapping.map_or(String::new(), |n| n.to_string()),
                    if mapping.is_some() {
                        frame.to_string()
                    } else {
                        String::new()
                    },
                    status
                );
                if kind == 2 && assassin && shape >= 1200 && shape != 1263 {
                    let right = weapon_right_library(shape as u16, female);
                    let status = if !eligible {
                        "class_or_gender_ineligible"
                    } else {
                        match right {
                            Some(lib) => {
                                let source = libraries.entry(lib).or_insert_with(|| {
                                    let path = library_path(lib)?;
                                    ZlLibrary::open(Path::new(&args[2]).join(path)).ok()
                                });
                                match source {
                                    Some(source)
                                        if source.info(frame as usize).is_some_and(|info| {
                                            info.width > 0 && info.height > 0 && info.position > 0
                                        }) =>
                                    {
                                        "source_frame_present"
                                    }
                                    Some(_) => "source_frame_missing",
                                    None => "source_library_missing",
                                }
                            }
                            None => "no_source_mapping",
                        }
                    };
                    println!(
                        "{},{},{},{},{},{},{},{},assassin_right,{},{},{},{}",
                        items.index(item),
                        quoted(name),
                        kind,
                        shape,
                        effect,
                        required_class,
                        required_gender,
                        gender,
                        right.map_or(String::new(), |n| n.to_string()),
                        right.map_or(String::new(), |n| n.to_string()),
                        frame,
                        status
                    );
                }
            }
        }
    }
    Ok(())
}
