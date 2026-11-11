//! List every System.db collection and row count for parity audits.

use mir_formats::mirdb::MirDb;

fn main() -> anyhow::Result<()> {
    let db = MirDb::load(std::env::args().nth(1).expect("System.db path required"))?;
    println!("collection,rows");
    for collection in &db.collections {
        println!("{},{}", collection.short_name(), collection.records.len());
    }
    Ok(())
}
