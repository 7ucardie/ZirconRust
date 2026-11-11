# ZirconRust agent guidelines

Use the globally installed Ponytail skill at full intensity for coding and Caveman for concise communication, unless the user requests otherwise.

The Rust workspace is in `rust/`. Game assets, `System.db`, account data, and build output are local only and must never be committed. Use the original Zircon C# repository as reference when needed.

Before pushing, run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace`, and `cargo test --workspace` from `rust/`. Check the Rust and Dungeon Atlas Pages GitHub Actions runs after pushing.
