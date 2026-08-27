# Couchy — How to Use

Couchy is a Rust CLI + GUI tool for managing CouchDB data. It backs up design documents and cleans up orphaned or unwanted records.

## Install

```bash
git clone git@code.salamander-jewelry.net:Salamander/couchy.git
cd couchy
cargo build --release
```

Binary: `target/release/couchy`

## Configuration

Couchy reads `~/config.toml` on first run:

```toml
host = "http://localhost:5984"
user = "admin"
password = "password"
database = "mydb"
```

Edit this file to set your CouchDB connection. The GUI also lets you override these values per session.

## Modes

| Mode | Flag | Description |
|------|------|-------------|
| GUI | `--nox 0` (default) | Opens egui window |
| CLI | `--nox 1` | Runs headless, prints to stdout |

## CLI Commands

### Save design docs (single database)

Backs up all `_design/*` documents from one database to `~/Documents/<db>--<design>.json`:

```bash
couchy --nox 1 --save all_design
```

Uses the `database` from `config.toml`.

### Save design docs (all databases)

Backs up design docs from every database on the server (skips system DBs starting with `_`):

```bash
couchy --nox 1 --save all_server_design
```

### Delete orphan documents

Compares a **replica** database against a **master** and deletes documents in the replica that don't exist in the master:

```bash
couchy --nox 1 --delete orphans \
  --master http://couchdb.salamander-jewelry.net \
  --repl http://cb1.salamander-jewelry.com:5984 \
  --db sl_usa_style
```

| Flag | Meaning |
|------|---------|
| `--master` / `-v` | Master CouchDB URL |
| `--repl` / `-b` | Replica CouchDB URL |
| `--db` / `-r` | Database name |

### Delete documents by key/value

Finds all documents matching a field selector and deletes them in parallel batches:

```bash
couchy --nox 1 --db logger --delete by_key --key logger --value API3
```

| Flag | Meaning |
|------|---------|
| `--db` / `-r` | Target database |
| `--key` / `-m` | Field name to match |
| `--value` / `-k` | Field value to match |

## GUI

```bash
./run_gui.sh
# or
cargo run
```

The GUI provides:
- Host / Database / User / Password fields
- **Views** menu → Save all_design / Save all_server_design
- Perform button runs the selected operation
- Log panel shows output

## Development

Hot-reload during development:

```bash
# CLI mode with watch
./run.sh

# GUI mode with watch
./run_gui.sh
```

Uses `cargo watch` to recompile on source changes.

## Output

Design doc backups go to:
```
~/Documents/<database_name>--<design_doc_id>.json
```

The `_rev` field is stripped so the JSON can be re-imported cleanly.

## Examples

```bash
# Backup all designs from "mydb"
couchy --nox 1 --save all_design

# Backup all designs from all databases
couchy --nox 1 --save all_server_design

# Clean orphaned docs from replica
couchy --nox 1 --delete orphans \
  --master http://master:5984 \
  --repl http://replica:5984 \
  --db mydb

# Delete all logs from app "SAPIF"
couchy --nox 1 --db logger --delete by_key --key logger --value SAPIF
```
