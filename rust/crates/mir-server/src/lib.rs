//! Library half of the Rust Zircon server: game data, accounts, world
//! simulation and the network layer. The binary in `main.rs` wires them up;
//! `mir-editor` uses the loader and validator directly.

pub mod accounts;
pub mod data;
pub mod items;
pub mod magic;
pub mod net;
pub mod world;

#[cfg(test)]
mod tests;
