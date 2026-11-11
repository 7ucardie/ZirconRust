//! Dump a summary of a MirDB System.db: `cargo run -p mir-formats --example dbdump -- <System.db> [Collection]`
use mir_formats::mirdb::{MirDb, Value};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .expect("usage: dbdump <System.db> [Collection] [limit]");
    let db = MirDb::load(path).expect("load");
    match args.get(2) {
        None => {
            for c in &db.collections {
                println!(
                    "{:40} {:6} records, {:2} props",
                    c.short_name(),
                    c.records.len(),
                    c.mapping.properties.len()
                );
            }
        }
        Some(name) => {
            let c = db.collection(name).expect("no such collection");
            let limit: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);
            println!("{} ({} records)", c.mapping.type_name, c.records.len());
            for p in &c.mapping.properties {
                println!("  {:28} {}", p.name, p.type_name);
            }
            for r in c.records.iter().take(limit) {
                let fields: Vec<String> = c
                    .mapping
                    .properties
                    .iter()
                    .zip(&r.values)
                    .map(|(p, v)| {
                        let s = match v {
                            Value::Str(s) => format!("{s:?}"),
                            Value::BitArray(Some(b)) => format!(
                                "bits[{} bytes, {} set]",
                                b.len(),
                                b.iter().map(|x| x.count_ones()).sum::<u32>()
                            ),
                            Value::PointArray(Some(p)) => format!("points[{}]", p.len()),
                            Value::Decimal(..) => format!("{}", v.as_f64().unwrap()),
                            other => format!("{other:?}"),
                        };
                        format!("{}={}", p.name, s)
                    })
                    .collect();
                println!("- {}", fields.join(" "));
            }
        }
    }
}
