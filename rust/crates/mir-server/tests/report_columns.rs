//! Developer report: which fame / script / currency columns this asset pack
//! carries (`cargo test -p mir-server --test report_columns -- --ignored --nocapture`).

use std::path::PathBuf;

#[test]
#[ignore]
fn report_fame_script_columns() {
    let Some(assets) = std::env::var_os("ZIRCON_ASSETS").map(PathBuf::from) else {
        return;
    };
    let db = mir_formats::mirdb::MirDb::load(assets.join("../Database/System.db")).unwrap();
    for name in [
        "FameInfo",
        "FameInfoStat",
        "FameInfoReward",
        "NPCPage",
        "CurrencyInfo",
    ] {
        match db.collection(name) {
            Some(c) => {
                let props: Vec<String> = c
                    .mapping
                    .properties
                    .iter()
                    .map(|p| p.name.clone())
                    .collect();
                eprintln!("{name}: {} records, props {:?}", c.records.len(), props);
                for r in c.records.iter().take(6) {
                    let vals: Vec<String> = c
                        .mapping
                        .properties
                        .iter()
                        .map(|p| format!("{}={:?}", p.name, c.get(r, &p.name)))
                        .collect();
                    eprintln!("  {}", vals.join(" "));
                }
            }
            None => eprintln!("{name}: absent"),
        }
    }
}
