//! Print ItemInfo exterior-effect use from System.db.
//! cargo run -p mir-server --example effect_inventory -- System.db

use mir_formats::mirdb::MirDb;

fn main() -> anyhow::Result<()> {
    let db = MirDb::load(std::env::args().nth(1).expect("System.db path required"))?;
    let items = db.collection("ItemInfo").expect("ItemInfo missing");
    println!("item_id,item_name,item_type,image,exterior_effect");
    for row in &items.records {
        let effect = items.int_or(row, "ExteriorEffect", 0);
        if effect == 0 {
            continue;
        }
        let name = items.str_or(row, "ItemName", "").replace('"', "\"\"");
        println!(
            "{},\"{}\",{},{},{}",
            items.index(row),
            name,
            items.int_or(row, "ItemType", 0),
            items.int_or(row, "Image", 0),
            effect
        );
    }
    Ok(())
}
