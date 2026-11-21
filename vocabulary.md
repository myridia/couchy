# Couchy vocabulary

This file defines the GUI labels, CouchDB terms, CLI flags, and code vocabulary used in Couchy.

## 1. Product and runtime

| Term | Meaning |
|---|---|
| Couchy | The application: a Rust CLI plus desktop GUI for viewing, backing up, editing, and cleaning CouchDB data. |
| GUI | Graphical user interface. Started with `cargo run`, `./run.sh`, or a built executable without `--nox 1`. |
| CLI | Command-line interface. Started with `--nox 1`; runs headless and prints results to stdout. |
| `--nox 1` | Switches Couchy into CLI/headless mode. Default is GUI mode. |
| Desktop window | The native OS window created by `eframe`. Initial inner size is 1400 × 900. |
| Viewport | The window area managed by `egui`. `persist_window` lets `eframe` save and restore its size. |
| First run | Start with no saved window size; Couchy uses the default 1400 × 900 viewport. |
| Persistence | Saving UI state between runs. Couchy uses it for the window size. |
| `config.toml` | The main configuration file, normally `~/config.toml`. Stores CouchDB/MySQL settings and saved servers. |
| Saved server | A named CouchDB connection stored in `config.toml` and shown in the left **Connections** list. |
| Build | Compiling the Rust project, for example `cargo build --release`. |
| Release binary | The compiled executable, normally `target/release/couchy`. |

## 2. GUI layout

| Term | Meaning |
|---|---|
| Menu bar | The classic top menu: **File / Edit / View / Tools / Settings / Help**. |
| Top panel | A strip attached to the top of the window. Couchy uses one for the menu bar and one for the connection bar. |
| Bottom panel | A strip attached to the bottom of the window. Couchy uses one for the status bar and, when enabled, the log panel. |
| Connection bar | The two-row area below the menu bar for entering connection details. |
| Name | A friendly name for the current saved server connection. |
| Host | CouchDB base URL, for example `https://localhost:5984`. |
| User | CouchDB username used for HTTP Basic authentication. |
| Pass | Password field. Characters are hidden. |
| DB | Default database name. Used by some CLI actions and forms. |
| Connect | Uses the current fields to connect to CouchDB and fetch database names. |
| Add/Update Server | Saves or updates the current connection in the **Connections** list. |
| Status bar | Bottom strip showing connection state, connected host, and selected database. |
| Connected | Status-bar state meaning the last connection test succeeded. |
| Disconnected | Status-bar state meaning Couchy is not currently connected. |
| Left panel | The navigation pane. It contains **Mode** and **Connections**. |
| Right pane | The contextual command pane. Its label matches the selected mode and lists that mode's actions. |
| Central panel | The main content area between the side panels. Shows tables, editors, and forms for the selected mode. |
| Breadcrumbs | The row near the top of the central panel showing `server › database › document` context. In Docs list mode it also contains **Find**. |
| Content area | The central panel below the breadcrumbs. |
| Controls / content area | Placeholder text shown when no mode-specific content is active. |
| Group | An `egui` framed container used around the central content. |
| Separator | A horizontal or vertical divider line. |
| Scroll area | A scrollable region. Couchy gives each one an `id_source` to avoid egui ID-clash warnings. |
| Grid | A table-like `egui` widget used for database, document, user, and result tables. |
| Selectable label | A label/button that stays highlighted while selected. Used for modes, connections, database links, and active commands. |
| Active command | The right-pane action currently highlighted. It stays highlighted until the mode really changes. |
| Link | Clickable blue text used for navigation, for example the server breadcrumb or user-table **db** link. |
| Warning | Red confirmation label shown before destructive actions such as deleting all documents or purging. |
| Dialog / window | A popup `egui::Window`, for example **Delete all Docs**, **Purge deleted Docs**, **Access Test**, **Help**, or **About Couchy**. |

## 3. Top menu

| Menu | Item | Meaning |
|---|---|---|
| File | Connect | Runs `quick_connect`, using the connection bar fields. |
| File | Disconnect | Clears the active connection and returns to Server mode. |
| File | Save log to file... | Opens a file picker and saves the log text. Same helper as **Save log**. |
| File | Quit | Closes the application window. |
| Edit | Copy | Copies the log text to the system clipboard. |
| Edit | Paste | Appends clipboard text to the log. |
| Edit | Clear log | Empties the log text. |
| View | Server | Switches to Server mode. |
| View | Databases | Switches to Dbs mode and clears the database-info table. |
| View | DB info | Switches to Dbs mode and loads database details if they are not loaded. |
| View | Users | Switches to Users mode and loads users. |
| View | Settings | Switches to Settings mode. |
| View | Toggle Log panel | Shows or hides the bottom log panel. |
| Tools | Load DB info | Loads detailed information for all databases. |
| Tools | Check public access | Checks which databases are publicly readable. |
| Tools | Access test | Runs the access test for the selected database, or logs a warning if none is selected. |
| Tools | Save all Designs | Backs up all design documents. |
| Tools | Save all Docs | Backs up all documents. |
| Settings | Log API results | Toggle for writing full API responses into the log. |
| Help | Help | Opens the Help window. |
| Help | About Couchy | Opens the About window. |

## 4. Log panel

| Term | Meaning |
|---|---|
| Log | Text area at the bottom showing commands, curl-equivalent requests, status messages, and API results. |
| Log panel header | The row containing the log title and log buttons. |
| Copy | Copies the whole log to the clipboard. |
| Paste | Appends clipboard contents to the log. |
| Clear log | Removes all log text. |
| Save log | Saves the log to a file. |
| Log API results | When enabled, full API response bodies are also logged. Available in the Settings menu and Settings screens. |
| Pending log | Internal queue where background tasks place messages before the GUI drains them into `log_lines`. |
| Curl-equivalent line | A readable `curl ...` command logged for each HTTP request. It is for inspection, not necessarily executed. |

## 5. Navigation modes

The left **Mode** selector and the right-pane heading use the same small labels.

| Mode | Meaning |
|---|---|
| Server | Server-level information and global database commands. |
| Dbs | Database list, database details, public-access checks, and bulk backups. |
| Docs | Submode for the selected database. Lists, creates, edits, and repairs documents. |
| Users | CouchDB normal-user management from `_users`. |
| Settings | Application and CouchDB server configuration information. |

### Server mode

| GUI term | Meaning |
|---|---|
| Server | Central heading for server information. |
| `/`, `/_up`, `/_active_tasks`, `/_membership` | CouchDB server API areas shown as reference text. |
| `/_node/{node}/_stats`, `/_node/{node}/_versions` | Node-level API paths shown as reference text. |
| `/_replicate`, `/_scheduler/jobs` | Replication-related API paths shown as reference text. |
| List Dbs | Fetches only database names. |
| Dbs Info | Fetches detailed database information. |
| Open Dbs | Switches to Dbs mode. |
| Users | Switches to Users mode and loads users. |

### Dbs mode

| GUI term | Meaning |
|---|---|
| Database list | Fast list of database names fetched when connecting. |
| Load DB info | Fetches details for the listed databases on demand. |
| Databases Info | Table shown after details are loaded. |
| Database | Column/name of the database. |
| Docs | Number of live documents. |
| Deleted | Number of deleted documents. |
| Data | Active data size. |
| Disk | Disk size used by the database. |
| Public | Whether anonymous/public read access appears available. |
| Compacting | Whether compaction is currently running. |
| List Dbs | Refreshes the database-name list. |
| Dbs Info | Loads or refreshes the detailed database table. |
| Check public access | Checks public read access for databases. |
| Save all Designs | Backs up design documents. |
| Save all Docs | Backs up documents. |
| Purge deleted Docs | Opens the purge confirmation dialog for all databases. |

### Docs mode

| GUI term | Meaning |
|---|---|
| Docs | Right-pane label and left mode label for document mode. |
| Breadcrumb database link | Returns from a document editor/new-document form to the document list. |
| Find | Local filter field for the loaded document IDs. It filters by document ID; it is not a CouchDB query. |
| Clear | Clears the **Find** filter. |
| Document ID | Primary key of a CouchDB document. |
| Rev | Current revision identifier of a document. |
| Reload | Re-fetches the currently open document, or the document list if no document editor is open. In new-document mode it exits the form and reloads the list. |
| Create Doc | Opens the new-document form. |
| Document ID: | Input for the new document's explicit ID. Leaving it empty allows an auto-generated ID. |
| Save | Saves the edited/new document. |
| List all Docs | Runs the generic `_all_docs` database command. |
| Delete all Docs | Opens a destructive confirmation dialog to delete all non-design documents in the selected database. |
| Purge deleted Docs | Purges deleted-document history in the selected database. |
| Access Test | Opens the access-test window for the selected database. |
| Back to Dbs | Returns to Dbs mode when no database is selected. |
| `null` | JSON editor display for an empty/null value. |
| New key: | Input for a new JSON property name in the JSON editor. |
| Add | Adds the typed JSON key to the editor. |
| `x` | Removes a JSON field/value row in the editor. |

### Users mode

| GUI term | Meaning |
|---|---|
| List users | Loads users from `_users` and shows them in the users table. |
| Add new User | Opens the new-user form. |
| Check read-only users | Checks users' per-user databases for public read-only state. |
| Loading users... | Status shown while the user list is loading. |
| No users found. | Status shown when `_users` returns no normal users. |
| User | Username column in the users table. |
| Roles | CouchDB roles assigned to the user. |
| Per-user DB | Whether Couchy found the user's own database. |
| Public R/O | Whether the user's per-user database is public read-only. |
| Db | Link column. Opens the computed per-user database in Docs mode if it exists; otherwise switches to Dbs mode and logs that it was not found. |
| Actions | Edit and delete controls for a user row. |
| Edit? | Confirmation text before deleting a user. |
| Yes / No | Confirm or cancel the user deletion. |
| Edit | Opens the selected user in the user form. |
| Delete | Starts/ confirms user deletion depending on confirmation state. |
| Username | User-name field in the add/edit form. |
| Password | Password field in the add/edit form. |
| Roles | Roles field in the add/edit form. |
| Read Only Public | Option meaning the user's own database is readable publicly but editable only by that user or admins. |
| Cancel | Closes the user form without saving. |

### Settings mode

| GUI term | Meaning |
|---|---|
| Application | Settings that affect Couchy itself. |
| Window size is saved automatically. | Explains eframe window persistence. |
| Log API results | Application toggle for verbose API-response logging. |
| CouchDB server | Reference section for server configuration. |
| Server configuration is admin-only. | Warning that node config endpoints require admin rights. |
| `GET /_node/{node}/_config` | Endpoint for reading node configuration. |
| `PUT /_node/{node}/_config/{section}/{key}` | Endpoint for writing a configuration key. |
| `_users` | CouchDB system database containing normal users. |
| Server admins | Administrators stored in server configuration, not in `_users`. |

## 6. CouchDB concepts

| Term | Meaning |
|---|---|
| CouchDB server | The remote database server addressed by **Host**. |
| Database | A CouchDB database; contains documents. Short form: DB. |
| Dbs | Short for databases. Used as a mode label. |
| Docs | Short for documents. Used as a mode label. |
| Document | A JSON record stored in a CouchDB database. |
| `_id` | Document ID field. |
| `_rev` | Document revision field. CouchDB writes create new revisions. |
| Design document | A document whose ID starts with `_design/`. Holds views and design metadata. |
| View | A CouchDB index/query definition, usually stored in a design document. |
| `_all_docs` | CouchDB endpoint that lists documents in a database. |
| `_all_dbs` | CouchDB endpoint that lists database names. |
| `_dbs_info` | CouchDB endpoint/request used to get detailed database statistics. |
| `_users` | CouchDB system database for normal user documents. |
| Per-user DB | A private database for one user. Couchy computes its name as `userdb-{username-as-hex}`. |
| User document | A JSON document in `_users`, normally ID `org.couchdb.user:<name>`. |
| Role | A named permission group attached to a CouchDB user. |
| Admin | A CouchDB administrator. Server admins are stored in config, normal users in `_users`. |
| Basic authentication | HTTP authentication using the **User** and **Pass** fields. |
| Public access | Whether a database can be read without authentication. |
| Public read-only | Public users may read, but only the owner or admins may edit. |
| Deleted document | A document marked deleted; its revision history may remain until purged/compacted. |
| Purge | Removes revision history of deleted documents. More destructive than a normal delete. |
| Compaction | CouchDB process that reclaims disk space by rewriting database files. |
| Data size | Size of active data. |
| Disk size | Size on disk, including overhead/deleted history. |
| Replication | Copying database changes between CouchDB servers. |
| Master | Source server in the CLI orphan-deletion workflow. |
| Replica | Target server compared against the master in the CLI orphan-deletion workflow. |
| Orphan document | A document in the replica that does not exist in the master. |
| Node | A CouchDB server node, used in `/_node/{node}/...` config and stats endpoints. |
| Active tasks | Currently running CouchDB operations. |
| Membership | CouchDB cluster membership information. |
| Scheduler jobs | Replication/scheduler jobs reported by CouchDB. |
| URL encoding | Couchy encodes `/` in database/document paths as `%2F`. |
| Host normalization | Adding `https://` when the host has no scheme and trimming trailing `/`. |

## 7. Right-pane commands by mode

Right-pane buttons are contextual: only commands for the current mode are shown. They use a selectable style so the last clicked command stays highlighted.

| Mode | Buttons |
|---|---|
| Server | List Dbs, Dbs Info, Open Dbs, Users |
| Dbs | List Dbs, Dbs Info, Check public access, Save all Designs, Save all Docs, Purge deleted Docs |
| Docs | Reload, Create Doc, Delete all Docs, Purge deleted Docs, List all Docs, Access Test |
| Users | List users, Add new User, Check read-only users |
| Settings | Log API results checkbox and informational text |

## 8. Buttons and dialogs

| Term | Meaning |
|---|---|
| Cancel | Closes the current dialog/form without applying changes. |
| Close | Closes the access-test results window. |
| Confirmation | A dialog asking the user to approve a destructive action. |
| Delete | Deletes the selected saved server or user, or starts a delete confirmation depending on context. |
| Yes / No | Approve or reject a confirmation. |
| Re-run | Repeats the access test. |
| Test | Access-test column naming the check that was run. |
| Result | Access-test column showing `OK` or `FAILED`. |
| Detail | Access-test column with extra information. |
| Access Test | Window/command that runs access checks and displays results. |
| Delete all Docs | Destructive dialog for deleting all non-design documents in a database. |
| Purge deleted Docs | Destructive dialog for purging deleted-document history. |

## 9. CLI terminology

| Term | Meaning |
|---|---|
| `--nox 1` | Headless/CLI mode. |
| `--save` | Save operation selector. Values include `all_design` and `all_server_design`. |
| `--delete` | Delete operation selector. Values include `orphans` and `by_key`. |
| `--migrate` | Migration operation selector. Values include `table` and `query`. |
| `--db` / `-r` | Target CouchDB database. |
| `--key` / `-y` | Field name for `--delete by_key`. |
| `--value` / `-k` | Field value for `--delete by_key`. |
| `--master` / `-v` | Master CouchDB URL for orphan deletion. |
| `--repl` / `-b` | Replica CouchDB URL for orphan deletion. |
| `--query` / `-q` | SQL query text for migration. |
| `--table` / `-t` | MySQL table name for migration. |
| `--action` / `-a` | Generic action name. |
| `--roles` | Comma-separated CouchDB roles. |
| `--readonly` | Read-only/public-read-only flag. |
| `--yes` | Non-interactive confirmation flag. |
| `all_design` | Saves design docs from one database. |
| `all_server_design` | Saves design docs from all non-system databases. |
| `orphans` | Deletes replica documents missing from the master. |
| `by_key` | Deletes documents whose field matches a key/value pair. |
| `table` | Migrates a whole MySQL table into CouchDB documents. |
| `query` | Migrates the result of a custom SQL query into CouchDB documents. |

## 10. Internal code terms

| Term | Meaning |
|---|---|
| `MyEguiApp` | Main GUI state struct in `src/main.rs`. |
| `AppMode` | Enum for `Server`, `Dbs`, `Docs`, `Users`, and `Settings`. |
| `mode` | Current `AppMode`. Controls central content and right-pane commands. |
| `set_mode` | Changes mode and clears state that belongs to the old mode. |
| `active_command` | Key of the highlighted right-pane command. Cleared only on a real mode change. |
| `action` | Generic action string used by the older/CLI action dispatcher. |
| `command_button` | Helper that renders a right-pane command as a selectable label and sets `active_command`. |
| `log_lines` | The visible log text. |
| `pending_log` | Thread-safe queue used by async tasks to send messages back to the GUI. |
| `Arc<Mutex<Vec<String>>>` | Shared, mutex-protected message queue type used for background task results. |
| `tokio::spawn` | Starts an async background task, for example an HTTP request. |
| `ctx.request_repaint()` | Asks egui to redraw after async data arrives. |
| `connected` | Boolean state for the current connection. |
| `connected_host` | Host shown in the status bar after connecting. |
| `db_list` | Fast list of database names. |
| `db_infos` | Detailed database rows shown in **Databases Info**. |
| `selected_db` | Database currently opened in Docs mode. |
| `doc_list` | Loaded document-ID/revision rows. |
| `doc_filter` | Text in the **Find** field. |
| `doc_json` | JSON of the currently open document. |
| `doc_loading` | True while a document is being fetched. |
| `new_doc_mode` | True while the new-document form is open. |
| `new_doc_json` | JSON being built in the new-document form. |
| `users` | Loaded user rows. |
| `users_loading` | True while users are being fetched. |
| `show_add_user` | True while the add/edit user form is open. |
| `editing_user` | The user being edited, if any. |
| `confirm_delete` | User ID awaiting delete confirmation. |
| `delete_all_open` / `delete_all_running` | State for the **Delete all Docs** dialog. |
| `purge_all_open` / `purge_all_running` | State for the **Purge deleted Docs** dialog. |
| `access_test_open` / `access_test_running` | State for the **Access Test** window. |
| `current_connection_config` | Builds the active `AppConfig` from the GUI fields or selected server. |
| `curl_command` | Builds the curl-like request string shown in the log. |
| `normalize_host` | Adds a scheme to hosts and trims trailing slashes. |
| `db_path` | URL-encodes `/` in database names. |
| `ServerConfig` | Saved connection: name, host, user, password, database. |
| `AppConfig` | Full config: current CouchDB/MySQL settings plus saved servers. |
| `DbInfoRow` | Detailed database row: name, doc counts, sizes, public flag, compacting flag. |
| `DocRow` | Document row: ID and revision. |
| `UserRow` | User row: ID, name, revision, roles, raw doc, per-user DB flag, public read-only flag. |
| `AccessTestResult` | One access-test result: name, ok/failed, detail. |
| `ScrollArea::id_source` | Unique ID given to each scroll region to prevent egui “first/second use of ScrollArea ID” warnings. |
| `eframe` | Native window/application framework. |
| `egui` | Immediate-mode GUI library used for all widgets. |
| `wgpu` | GPU rendering backend enabled for `eframe`. |
| `reqwest` | HTTP client used for direct CouchDB API calls. |
| `couch_rs` | CouchDB client library used by some commands. |
| `clap` | CLI argument parser. |
| `serde` / `serde_json` | JSON/config serialization and parsing. |
| `toml` | Config file format parser/writer. |
| `arboard` | Clipboard access for **Copy** and **Paste**. |
| `rfd` | Native file picker used for saving logs. |
| `sqlx` | MySQL access used by migration commands. |

## 11. Source files

| File | Meaning |
|---|---|
| `src/main.rs` | Entry point, CLI/GUI dispatch, GUI layout, mode state, dialogs, and most GUI actions. |
| `src/config.rs` | `Args`, `AppConfig`, `ServerConfig`, and `~/config.toml` loading/saving. |
| `src/commands.rs` | CouchDB command implementations and data row structs. |
| `src/view.rs` | CouchDB view operations. |
| `src/mysql.rs` | MySQL migration support. |
| `src/lib.rs` | Library module root. |
| `Cargo.toml` | Rust package and dependency manifest. |
| `Cargo.lock` | Locked dependency versions. |
| `README.md` | Short project overview. |
| `HOWITWORKS.md` | Usage guide for CLI commands and older GUI notes. |
| `AGENTS.md` | Repository conventions and current architecture summary. |
