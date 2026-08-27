# AGENTS.md — couchy

## What this is
Couchy — Convenient Utility to get and update CouchDB data. Rust CLI + GUI (eframe/egui).

## Stack
- Rust (edition 2024)
- eframe/egui (GUI)
- couch_rs (CouchDB client)
- clap (CLI args)
- tokio (async runtime)

## Build
```bash
cargo build --release
```

## Run
- GUI: `./run.sh` or `cargo run`
- CLI: `cargo run -- --nox 1` (see `Args` in `src/config.rs`)

## Structure
- `src/main.rs` — entry point, GUI + CLI dispatch
- `src/config.rs` — config loading, CLI args
- `src/lib.rs` — library root
- `src/view.rs` — CouchDB view operations

## Conventions
- No comments in code unless asked.
- Verify: `cargo check && cargo build`
