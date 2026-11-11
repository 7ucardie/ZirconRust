//! Export item IDs from the same System.db that the server loads.
//! Run: cargo run -p mir-server --example item_ids -- /path/to/System.db > item_ids.csv

use mir_server::data::GameData;

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("pass path to System.db");
    let data = GameData::load(path)?;
    let mut items: Vec<_> = data.items.values().collect();
    items.sort_by_key(|item| item.index);
    println!("id,name,type");
    for item in items {
        println!(
            "{},\"{}\",{}",
            item.index,
            item.name.replace('"', "\"\""),
            item.item_type
        );
    }
    Ok(())
}
