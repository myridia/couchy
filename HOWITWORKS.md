# Couchy — How to Use

Couchy is a Rust CLI + GUI tool for managing CouchDB data. It backs up design documents, cleans up orphaned records, and migrates MySQL data to CouchDB.

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
# CouchDB connection
host = "http://localhost:5984"
user = "admin"
password = "password"
database = "mydb"

# MySQL connection (for migrate commands)
mysql_host = "127.0.0.1"
mysql_user = "root"
mysql_password = "password"
mysql_database = "mydb"
```

Edit this file to set your CouchDB and MySQL connections.

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

### Migrate MySQL table to CouchDB

Copies all rows from a MySQL table into CouchDB documents:

```bash
couchy --nox 1 --migrate table --table products
```

| Flag | Meaning |
|------|---------|
| `--table` / `-t` | MySQL table name (required) |
| `--query` / `-q` | Custom SQL query (optional, defaults to `SELECT * FROM <table>`) |

The CouchDB database name defaults to the table name. Override with `--db`:

```bash
couchy --nox 1 --migrate table --table products --db my_couch_db
```

### Migrate MySQL query to CouchDB

Runs a custom SQL query and inserts results into a specific CouchDB database:

```bash
couchy --nox 1 --migrate query \
  --query "SELECT p.id, p.name, d.description FROM products p JOIN descriptions d ON p.id = d.product_id" \
  --db product_docs
```

| Flag | Meaning |
|------|---------|
| `--query` / `-q` | SQL query to run (required) |
| `--db` / `-r` | Target CouchDB database (required) |

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

# Migrate MySQL "products" table to CouchDB
couchy --nox 1 --migrate table --table products

# Migrate with custom query
couchy --nox 1 --migrate table --table products \
  --query "SELECT id, name, price FROM products WHERE status = 'active'"

# Migrate complex join to specific CouchDB database
couchy --nox 1 --migrate query \
  --query "SELECT p.*, d.name FROM product p JOIN product_description d ON p.product_id = d.product_id" \
  --db product_docs
```
