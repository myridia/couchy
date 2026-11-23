# AGENTS.md — couchy

## What this is
Couchy — Convenient Utility to get and update CouchDB data. Rust CLI + GUI (eframe/egui).

## Stack
- Rust (edition 2024)
- eframe/egui (GUI)
- couch_rs (CouchDB client)
- clap (CLI args)
- tokio (async runtime)
- arboard (clipboard read for Edit → Paste)

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

## Modes
- AppMode: Server / Dbs / Docs / Users / Settings.
- Docs is a submode reached by selecting a database in Dbs.
- Left panel has a mode selector above the Connections list.
- Central panel and command pane render from AppMode.

## Top menu
- File / Edit / View / Tools / Settings / Help. View switches modes and toggles the log panel.

## Log panel
- Header buttons: Copy, Paste, Clear log, Save log.
- Edit and File menu actions use the same log helpers.

## Window
- Default inner size is 1400 × 900.
- Window icon uses `pages/img/icon_192x192.ico` decoded to RGBA.
- Help and About dialogs show a 128×128 icon from `pages/img/icon_512x512.png`.
- Resized window size is persisted and restored by eframe.
- Connection bar uses two rows: Name/Host, then User/Pass/DB/actions.
- Left panel is labeled Connections.

## Database list flow
- Connect fetches database names only and shows them right away.
- Command pane has a Dbs section with "List Dbs" and "Dbs Info".
- "Load DB info" (database list header) and "Dbs Info" fetch details on demand.
- "Check public access" runs the per-database public check separately.

## Conventions
- No comments in code unless asked.
- Verify: `cargo check && cargo build`
