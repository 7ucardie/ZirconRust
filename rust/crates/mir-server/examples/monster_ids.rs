//! Export monster IDs from the same System.db that the server loads.
//! Run: cargo run -p mir-server --example monster_ids -- /path/to/System.db > monster_ids.csv

use mir_server::data::GameData;

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("pass path to System.db");
    let data = GameData::load(path)?;
    let mut monsters: Vec<_> = data.monsters.values().collect();
    monsters.sort_by_key(|monster| monster.index);
    println!("id,name,level,boss");
    for monster in monsters {
        println!(
            "{},\"{}\",{},{}",
            monster.index,
            monster.name.replace('"', "\"\""),
            monster.level,
            monster.is_boss
        );
    }
    Ok(())
}
