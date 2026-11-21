use clap::Parser;
use couchy::commands as cmd;
use couchy::commands::{AccessTestResult, DbInfoRow, UserRow};
use couchy::config::{get_config, save_config, AppConfig, Args, ServerConfig};
use couchy::mysql::*;
use couchy::view::*;
use eframe::egui;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if args.nox != 1 {
        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0]),
            persist_window: true,
            ..Default::default()
        };
        let _ = eframe::run_native(
            "Couchy",
            native_options,
            Box::new(|cc| Ok(Box::new(MyEguiApp::new(cc)))),
        );
    } else {
        println!("...run console");
        if !args.action.is_empty() {
            match cli_action(&args).await {
                Ok(out) => println!("{out}"),
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }
        let config = get_config();
        if args.save == "all_design" {
            let base_dir = homedir::my_home()
                .ok()
                .flatten()
                .map(|h| h.join("Documents"))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let _r = save_all_design(&config, &base_dir, None, None).await;
        } else if args.save == "all_server_design" {
            let base_dir = homedir::my_home()
                .ok()
                .flatten()
                .map(|h| h.join("Documents"))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let _r = save_all_server_design(&config, &base_dir, &config.host, None, None).await;
        } else if args.delete == "orphans" {
            let _r = delete_orphans(&config, args).await;
        } else if args.delete == "by_key" {
            if !args.db.is_empty() && !args.key.is_empty() && !args.value.is_empty() {
                let _r = delete_by_key(&config, args).await;
            }
        } else if args.migrate == "table" {
            if !args.table.is_empty() {
                let query = if args.query.is_empty() {
                    format!("SELECT * FROM {}", args.table)
                } else {
                    args.query.clone()
                };
                let _r = migrate_table(&config, &args.table, &query).await;
            } else {
                eprintln!("Error: --table is required for migrate table");
            }
        } else if args.migrate == "query" {
            if args.query.is_empty() {
                eprintln!("Error: --query is required for migrate query");
            } else if args.db.is_empty() {
                eprintln!("Error: --db is required for migrate query (target CouchDB database)");
            } else {
                let _r = migrate_query(&config, &args.query, &args.db).await;
            }
        }
    }
}

fn req_db(args: &Args) -> Result<String, String> {
    let db = args.db.trim().to_string();
    if db.is_empty() {
        Err("--db is required for this action".to_string())
    } else {
        Ok(db)
    }
}

fn value_flag(arg: &str) -> Result<serde_json::Value, String> {
    let s = if let Some(path) = arg.strip_prefix('@') {
        std::fs::read_to_string(path).map_err(|e| format!("Cannot read {path}: {e}"))?
    } else {
        arg.to_string()
    };
    serde_json::from_str(&s).map_err(|e| format!("Not valid JSON: {e}"))
}

fn format_bytes_opt(size: Option<u64>) -> String {
    match size {
        None => "-".to_string(),
        Some(v) => {
            const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
            let mut value = v as f64;
            let mut unit = 0;
            while value >= 1024.0 && unit < UNITS.len() - 1 {
                value /= 1024.0;
                unit += 1;
            }
            if unit == 0 {
                format!("{v} {}", UNITS[unit])
            } else {
                format!("{value:.1} {}", UNITS[unit])
            }
        }
    }
}

async fn cli_action(args: &Args) -> Result<String, String> {
    let config = get_config();
    match args.action.as_str() {
        "connect" => cmd::connect(&config).await,
        "get_dbs_info" | "db_info" => {
            let v = cmd::get_dbs_info(&config).await?;
            let rows = cmd::parse_db_infos(&v);
            let mut out = String::new();
            out.push_str("Database  Docs  Deleted  Data  Disk  Compacting  Public\n");
            for r in &rows {
                let public = cmd::check_public_db(&config, &r.name).await.unwrap_or(false);
                out.push_str(&format!(
                    "{:30} {:>6} {:>7} {:>8} {:>8} {:<10} {}\n",
                    r.name,
                    r.doc_count.map(|v| v.to_string()).unwrap_or("-".to_string()),
                    r.doc_del_count.map(|v| v.to_string()).unwrap_or("-".to_string()),
                    format_bytes_opt(r.data_size),
                    format_bytes_opt(r.disk_size),
                    r.compact_running.map(|v| if v { "yes" } else { "no" }).unwrap_or("-"),
                    if public { "yes" } else { "no" }
                ));
            }
            out.push_str(&format!("{} databases", rows.len()));
            Ok(out)
        }
        "list_docs" => {
            let db = req_db(args)?;
            let docs = cmd::list_docs(&config, &db).await?;
            let mut out = String::new();
            for (id, rev) in &docs {
                out.push_str(&format!("{id}  {rev}\n"));
            }
            out.push_str(&format!("{} documents", docs.len()));
            Ok(out)
        }
        "get_doc" => {
            let db = req_db(args)?;
            let id = args.key.trim().to_string();
            if id.is_empty() {
                return Err("--key (document id) is required".to_string());
            }
            let doc = cmd::get_doc(&config, &db, &id).await?;
            Ok(serde_json::to_string_pretty(&doc).unwrap_or_else(|_| doc.to_string()))
        }
        "create_doc" => {
            let db = req_db(args)?;
            let mut doc = value_flag(&args.value)?;
            let id = args.key.trim().to_string();
            if !id.is_empty() && doc.get("_id").is_none() {
                doc["_id"] = serde_json::Value::String(id.clone());
            }
            let id_arg = if id.is_empty() {
                doc.get("_id").and_then(|i| i.as_str()).unwrap_or("").to_string()
            } else {
                id
            };
            let created = cmd::create_doc(&config, &db, &id_arg, &doc).await?;
            let new_id = created
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or(id_arg.as_str())
                .to_string();
            Ok(format!(
                "Created document {new_id} in {db} [{}]",
                created.get("rev").and_then(|r| r.as_str()).unwrap_or("?")
            ))
        }
        "save_doc" => {
            let db = req_db(args)?;
            let doc = value_flag(&args.value)?;
            let id = args.key.trim().to_string();
            let saved = if id.is_empty() {
                cmd::save_doc(&config, &db, &doc).await?
            } else {
                let mut d = doc.clone();
                d["_id"] = serde_json::Value::String(id);
                cmd::save_doc(&config, &db, &d).await?
            };
            Ok(format!(
                "Saved document {} [{}]",
                saved.get("id").and_then(|i| i.as_str()).unwrap_or("?"),
                saved.get("rev").and_then(|r| r.as_str()).unwrap_or("?")
            ))
        }
        "delete_all_docs" => {
            let db = req_db(args)?;
            if !args.yes {
                return Err("delete_all_docs is destructive; pass --yes to confirm".to_string());
            }
            cmd::delete_all_docs(&config, &db, None, None).await
        }
        "purge_deleted_docs" => {
            if !args.yes {
                return Err("purge_deleted_docs is destructive; pass --yes to confirm".to_string());
            }
            let db = args.db.trim().to_string();
            if db.is_empty() {
                cmd::purge_server_deleted_docs(&config, None, None).await
            } else {
                cmd::purge_db_deleted_docs(&config, &db, None, None).await
            }
        }
        "list_users" => {
            let users = cmd::list_users(&config).await?;
            let mut out = String::new();
            out.push_str("Name  Roles  Per-user DB  Public R/O\n");
            for u in &users {
                let peruser = cmd::user_peruser_db_exists(&config, &u.name).await.unwrap_or(false);
                let readonly = if peruser {
                    let hex: String = u.name.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
                    let db = format!("userdb-{hex}");
                    cmd::check_readonly_db(&config, &db).await.unwrap_or(false)
                } else {
                    false
                };
                out.push_str(&format!(
                    "{:30} {:20} {:^10} {:^8}\n",
                    u.name,
                    if u.roles.is_empty() { "-" } else { u.roles.as_str() },
                    if peruser { "yes" } else { "no" },
                    if readonly { "yes" } else { "no" }
                ));
            }
            out.push_str(&format!("{} users", users.len()));
            Ok(out)
        }
        "create_user" => {
            let name = args.key.trim().to_string();
            if name.is_empty() {
                return Err("--key (username) is required".to_string());
            }
            let password = args.value.clone();
            let roles: Vec<String> = args
                .roles
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            let existing = cmd::find_user(&config, &name).await?.map(|u| u.doc);
            let msg = cmd::save_user(&config, &name, &password, roles, existing).await?;
            if args.readonly {
                let extra = cmd::setup_readonly_db(
                    config.host.clone(),
                    config.user.clone(),
                    config.password.clone(),
                    name,
                )
                .await;
                return Ok(format!("{msg}\n{}", extra.join("\n")));
            }
            Ok(msg)
        }
        "delete_user" => {
            if !args.yes {
                return Err("delete_user is destructive; pass --yes to confirm".to_string());
            }
            let name = args.key.trim().to_string();
            if name.is_empty() {
                return Err("--key (username) is required".to_string());
            }
            let existing = cmd::find_user(&config, &name)
                .await?
                .ok_or_else(|| format!("User {name} not found"))?;
            cmd::delete_user(&config, &existing.id, &existing.rev).await
        }
        "check_public" => {
            let db = req_db(args)?;
            let public = cmd::check_public_db(&config, &db).await?;
            Ok(format!(
                "{}: {}",
                db,
                if public { "public read access: yes" } else { "public read access: no" }
            ))
        }
        "check_readonly" => {
            let db = req_db(args)?;
            let readonly = cmd::check_readonly_db(&config, &db).await?;
            Ok(format!(
                "{}: {}",
                db,
                if readonly { "read-only (design auth present)" } else { "not read-only" }
            ))
        }
        "access_test" => {
            let db = req_db(args)?;
            let results = cmd::access_test(&config, &db, None, None).await?;
            let mut out = String::new();
            out.push_str("Test  Result  Detail\n");
            for r in &results {
                out.push_str(&format!(
                    "{:30} {:<8} {}\n",
                    r.name,
                    if r.ok { "OK" } else { "FAILED" },
                    r.detail
                ));
            }
            let ok = results.iter().filter(|r| r.ok).count();
            out.push_str(&format!("{}/{} tests OK", ok, results.len()));
            Ok(out)
        }
        "save_all_docs" => {
            let base_dir = homedir::my_home()
                .ok()
                .flatten()
                .map(|h| h.join("Documents"))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let server_name: String = config
                .host
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("server")
                .to_string();
            couchy::view::save_all_server_dbs(&base_dir, &server_name, &config, None, None)
                .await
                .map(|_| format!("Save all Docs completed: {}", base_dir.display()))
                .map_err(|e| format!("Save all Docs failed: {e}"))
        }
        "save_all_design" => {
            let base_dir = homedir::my_home()
                .ok()
                .flatten()
                .map(|h| h.join("Documents"))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            couchy::view::save_all_design(&config, &base_dir, None, None)
                .await
                .map(|_| format!("Save all design docs completed: {}", base_dir.display()))
                .map_err(|e| format!("Save all design docs failed: {e}"))
        }
        "save_all_server_design" => {
            let base_dir = homedir::my_home()
                .ok()
                .flatten()
                .map(|h| h.join("Documents"))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            couchy::view::save_all_server_design(&config, &base_dir, &config.host, None, None)
                .await
                .map(|_| format!("Save all server design docs completed: {}", base_dir.display()))
                .map_err(|e| format!("Save all server design docs failed: {e}"))
        }
        other => Err(format!(
            "Unknown action: {other}. Available: connect, get_dbs_info, list_docs, get_doc, create_doc, save_doc, delete_all_docs, purge_deleted_docs, list_users, create_user, delete_user, check_public, check_readonly, access_test, save_all_docs, save_all_design, save_all_server_design"
        )),
    }
}

type AccessTestRow = AccessTestResult;

struct DocRow {
    id: String,
    rev: String,
}

fn json_editor(ui: &mut egui::Ui, value: &mut serde_json::Value, new_key_input: &mut String) {
    let key_color = ui.visuals().hyperlink_color;
    let faint = ui.visuals().faint_bg_color;
    match value {
        serde_json::Value::Null => {
            ui.label(egui::RichText::new("null").monospace().italics().weak());
        }
        serde_json::Value::Bool(b) => {
            ui.add(egui::Checkbox::new(b, egui::RichText::new(if *b {
                "true"
            } else {
                "false"
            }).monospace()));
        }
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                let mut v = i;
                if ui
                    .add(egui::DragValue::new(&mut v).speed(1.0).max_decimals(0))
                    .changed()
                {
                    *n = serde_json::Number::from(v);
                }
            } else {
                let mut f = n.as_f64().unwrap_or(0.0);
                if ui
                    .add(egui::DragValue::new(&mut f).speed(0.1).max_decimals(6))
                    .changed()
                {
                    *n = serde_json::Number::from_f64(f).unwrap_or_else(|| serde_json::Number::from(0));
                }
            }
        }
        serde_json::Value::String(s) => {
            ui.add(
                egui::TextEdit::singleline(s)
                    .desired_width(420.0)
                    .font(egui::TextStyle::Monospace)
                    .margin(egui::Margin::symmetric(10.0, 8.0)),
            );
        }
        serde_json::Value::Array(arr) => {
            egui::Frame::none()
                .fill(faint)
                .rounding(8.0)
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(format!("Array ({} items)", arr.len())).strong().monospace());
                        ui.add_space(6.0);
                        let mut remove_idx: Option<usize> = None;
                        for (i, item) in arr.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(format!("[{i}]")).monospace().weak());
                                json_editor(ui, item, new_key_input);
                                if ui.button("x").clicked() {
                                    remove_idx = Some(i);
                                }
                            });
                            ui.add_space(3.0);
                        }
                        if let Some(i) = remove_idx {
                            arr.remove(i);
                        }
                        ui.add_space(6.0);
                        if ui.button("+ Add item").clicked() {
                            arr.push(serde_json::Value::String(String::new()));
                        }
                    });
                });
        }
        serde_json::Value::Object(map) => {
            egui::Frame::none()
                .fill(faint)
                .rounding(8.0)
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(format!("Object ({} keys)", map.len())).strong().monospace());
                        ui.add_space(6.0);
                        let keys: Vec<String> = map.keys().cloned().collect();
                        let mut remove_key: Option<String> = None;
                        for key in keys {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(&key).strong().monospace().color(key_color));
                                if let Some(v) = map.get_mut(&key) {
                                    json_editor(ui, v, new_key_input);
                                }
                                if ui.button("x").clicked() {
                                    remove_key = Some(key);
                                }
                            });
                            ui.add_space(3.0);
                        }
                        if let Some(k) = remove_key {
                            map.remove(&k);
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("New key:").monospace());
                            ui.add(
                                egui::TextEdit::singleline(new_key_input)
                                    .hint_text("key name")
                                    .desired_width(180.0)
                                    .font(egui::TextStyle::Monospace)
                                    .margin(egui::Margin::symmetric(8.0, 6.0)),
                            );
                            if ui.button("Add").clicked() && !new_key_input.trim().is_empty() {
                                let k = new_key_input.trim().to_string();
                                if !map.contains_key(&k) {
                                    map.insert(k, serde_json::Value::String(String::new()));
                                }
                                new_key_input.clear();
                            }
                        });
                    });
                });
        }
    }
}

struct MyEguiApp {
    host: String,
    database: String,
    user: String,
    password: String,
    server_name: String,
    log_lines: String,
    pending_log: Arc<Mutex<Vec<String>>>,
    window_help_open: bool,
    window_about_open: bool,
    show_log_panel: bool,
    action: String,
    servers: Vec<ServerConfig>,
    selected_server: Option<usize>,
    base_config: AppConfig,
    connected: bool,
    connected_host: String,
    db_list: Vec<String>,
    db_infos: Vec<DbInfoRow>,
    selected_db: Option<String>,
    doc_list: Vec<DocRow>,
    selected_doc: Option<String>,
    log_results: bool,
    doc_filter: String,
    doc_json: Option<serde_json::Value>,
    doc_loading: bool,
    new_key_input: String,
    new_doc_mode: bool,
    new_doc_id_input: String,
    new_doc_json: Option<serde_json::Value>,
    users: Vec<UserRow>,
    users_view: bool,
    users_loading: bool,
    show_add_user: bool,
    editing_user: Option<UserRow>,
    user_name_input: String,
    user_password_input: String,
    user_roles_input: String,
    confirm_delete: Option<String>,
    read_only_public: bool,
    delete_all_open: bool,
    delete_all_running: bool,
    purge_all_open: bool,
    purge_all_running: bool,
    access_test_open: bool,
    access_test_running: bool,
    access_test_db: String,
    access_test_results: Vec<AccessTestRow>,
}

impl Default for MyEguiApp {
    fn default() -> Self {
        let mut cfg = get_config();
        cfg.ensure_servers();
        let servers = cfg.servers.clone();
        let mut app = Self {
            host: cfg.host.clone(),
            database: cfg.database.clone(),
            user: cfg.user.clone(),
            password: cfg.password.clone(),
            server_name: String::new(),
            log_lines: String::new(),
            pending_log: Arc::new(Mutex::new(Vec::new())),
            window_help_open: false,
            window_about_open: false,
            show_log_panel: true,
            action: String::new(),
            servers,
            selected_server: None,
            base_config: cfg,
            connected: false,
            connected_host: String::new(),
            db_list: Vec::new(),
            db_infos: Vec::new(),
            selected_db: None,
            doc_list: Vec::new(),
            selected_doc: None,
            log_results: false,
            doc_filter: String::new(),
            doc_json: None,
            doc_loading: false,
            new_key_input: String::new(),
            new_doc_mode: false,
            new_doc_id_input: String::new(),
            new_doc_json: None,
            users: Vec::new(),
            users_view: false,
            users_loading: false,
            show_add_user: false,
            editing_user: None,
            user_name_input: String::new(),
            user_password_input: String::new(),
            user_roles_input: String::new(),
            confirm_delete: None,
            read_only_public: false,
            delete_all_open: false,
            delete_all_running: false,
            purge_all_open: false,
            purge_all_running: false,
            access_test_open: false,
            access_test_running: false,
            access_test_db: String::new(),
            access_test_results: Vec::new(),
        };
        if !app.servers.is_empty() {
            app.select_server(0);
        }
        app
    }
}

impl MyEguiApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::proportional(14.0),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::proportional(17.0),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::proportional(17.0),
        );
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::proportional(24.0),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::monospace(15.0),
        );
        cc.egui_ctx.set_style(style);
        Self::default()
    }

    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    fn curl_command(host: &str, user: &str, password: &str, method: &str, path: &str, body: Option<&str>) -> String {
        let url = format!("{}{}", host.trim_end_matches('/'), path);
        let mut cmd = format!("curl -X {method}");
        if !user.is_empty() || !password.is_empty() {
            cmd.push_str(&format!(" -u {}", Self::shell_quote(&format!("{user}:{password}"))));
        }
        if let Some(b) = body {
            cmd.push_str(&format!(
                " -H {} -d {}",
                Self::shell_quote("Content-Type: application/json"),
                Self::shell_quote(b)
            ));
        }
        cmd.push_str(&format!(" {}", Self::shell_quote(&url)));
        cmd
    }

    fn bare_host(s: &str) -> String {
        s.trim()
            .strip_prefix("https://")
            .or_else(|| s.trim().strip_prefix("http://"))
            .unwrap_or(s.trim())
            .trim_end_matches('/')
            .to_string()
    }

    fn current_connection_config(&self) -> AppConfig {
        ServerConfig {
            name: self.server_name.clone(),
            host: cmd::normalize_host(&self.host),
            user: self.user.clone(),
            password: self.password.clone(),
            database: self.database.clone(),
        }
        .to_app_config(&self.base_config)
    }

    fn persist_servers(&mut self) {
        self.base_config.servers = self.servers.clone();
        self.base_config.host = self.host.clone();
        self.base_config.user = self.user.clone();
        self.base_config.password = self.password.clone();
        self.base_config.database = self.database.clone();
        if let Err(e) = save_config(&self.base_config) {
            self.log_lines.push_str(&format!("\nFailed to save config: {e:?}"));
        }
    }

    fn select_server(&mut self, idx: usize) {
        if let Some(s) = self.servers.get(idx).cloned() {
            self.selected_server = Some(idx);
            self.server_name = s.name;
            self.host = s.host;
            self.user = s.user;
            self.password = s.password;
            self.database = s.database;
            self.selected_db = None;
            self.doc_filter.clear();
        }
    }

    fn save_server_entry(
        &mut self,
        name: String,
        host: String,
        user: String,
        password: String,
        database: String,
    ) {
        let server = ServerConfig {
            name: if name.trim().is_empty() {
                if host.trim().is_empty() {
                    format!("Server {}", self.servers.len() + 1)
                } else {
                    Self::bare_host(&host)
                }
            } else {
                name
            },
            host,
            user,
            password,
            database,
        };
        if let Some(idx) = self.selected_server {
            if idx < self.servers.len() {
                self.servers[idx] = server;
            } else {
                self.servers.push(server);
                self.selected_server = Some(self.servers.len() - 1);
            }
        } else {
            self.servers.push(server);
            self.selected_server = Some(self.servers.len() - 1);
        }
        self.persist_servers();
        self.log_lines.push_str("\nServer list saved.");
    }

    fn add_or_update_server(&mut self, ctx: &egui::Context) {
        let host = self.host.trim().to_string();
        let name = self.server_name.trim().to_string();
        let user = self.user.clone();
        let password = self.password.clone();
        let database = self.database.clone();
        if host.is_empty() || host.starts_with("http://") || host.starts_with("https://") {
            self.save_server_entry(name, host, user, password, database);
            return;
        }
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        let https_curl = Self::curl_command(&format!("https://{host}"), "", "", "GET", "/", None);
        let http_curl = Self::curl_command(&format!("http://{host}"), "", "", "GET", "/", None);
        self.log_lines.push_str(&format!("\nTesting {host} ..."));
        tokio::spawn(async move {
            let http = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            let send = |msg: String| {
                println!("{msg}");
                if let Ok(mut p) = pending.lock() {
                    p.push(msg);
                }
                ctx2.request_repaint();
            };
            send(https_curl);
            let https_ok = match http.get(format!("https://{host}/")).send().await {
                Ok(resp) => resp.status().as_u16() < 500,
                Err(_) => false,
            };
            let mut scheme = if https_ok { Some("https") } else { None };
            if scheme.is_none() {
                send(http_curl);
                let http_ok = match http.get(format!("http://{host}/")).send().await {
                    Ok(resp) => resp.status().as_u16() < 500,
                    Err(_) => false,
                };
                if http_ok {
                    scheme = Some("http");
                }
            }
            match scheme {
                Some(s) => {
                    let payload = serde_json::json!({
                        "scheme": s,
                        "host": host,
                        "name": name,
                        "user": user,
                        "password": password,
                        "database": database,
                    });
                    send(format!("PROBE_OK|{payload}"));
                }
                None => {
                    send(format!("PROBE_ERR|Cannot reach {host} with https or http - server entry not saved."));
                }
            }
        });
    }

    fn delete_selected_server(&mut self) {
        if let Some(idx) = self.selected_server {
            if idx < self.servers.len() {
                self.servers.remove(idx);
                if self.servers.is_empty() {
                    self.selected_server = None;
                    self.server_name.clear();
                    self.host.clear();
                    self.user.clear();
                    self.password.clear();
                    self.database.clear();
                } else {
                    let new_idx = idx.min(self.servers.len() - 1);
                    self.select_server(new_idx);
                }
                self.persist_servers();
                self.log_lines.push_str("\nServer removed.");
            }
        }
    }

    fn db_display_label(db: &str) -> String {
        if let Some(hex) = db.strip_prefix("userdb-") {
            if !hex.is_empty() && hex.len() % 2 == 0 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                let mut bytes = Vec::with_capacity(hex.len() / 2);
                let mut ok = true;
                let chars: Vec<char> = hex.chars().collect();
                for i in (0..chars.len()).step_by(2) {
                    let pair: String = chars[i..i + 2].iter().collect();
                    if let Ok(b) = u8::from_str_radix(&pair, 16) {
                        bytes.push(b);
                    } else {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    if let Ok(s) = String::from_utf8(bytes) {
                        if !s.is_empty() {
                            return format!("{s} ({db})");
                        }
                    }
                }
            }
        }
        db.to_string()
    }

    fn format_bytes(size: Option<u64>) -> String {
        match size {
            None => "-".to_string(),
            Some(v) => {
                const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
                let mut value = v as f64;
                let mut unit = 0;
                while value >= 1024.0 && unit < UNITS.len() - 1 {
                    value /= 1024.0;
                    unit += 1;
                }
                if unit == 0 {
                    format!("{v} {}", UNITS[unit])
                } else {
                    format!("{value:.1} {}", UNITS[unit])
                }
            }
        }
    }

    fn run_action(&mut self, ctx: &egui::Context) {
        let action = self.action.clone();
        if action.is_empty() {
            self.log_lines.push_str("\nNo action selected.");
            return;
        }
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx = ctx.clone();
        let log_results = self.log_results;
        let api_call = match action.as_str() {
            "get_all_dbs" => Some(Self::curl_command(&config.host, &config.user, &config.password, "GET", "/_all_dbs", None)),
            _ => None,
        };
        if let Some(cmd) = api_call {
            self.log_lines.push_str(&format!("\n{cmd}"));
        } else {
            self.log_lines
                .push_str(&format!("\nRunning action: {action} ..."));
        }
        tokio::spawn(async move {
            let msg = match action.as_str() {
                "get_all_dbs" => match cmd::list_db_names(&config).await {
                    Ok(dbs) => {
                        let json = serde_json::to_string(&dbs).unwrap_or_else(|_| "[]".to_string());
                        let log = if log_results {
                            format!("GET /_all_dbs: {}", serde_json::to_string_pretty(&dbs).unwrap_or_else(|_| format!("{dbs:?}")))
                        } else {
                            format!("GET /_all_dbs: OK ({} databases)", dbs.len())
                        };
                        format!("DB_LIST|{json}|{log}")
                    }
                    Err(e) => format!("GET /_all_dbs failed: {e}"),
                },
                "get_dbs_info" => match cmd::get_dbs_info(&config).await {
                    Ok(v) => {
                        let curl = Self::curl_command(&config.host, &config.user, &config.password, "POST", "/_dbs_info", None);
                        let log = if log_results {
                            format!("{curl}\nPOST /_dbs_info: {}", serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()))
                        } else {
                            format!("{curl}\nPOST /_dbs_info: OK")
                        };
                        let envelope = serde_json::json!({ "data": v, "log": log });
                        format!("DB_INFO|{}", serde_json::to_string(&envelope).unwrap_or(log))
                    }
                    Err(e) => format!("POST /_dbs_info failed: {e}"),
                },
                _ => format!("Unknown action: {action}"),
            };
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx.request_repaint();
        });
    }

    fn load_docs(&mut self, ctx: &egui::Context, db: &str) {
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        let db = db.to_string();
        let curl = Self::curl_command(&config.host, &config.user, &config.password, "GET", &format!("/{}/_all_docs", db.replace('/', "%2F")), None);
        self.log_lines.push_str(&format!("\n{curl}"));
        tokio::spawn(async move {
            let msg = match cmd::list_docs(&config, &db).await {
                Ok(docs) => {
                    let count = docs.len();
                    let rows: Vec<serde_json::Value> = docs
                        .into_iter()
                        .map(|(id, rev)| serde_json::json!({ "id": id, "rev": rev }))
                        .collect();
                    let envelope = serde_json::json!({
                        "docs": rows,
                        "log": format!("Loaded {count} document IDs from {db}"),
                    });
                    format!(
                        "DOC_LIST|{}",
                        serde_json::to_string(&envelope).unwrap_or_default()
                    )
                }
                Err(e) => format!("GET /{db}/_all_docs failed: {e}"),
            };
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx2.request_repaint();
        });
    }

    fn load_doc(&mut self, ctx: &egui::Context, db: &str, doc_id: &str) {
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        let db = db.to_string();
        let doc_id = doc_id.to_string();
        let path = format!("/{}/{}", db.replace('/', "%2F"), doc_id.replace('/', "%2F"));
        let curl = Self::curl_command(&config.host, &config.user, &config.password, "GET", &path, None);
        self.log_lines.push_str(&format!("\n{curl}"));
        tokio::spawn(async move {
            let msg = match cmd::get_doc(&config, &db, &doc_id).await {
                Ok(v) => format!("DOC|{}", serde_json::to_string(&v).unwrap_or_default()),
                Err(e) => format!("DOC_ERR|{e}"),
            };
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx2.request_repaint();
        });
    }

    fn save_doc(&mut self, ctx: &egui::Context, db: &str, doc_id: &str) {
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        let db = db.to_string();
        let doc_id = doc_id.to_string();
        let doc = match &self.doc_json {
            Some(d) => d.clone(),
            None => return,
        };
        let body = serde_json::to_string_pretty(&doc).unwrap_or_else(|_| doc.to_string());
        let log_results = self.log_results;
        let path = format!("/{}/{}", db.replace('/', "%2F"), doc_id.replace('/', "%2F"));
        let curl = Self::curl_command(&config.host, &config.user, &config.password, "PUT", &path, Some(&body));
        if log_results {
            self.log_lines.push_str(&format!("\n{curl}"));
        } else {
            self.log_lines.push_str(&format!("\n{}", Self::curl_command(&config.host, &config.user, &config.password, "PUT", &path, None)));
        }
        tokio::spawn(async move {
            let msg = match cmd::save_doc(&config, &db, &doc).await {
                Ok(v) => {
                    let body = serde_json::to_string(&v).unwrap_or_default();
                    if log_results {
                        format!("DOC_SAVED|{body}|PUT {path} [201]")
                    } else {
                        format!("DOC_SAVED|{body}|PUT {path}: OK")
                    }
                }
                Err(e) => format!("DOC_SAVE_ERR|{e}"),
            };
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx2.request_repaint();
        });
    }

    fn load_users(&mut self, ctx: &egui::Context) {
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        let path = "/_users/_all_docs?include_docs=true";
        let curl = Self::curl_command(&config.host, &config.user, &config.password, "GET", path, None);
        self.users_loading = true;
        self.log_lines.push_str(&format!("\n{curl}"));
        tokio::spawn(async move {
            let msg = match cmd::list_users(&config).await {
                Ok(users) => {
                    let count = users.len();
                    let rows: Vec<serde_json::Value> = users
                        .iter()
                        .map(|u| serde_json::to_value(u).unwrap_or(serde_json::json!({})))
                        .collect();
                    let envelope = serde_json::json!({
                        "users": rows,
                        "log": format!("Loaded {count} users."),
                    });
                    format!("USERS|{}", serde_json::to_string(&envelope).unwrap_or_default())
                }
                Err(e) => format!("USERS_ERR|{e}"),
            };
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx2.request_repaint();
        });
    }

    fn check_public_dbs(&mut self, ctx: &egui::Context) {
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        let names: Vec<String> = self.db_infos.iter().map(|r| r.name.clone()).collect();
        self.log_lines.push_str(&format!(
            "\nChecking public access for {} database(s)...",
            names.len()
        ));
        tokio::spawn(async move {
            let mut statuses = Vec::new();
            for name in &names {
                let public = cmd::check_public_db(&config, name).await.unwrap_or(false);
                statuses.push(serde_json::json!({
                    "name": name,
                    "public": public,
                }));
            }
            let envelope = serde_json::json!({
                "statuses": statuses,
                "log": format!("Checked public access for {} database(s).", names.len()),
            });
            let msg = format!(
                "DB_PUBLIC|{}",
                serde_json::to_string(&envelope).unwrap_or_default()
            );
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx2.request_repaint();
        });
    }

    fn check_readonly_users(&mut self, ctx: &egui::Context) {
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        let users = self.users.clone();
        self.log_lines
            .push_str(&format!("\nChecking {} per-user database(s)...", users.len()));
        tokio::spawn(async move {
            let mut statuses = Vec::new();
            for u in &users {
                let hex: String = u.name.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
                let db = format!("userdb-{hex}");
                let peruser = cmd::user_peruser_db_exists(&config, &u.name).await.unwrap_or(false);
                let mut public_ro = false;
                if peruser {
                    public_ro = cmd::check_readonly_db(&config, &db).await.unwrap_or(false);
                }
                statuses.push(serde_json::json!({
                    "name": u.name,
                    "peruser": peruser,
                    "public_ro": public_ro,
                }));
            }
            let envelope = serde_json::json!({
                "statuses": statuses,
                "log": format!("Checked {} per-user database(s).", users.len()),
            });
            let msg = format!(
                "USERS_STATUS|{}",
                serde_json::to_string(&envelope).unwrap_or_default()
            );
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx2.request_repaint();
        });
    }

    fn save_user(&mut self, ctx: &egui::Context) {
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        let name = self.user_name_input.trim().to_string();
        if name.is_empty() {
            self.log_lines.push_str("\nUsername is empty.");
            return;
        }
        let password = self.user_password_input.clone();
        let roles: Vec<String> = self
            .user_roles_input
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        let editing = self.editing_user.clone();
        let editing_doc = editing.clone().map(|e| e.doc);
        let make_readonly = self.read_only_public;
        let readonly_host = config.host.clone();
        let readonly_user = config.user.clone();
        let readonly_password = config.password.clone();
        let doc = if let Some(e) = editing.as_ref() {
            let mut d = e.doc.clone();
            d["name"] = serde_json::Value::String(name.clone());
            d["type"] = serde_json::Value::String("user".to_string());
            d["roles"] = serde_json::Value::Array(
                roles.iter().map(|r| serde_json::Value::String(r.clone())).collect(),
            );
            if !password.is_empty() {
                d["password"] = serde_json::Value::String(password.clone());
            }
            d
        } else {
            serde_json::json!({
                "name": name,
                "password": password,
                "roles": roles,
                "type": "user",
            })
        };
        let path = format!("/_users/org.couchdb.user:{name}");
        let body = serde_json::to_string(&doc).unwrap_or_else(|_| doc.to_string());
        let curl = Self::curl_command(&config.host, &config.user, &config.password, "PUT", &path, Some(&body));
        self.log_lines.push_str(&format!("\n{curl}"));
        tokio::spawn(async move {
            let msg = match cmd::save_user(&config, &name, &password, roles.clone(), editing_doc.clone()).await {
                Ok(saved) => {
                    let mut log = saved;
                    if make_readonly {
                        let extra = cmd::setup_readonly_db(
                            readonly_host.clone(),
                            readonly_user.clone(),
                            readonly_password.clone(),
                            name.clone(),
                        )
                        .await;
                        if !extra.is_empty() {
                            log.push('\n');
                            log.push_str(&extra.join("\n"));
                        }
                    }
                    format!("USER_SAVED|{log}|{log}")
                }
                Err(e) => format!("USER_SAVE_ERR|{e}"),
            };
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx2.request_repaint();
        });
    }

    fn delete_user(&mut self, ctx: &egui::Context, id: &str, rev: &str) {
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        let id = id.to_string();
        let rev = rev.to_string();
        let path = format!("/_users/{id}?rev={rev}");
        let curl = Self::curl_command(&config.host, &config.user, &config.password, "DELETE", &path, None);
        self.log_lines.push_str(&format!("\n{curl}"));
        tokio::spawn(async move {
            let msg = match cmd::delete_user(&config, &id, &rev).await {
                Ok(log) => format!("USER_DELETED|{log}|{log}"),
                Err(e) => format!("USER_DELETE_ERR|{e}"),
            };
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx2.request_repaint();
        });
    }

    fn start_access_test(&mut self, ctx: &egui::Context, db: &str) {
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        let db = db.to_string();
        self.access_test_open = true;
        self.access_test_running = true;
        self.access_test_results.clear();
        self.access_test_db = db.clone();
        let path = format!("/{}/_all_docs?limit=1", db.replace('/', "%2F"));
        let curl = Self::curl_command(&config.host, &config.user, &config.password, "GET", &path, None);
        self.log_lines.push_str(&format!("\n{curl}"));
        tokio::spawn(async move {
            let msg = match cmd::access_test(&config, &db, Some(pending.clone()), Some(ctx2.clone())).await {
                Ok(results) => {
                    format!("ACCESS_TEST_DONE|{}", serde_json::to_string(&results).unwrap_or("[]".to_string()))
                }
                Err(e) => format!("ACCESS_TEST_ERR|{e}"),
            };
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx2.request_repaint();
        });
    }

    fn delete_all_docs(&mut self, ctx: &egui::Context, db: &str) {
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        let db = db.to_string();
        self.delete_all_running = true;
        self.log_lines.push_str(&format!("\nDelete all Docs: starting in {db} ..."));
        tokio::spawn(async move {
            let msg = match cmd::delete_all_docs(&config, &db, Some(pending.clone()), Some(ctx2.clone())).await {
                Ok(_) => format!("DEL_ALL_DONE|{db}|Delete all Docs finished."),
                Err(e) => format!("DEL_ALL_ERR|{e}"),
            };
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx2.request_repaint();
        });
    }

    fn purge_all_server_dbs(&mut self, ctx: &egui::Context) {
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        self.purge_all_running = true;
        self.log_lines.push_str("\nPurge deleted Docs (all databases) starting...");
        tokio::spawn(async move {
            let msg = match cmd::purge_server_deleted_docs(&config, Some(pending.clone()), Some(ctx2.clone())).await {
                Ok(_) => "DEL_PURGE_ALL_DONE|Purge deleted Docs finished.".to_string(),
                Err(e) => format!("DEL_PURGE_ALL_ERR|{e}"),
            };
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx2.request_repaint();
        });
    }

    fn purge_deleted_docs(&mut self, ctx: &egui::Context, db: &str) {
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        let db = db.to_string();
        let db_enc = db.replace('/', "%2F");
        let path = format!("/{db_enc}/_changes");
        let curl = Self::curl_command(&config.host, &config.user, &config.password, "GET", &path, None);
        self.log_lines.push_str(&format!("\n{curl}"));
        tokio::spawn(async move {
            let msg = match cmd::purge_db_deleted_docs(&config, &db, Some(pending.clone()), Some(ctx2.clone())).await {
                Ok(line) => format!("DEL_PURGE_OK|{line}"),
                Err(e) => format!("DEL_PURGE_ERR|{db}: {e}"),
            };
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx2.request_repaint();
        });
    }

    fn create_doc(&mut self, ctx: &egui::Context, db: &str, id: &str) {
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        let db = db.to_string();
        let doc = self
            .new_doc_json
            .clone()
            .unwrap_or(serde_json::json!({}));
        let body = serde_json::to_string(&doc).unwrap_or_else(|_| doc.to_string());
        let id = id.trim().to_string();
        let db_enc = db.replace('/', "%2F");
        let (method, path) = if id.trim().is_empty() {
            ("POST", format!("/{db_enc}"))
        } else {
            ("PUT", format!("/{db_enc}/{}", id.replace('/', "%2F")))
        };
        let id_for_call = id.trim().to_string();
        let curl = Self::curl_command(&config.host, &config.user, &config.password, method, &path, Some(&body));
        self.log_lines.push_str(&format!("\n{curl}"));
        tokio::spawn(async move {
            let msg = match cmd::create_doc(&config, &db, &id_for_call, &doc).await {
                Ok(v) => {
                    let created_id = v
                        .get("id")
                        .and_then(|i| i.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| id_for_call.clone());
                    format!("DOC_CREATED|{db}|Created document {created_id}")
                }
                Err(e) => format!("DOC_CREATE_ERR|{e}"),
            };
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx2.request_repaint();
        });
    }

    fn run_db_command(&mut self, ctx: &egui::Context, db: &str, endpoint: &str) {
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        let db = db.to_string();
        let endpoint = endpoint.to_string();
        let log_results = self.log_results;
        let path = format!("/{db}/{endpoint}");
        let curl = Self::curl_command(&config.host, &config.user, &config.password, "GET", &format!("/{}/{}", db.replace('/', "%2F"), endpoint), None);
        self.log_lines.push_str(&format!("\n{curl}"));
        tokio::spawn(async move {
            let base = config.host.trim_end_matches('/').to_string();
            let db_enc = db.replace('/', "%2F");
            let url = format!("{base}/{db_enc}/{endpoint}");
            let http = reqwest::Client::new();
            let req = http.get(&url);
            let req = if !config.user.is_empty() || !config.password.is_empty() {
                req.basic_auth(&config.user, Some(&config.password))
            } else {
                req
            };
            let msg = match req.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    match resp.text().await {
                        Ok(body) => {
                            if log_results {
                                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                                    format!(
                                        "GET {path} [{status}]: {}",
                                        serde_json::to_string_pretty(&v).unwrap_or(body)
                                    )
                                } else {
                                    format!("GET {path} [{status}]: {body}")
                                }
                            } else {
                                format!("GET {path} [{status}]: OK")
                            }
                        }
                        Err(e) => format!("GET {path} failed: {e}"),
                    }
                }
                Err(e) => format!("GET {path} failed: {e}"),
            };
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx2.request_repaint();
        });
    }

    fn disconnect(&mut self) {
        self.connected = false;
        self.connected_host.clear();
        self.db_list.clear();
        self.db_infos.clear();
        self.selected_db = None;
        self.doc_list.clear();
        self.selected_doc = None;
        self.doc_json = None;
        self.doc_loading = false;
        self.users_view = false;
        self.users.clear();
        self.users_loading = false;
        self.show_add_user = false;
        self.editing_user = None;
        self.confirm_delete = None;
        self.log_lines.push_str("\nDisconnected.");
    }

    fn server_name_for_files(&self) -> String {
        if !self.server_name.trim().is_empty() {
            self.server_name.clone()
        } else if !self.connected_host.trim().is_empty() {
            self.connected_host.clone()
        } else {
            self.host.clone()
        }
    }

    fn save_designs(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            let config = self.current_connection_config();
            let pending = self.pending_log.clone();
            let ctx2 = ctx.clone();
            let server_name = self.server_name_for_files();
            self.log_lines.push_str(&format!("\nSaving all Designs to {} ...", path.display()));
            tokio::spawn(async move {
                let msg = match save_all_server_design(&config, &path, &server_name, Some(pending.clone()), Some(ctx2.clone())).await {
                    Ok(_) => format!("Save all Designs completed: {}", path.display()),
                    Err(e) => format!("Save all Designs failed: {e}"),
                };
                println!("{msg}");
                if let Ok(mut p) = pending.lock() {
                    p.push(msg);
                }
                ctx2.request_repaint();
            });
        }
    }

    fn save_docs(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            let config = self.current_connection_config();
            let pending = self.pending_log.clone();
            let ctx2 = ctx.clone();
            let server_name = self.server_name_for_files();
            self.log_lines.push_str(&format!("\nSaving all Docs to {} ...", path.display()));
            tokio::spawn(async move {
                let msg = match save_all_server_dbs(&path, &server_name, &config, Some(pending.clone()), Some(ctx2.clone())).await {
                    Ok(_) => format!("Save all Docs completed: {}", path.display()),
                    Err(e) => format!("Save all Docs failed: {e}"),
                };
                println!("{msg}");
                if let Ok(mut p) = pending.lock() {
                    p.push(msg);
                }
                ctx2.request_repaint();
            });
        }
    }

    fn log_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("log_panel")
            .resizable(true)
            .default_height(180.0)
            .min_height(120.0)
            .max_height(360.0)
            .show(ctx, |ui| {
                ui.group(|ui| {
                    ui.label("Log");
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_source("log_scroll")
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&self.log_lines).monospace(),
                                )
                                .selectable(true)
                                .wrap(),
                            );
                        });
                });
            });
    }

    fn quick_connect(&mut self, ctx: &egui::Context) {
        let mut host = self.host.trim().to_string();
        let user = self.user.clone();
        let password = self.password.clone();
        if host.is_empty() {
            self.log_lines.push_str("\nHost is empty.");
            return;
        }
        if !host.starts_with("http://") && !host.starts_with("https://") {
            host = format!("https://{host}");
        }
        self.host = host.clone();
        self.connected = false;
        self.connected_host.clear();
        self.selected_db = None;
        self.doc_filter.clear();
        self.users_view = false;
        self.users.clear();
        self.users_loading = false;
        self.show_add_user = false;
        self.editing_user = None;
        self.confirm_delete = None;
        self.read_only_public = false;
        self.new_doc_mode = false;
        self.new_doc_json = None;
        self.new_doc_id_input.clear();
        self.db_infos.clear();
        self.db_list.clear();
        let test_path_s = {
            let db = self.database.trim();
            if db.is_empty() {
                "/_all_dbs".to_string()
            } else {
                format!("/{}", db.replace('/', "%2F"))
            }
        };
        self.log_lines.push_str(&format!(
            "\n{}",
            Self::curl_command(&host, &user, &password, "GET", &test_path_s, None)
        ));
        let pending = self.pending_log.clone();
        let ctx = ctx.clone();
        let host_for_state = host.clone();
        let db_test = self.database.trim().to_string();
        tokio::spawn(async move {
            let msg = {
                let cfg = AppConfig {
                    host: host.clone(),
                    user: user.clone(),
                    password: password.clone(),
                    database: db_test.clone(),
                    ..AppConfig::default()
                };
                match cmd::connect_with_names(&cfg).await {
                    Ok((m, dbs)) => {
                        let names = serde_json::to_string(&dbs).unwrap_or_else(|_| "[]".to_string());
                        format!("CONNECTED_OK|{host_for_state}|{m}|{names}")
                    }
                    Err(e) => format!("CONNECTED_ERR|{e}"),
                }
            };
            println!("{msg}");
            if let Ok(mut p) = pending.lock() {
                p.push(msg);
            }
            ctx.request_repaint();
        });
    }
}

impl eframe::App for MyEguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut open_db: Option<String> = None;
        let mut refresh_doc = false;
        let mut refresh_users = false;
        let mut refresh_readonly = false;
        let mut create_refresh = false;
        let mut delete_all_refresh: Option<String> = None;
        let mut probe_ok: Option<serde_json::Value> = None;
        if let Ok(mut pending) = self.pending_log.lock() {
            if !pending.is_empty() {
                for msg in pending.drain(..) {
                    if let Some(rest) = msg.strip_prefix("CONNECTED_OK|") {
                        if let Some((h, detail)) = rest.split_once('|') {
                            self.connected = true;
                            self.connected_host = h.to_string();
                            let (m, names) = detail.split_once('|').unwrap_or((detail, ""));
                            if !names.is_empty() {
                                if let Ok(dbs) = serde_json::from_str::<Vec<String>>(names) {
                                    self.db_list = dbs;
                                }
                            }
                            let db = self.database.trim().to_string();
                            if !db.is_empty() {
                                self.selected_db = Some(db.clone());
                                self.doc_filter.clear();
                                self.doc_list.clear();
                                self.selected_doc = None;
                                self.doc_json = None;
                                self.doc_loading = false;
                                open_db = Some(db);
                            }
                            self.log_lines.push('\n');
                            self.log_lines.push_str(m);
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if let Some(m) = msg.strip_prefix("CONNECTED_ERR|") {
                        self.connected = false;
                        self.connected_host.clear();
                        self.users_view = false;
                        self.users.clear();
                        self.users_loading = false;
                        self.show_add_user = false;
                        self.log_lines.push('\n');
                        self.log_lines.push_str(m);
                    } else if let Some(rest) = msg.strip_prefix("DB_LIST|") {
                        if let Some((json, m)) = rest.split_once('|') {
                            if let Ok(dbs) = serde_json::from_str::<Vec<String>>(json) {
                                self.db_list = dbs;
                                self.db_infos.clear();
                                self.selected_db = None;
                                self.doc_filter.clear();
                                self.doc_list.clear();
                                self.selected_doc = None;
                                self.doc_json = None;
                                self.doc_loading = false;
                            }
                            self.log_lines.push('\n');
                            self.log_lines.push_str(m);
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if let Some(rest) = msg.strip_prefix("DB_INFO|") {
                        if let Ok(env) = serde_json::from_str::<serde_json::Value>(rest) {
                            if let Some(data) = env.get("data") {
                                self.db_infos = cmd::parse_db_infos(data);
                            }
                            if let Some(log) = env.get("log").and_then(|v| v.as_str()) {
                                self.log_lines.push('\n');
                                self.log_lines.push_str(log);
                            }
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if let Some(rest) = msg.strip_prefix("DOC_LIST|") {
                        if let Ok(env) = serde_json::from_str::<serde_json::Value>(rest) {
                            if let Some(docs) = env.get("docs").and_then(|v| v.as_array()) {
                                self.doc_list = docs
                                    .iter()
                                    .filter_map(|d| {
                                        Some(DocRow {
                                            id: d.get("id")?.as_str()?.to_string(),
                                            rev: d
                                                .get("rev")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string(),
                                        })
                                    })
                                    .collect();
                                self.selected_doc = None;
                                self.doc_json = None;
                                self.doc_loading = false;
                            }
                            if let Some(log) = env.get("log").and_then(|v| v.as_str()) {
                                self.log_lines.push('\n');
                                self.log_lines.push_str(log);
                            }
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if let Some(rest) = msg.strip_prefix("DOC|") {
                        self.doc_loading = false;
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(rest) {
                            self.doc_json = Some(v);
                            if let Some(id) = &self.selected_doc {
                                self.log_lines.push_str(&format!("\nLoaded document {id}"));
                            }
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if let Some(m) = msg.strip_prefix("DOC_ERR|") {
                        self.doc_loading = false;
                        self.log_lines.push('\n');
                        self.log_lines.push_str(m);
                    } else if let Some(rest) = msg.strip_prefix("DOC_SAVED|") {
                        if let Some((body, log)) = rest.split_once('|') {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
                                if let Some(rev) = v.get("rev").and_then(|r| r.as_str()) {
                                    if let Some(doc) = self.doc_json.as_mut() {
                                        doc["_rev"] = serde_json::Value::String(rev.to_string());
                                    }
                                }
                            }
                            self.log_lines.push('\n');
                            self.log_lines.push_str(log);
                            refresh_doc = true;
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if let Some(m) = msg.strip_prefix("DOC_SAVE_ERR|") {
                        self.log_lines.push('\n');
                        self.log_lines.push_str(m);
                    } else if let Some(rest) = msg.strip_prefix("DOC_CREATED|") {
                        if let Some((_db, log)) = rest.split_once('|') {
                            self.new_doc_mode = false;
                            self.new_doc_id_input.clear();
                            self.new_doc_json = None;
                            self.log_lines.push('\n');
                            self.log_lines.push_str(log);
                            create_refresh = true;
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if let Some(m) = msg.strip_prefix("DOC_CREATE_ERR|") {
                        self.log_lines.push('\n');
                        self.log_lines.push_str(m);
                    } else if let Some(rest) = msg.strip_prefix("DEL_ALL_DONE|") {
                        if let Some((db, log)) = rest.split_once('|') {
                            self.delete_all_running = false;
                            self.delete_all_open = false;
                            self.log_lines.push('\n');
                            self.log_lines.push_str(log);
                            delete_all_refresh = Some(db.to_string());
                        } else {
                            self.delete_all_running = false;
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if let Some(m) = msg.strip_prefix("DEL_ALL_ERR|") {
                        self.delete_all_running = false;
                        self.log_lines.push('\n');
                        self.log_lines.push_str(m);
                    } else if let Some(m) = msg.strip_prefix("DEL_PURGE_OK|") {
                        self.log_lines.push('\n');
                        self.log_lines.push_str(m);
                    } else if let Some(m) = msg.strip_prefix("DEL_PURGE_ERR|") {
                        self.log_lines.push('\n');
                        self.log_lines.push_str(m);
                    } else if let Some(m) = msg.strip_prefix("DEL_PURGE_ALL_DONE|") {
                        self.purge_all_running = false;
                        self.purge_all_open = false;
                        self.log_lines.push('\n');
                        self.log_lines.push_str(m);
                    } else if let Some(m) = msg.strip_prefix("DEL_PURGE_ALL_ERR|") {
                        self.purge_all_running = false;
                        self.log_lines.push('\n');
                        self.log_lines.push_str(m);
                    } else if let Some(rest) = msg.strip_prefix("USERS|") {
                        self.users_loading = false;
                        if let Ok(env) = serde_json::from_str::<serde_json::Value>(rest) {
                            if let Some(users) = env.get("users").and_then(|v| v.as_array()) {
                                self.users = users
                                    .iter()
                                    .filter_map(|d| serde_json::from_value(d.clone()).ok())
                                    .collect();
                            }
                            if let Some(log) = env.get("log").and_then(|v| v.as_str()) {
                                self.log_lines.push('\n');
                                self.log_lines.push_str(log);
                            }
                            if !self.users.is_empty() {
                                refresh_readonly = true;
                            }
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if let Some(rest) = msg.strip_prefix("USERS_STATUS|") {
                        if let Ok(env) = serde_json::from_str::<serde_json::Value>(rest) {
                            if let Some(statuses) = env.get("statuses").and_then(|v| v.as_array()) {
                                for s in statuses {
                                    let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                    let peruser = s.get("peruser").and_then(|v| v.as_bool()).unwrap_or(false);
                                    let public_ro = s.get("public_ro").and_then(|v| v.as_bool()).unwrap_or(false);
                                    if let Some(u) = self.users.iter_mut().find(|u| u.name == name) {
                                        u.peruser_db_exists = peruser;
                                        u.public_readonly = public_ro;
                                    }
                                }
                            }
                            if let Some(log) = env.get("log").and_then(|v| v.as_str()) {
                                self.log_lines.push('\n');
                                self.log_lines.push_str(log);
                            }
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if let Some(rest) = msg.strip_prefix("DB_PUBLIC|") {
                        if let Ok(env) = serde_json::from_str::<serde_json::Value>(rest) {
                            if let Some(statuses) = env.get("statuses").and_then(|v| v.as_array()) {
                                for s in statuses {
                                    let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                    let public = s.get("public").and_then(|v| v.as_bool()).unwrap_or(false);
                                    if let Some(r) = self.db_infos.iter_mut().find(|r| r.name == name) {
                                        r.public = Some(public);
                                    }
                                }
                            }
                            if let Some(log) = env.get("log").and_then(|v| v.as_str()) {
                                self.log_lines.push('\n');
                                self.log_lines.push_str(log);
                            }
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if msg.strip_prefix("ACCESS_TEST_DONE").is_some() {
                        self.access_test_running = false;
                        self.log_lines.push_str("\nAccess test finished.");
                    } else if let Some(m) = msg.strip_prefix("USERS_ERR|") {
                        self.users_loading = false;
                        self.log_lines.push('\n');
                        self.log_lines.push_str(m);
                    } else if let Some(rest) = msg.strip_prefix("USER_SAVED|") {
                        if let Some((_body, log)) = rest.split_once('|') {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(log);
                            self.show_add_user = false;
                            self.editing_user = None;
                            self.user_name_input.clear();
                            self.user_password_input.clear();
                            self.user_roles_input.clear();
                            self.read_only_public = false;
                            refresh_users = true;
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if let Some(m) = msg.strip_prefix("USER_SAVE_ERR|") {
                        self.log_lines.push('\n');
                        self.log_lines.push_str(m);
                    } else if let Some(rest) = msg.strip_prefix("USER_DELETED|") {
                        if let Some((_body, log)) = rest.split_once('|') {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(log);
                            self.confirm_delete = None;
                            refresh_users = true;
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if let Some(m) = msg.strip_prefix("USER_DELETE_ERR|") {
                        self.log_lines.push('\n');
                        self.log_lines.push_str(m);
                    } else {
                        self.log_lines.push('\n');
                        self.log_lines.push_str(&msg);
                    }
                }
            }
        }
        if let Some(db) = open_db {
            self.load_docs(ctx, &db);
        }
        if refresh_doc {
            if let (Some(db), Some(id)) = (self.selected_db.clone(), self.selected_doc.clone()) {
                self.doc_loading = true;
                self.load_doc(ctx, &db, &id);
            }
        }
        if refresh_users {
            self.load_users(ctx);
        }
        if let Some(v) = probe_ok.take() {
            let scheme = v.get("scheme").and_then(|x| x.as_str()).unwrap_or("https").to_string();
            let host = v.get("host").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let user = v.get("user").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let password = v.get("password").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let database = v.get("database").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let full = format!("{scheme}://{host}");
            self.host = full.clone();
            self.save_server_entry(name, full.clone(), user, password, database);
            self.log_lines
                .push_str(&format!("\nSaved server entry for {full} ({scheme} works)."));
        }
        if let Some(db) = delete_all_refresh {
            self.doc_list.clear();
            self.selected_doc = None;
            self.doc_json = None;
            self.doc_loading = false;
            if self.selected_db.as_deref() == Some(db.as_str()) {
                self.load_docs(ctx, &db);
            }
        }
        if create_refresh {
            if let Some(db) = self.selected_db.clone() {
                self.load_docs(ctx, &db);
            }
        }
        if refresh_readonly {
            self.check_readonly_users(ctx);
        }
        if self.window_help_open {
            egui::Window::new("Help")
                .open(&mut self.window_help_open)
                .show(ctx, |ui| {
                    ui.label("Use the top bar to enter CouchDB connection details (Host, User, Password, Database).");
                    ui.label("Manage saved servers in the left panel (select, add/update, delete).");
                    ui.label("Settings > Log API results: also write full API responses to the log.");
                });
        }

        if self.window_about_open {
            egui::Window::new("About")
                .open(&mut self.window_about_open)
                .show(ctx, |ui| {
                    ui.label("Couchy");
                    ui.label("Convenient utility to get and update CouchDB data.");
                });
        }

        if self.delete_all_open {
            let db = self.selected_db.clone().unwrap_or_default();
            egui::Window::new("Delete all Docs")
                .resizable(false)
                .collapsible(false)
                .default_width(560.0)
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new("Warning").strong().color(egui::Color32::from_rgb(230, 90, 90)));
                    ui.add_space(4.0);
                    ui.label(format!(
                        "This permanently deletes ALL documents in database '{db}' except documents whose id starts with '_' (design documents)."
                    ));
                    ui.label("This cannot be undone.");
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(format!("Documents currently loaded: {}", self.doc_list.len())).weak());
                    ui.add_space(6.0);
                    if self.delete_all_running {
                        ui.label("Deleting documents...");
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        let running = self.delete_all_running;
                        if ui.add_enabled(
                            !running,
                            egui::Button::new(egui::RichText::new("Delete all Docs").color(egui::Color32::from_rgb(230, 90, 90)).strong()),
                        ).clicked() {
                            self.delete_all_docs(ctx, &db);
                        }
                        if ui.button("Cancel").clicked() {
                            self.delete_all_open = false;
                        }
                    });
                });
        }

        if self.purge_all_open {
            let server = if !self.server_name.trim().is_empty() {
                self.server_name.trim().to_string()
            } else if !self.connected_host.trim().is_empty() {
                self.connected_host.clone()
            } else {
                self.host.clone()
            };
            egui::Window::new("Purge deleted Docs")
                .resizable(false)
                .collapsible(false)
                .default_width(560.0)
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new("Warning").strong().color(egui::Color32::from_rgb(230, 90, 90)));
                    ui.add_space(4.0);
                    ui.label(format!(
                        "This permanently purges deleted documents in ALL databases on server '{server}' (including system databases)."
                    ));
                    ui.label("Old revision history of deleted documents is removed. This cannot be undone.");
                    ui.add_space(6.0);
                    if self.purge_all_running {
                        ui.label("Purging deleted documents on all databases...");
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        let running = self.purge_all_running;
                        if ui.add_enabled(
                            !running,
                            egui::Button::new(egui::RichText::new("Purge deleted Docs").color(egui::Color32::from_rgb(230, 90, 90)).strong()),
                        ).clicked() {
                            self.purge_all_server_dbs(ctx);
                        }
                        if ui.button("Cancel").clicked() {
                            self.purge_all_open = false;
                        }
                    });
                });
        }

        if self.access_test_open {
            let db = self.access_test_db.clone();
            egui::Window::new("Access Test")
                .resizable(false)
                .default_width(620.0)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new(format!("Database: {db}")).strong().monospace());
                    ui.add_space(4.0);
                    if self.access_test_results.is_empty() && self.access_test_running {
                        ui.label("Running tests...");
                    }
                    egui::Grid::new("access_test_grid")
                        .striped(true)
                        .num_columns(3)
                        .spacing([16.0, 6.0])
                        .show(ui, |ui| {
                            ui.strong("Test");
                            ui.strong("Result");
                            ui.strong("Detail");
                            ui.end_row();
                            for row in &self.access_test_results {
                                ui.label(egui::RichText::new(&row.name).monospace());
                                if row.ok {
                                    ui.label(
                                        egui::RichText::new("OK")
                                            .color(egui::Color32::from_rgb(110, 210, 110))
                                            .strong(),
                                    );
                                } else {
                                    ui.label(
                                        egui::RichText::new("FAILED")
                                            .color(egui::Color32::from_rgb(230, 90, 90))
                                            .strong(),
                                    );
                                }
                                ui.label(egui::RichText::new(&row.detail).monospace().small());
                                ui.end_row();
                            }
                        });
                    ui.add_space(6.0);
                    if self.access_test_running {
                        ui.label("Running tests...");
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        let running = self.access_test_running;
                        if ui.add_enabled(!running, egui::Button::new("Re-run")).clicked() {
                            self.start_access_test(ctx, &db);
                        }
                        if ui.button("Close").clicked() {
                            self.access_test_open = false;
                        }
                    });
                });
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Connect").clicked() {
                        self.quick_connect(ctx);
                        ui.close_menu();
                    }
                    if ui.button("Disconnect").clicked() {
                        self.disconnect();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Save log to file...").clicked() {
                        if let Some(path) = rfd::FileDialog::new().add_filter("Log", &["log", "txt"]).save_file() {
                            match std::fs::write(&path, &self.log_lines) {
                                Ok(_) => self.log_lines.push_str(&format!("\nLog saved to {}", path.display())),
                                Err(e) => self.log_lines.push_str(&format!("\nSave log failed: {e}")),
                            }
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Edit", |ui| {
                    if ui.button("Copy").clicked() {
                        if !self.log_lines.is_empty() {
                            ctx.copy_text(self.log_lines.clone());
                        }
                        ui.close_menu();
                    }
                    if ui.button("Paste").clicked() {
                        match arboard::Clipboard::new() {
                            Ok(mut clipboard) => match clipboard.get_text() {
                                Ok(text) if !text.is_empty() => {
                                    self.log_lines.push('\n');
                                    self.log_lines.push_str(&text);
                                }
                                Ok(_) => {}
                                Err(e) => self.log_lines.push_str(&format!("\nPaste failed: {e}")),
                            },
                            Err(e) => self.log_lines.push_str(&format!("\nClipboard unavailable: {e}")),
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Clear log").clicked() {
                        self.log_lines.clear();
                        ui.close_menu();
                    }
                });

                ui.menu_button("View", |ui| {
                    if ui.button("Databases").clicked() {
                        self.users_view = false;
                        self.show_add_user = false;
                        self.selected_db = None;
                        self.db_infos.clear();
                        self.doc_list.clear();
                        self.selected_doc = None;
                        self.doc_json = None;
                        self.doc_loading = false;
                        ui.close_menu();
                    }
                    if ui.button("DB info").clicked() {
                        self.users_view = false;
                        self.selected_db = None;
                        if self.db_infos.is_empty() {
                            self.action = "get_dbs_info".to_string();
                            self.run_action(ctx);
                        }
                        ui.close_menu();
                    }
                    if ui.button("Users").clicked() {
                        self.users_view = true;
                        self.show_add_user = false;
                        self.confirm_delete = None;
                        self.load_users(ctx);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Toggle Log panel").clicked() {
                        self.show_log_panel = !self.show_log_panel;
                        ui.close_menu();
                    }
                });

                ui.menu_button("Tools", |ui| {
                    if ui.button("Load DB info").clicked() {
                        self.users_view = false;
                        self.selected_db = None;
                        self.action = "get_dbs_info".to_string();
                        self.run_action(ctx);
                        ui.close_menu();
                    }
                    if ui.button("Check public access").clicked() {
                        self.check_public_dbs(ctx);
                        ui.close_menu();
                    }
                    if ui.button("Access test").clicked() {
                        if let Some(db) = self.selected_db.clone() {
                            self.start_access_test(ctx, &db);
                        } else {
                            self.log_lines.push_str("\nSelect a database first.");
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Save all Designs").clicked() {
                        self.save_designs(ctx);
                        ui.close_menu();
                    }
                    if ui.button("Save all Docs").clicked() {
                        self.save_docs(ctx);
                        ui.close_menu();
                    }
                });

                ui.menu_button("Settings", |ui| {
                    if ui.checkbox(&mut self.log_results, "Log API results").clicked() {
                        ui.close_menu();
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("Help").clicked() {
                        self.window_help_open = true;
                        ui.close_menu();
                    }
                    if ui.button("About Couchy").clicked() {
                        self.window_about_open = true;
                        ui.close_menu();
                    }
                });
            });
        });

        egui::TopBottomPanel::top("connection_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Name:");
                ui.add(egui::TextEdit::singleline(&mut self.server_name).desired_width(120.0));
                ui.label("Host:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.host)
                        .hint_text("https://localhost:5984")
                        .desired_width(340.0),
                );
                ui.label("User:");
                ui.add(egui::TextEdit::singleline(&mut self.user).desired_width(100.0));
                ui.label("Pass:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.password)
                        .password(true)
                        .desired_width(100.0),
                );
                ui.label("DB:");
                ui.add(egui::TextEdit::singleline(&mut self.database).desired_width(180.0));
                if ui.button("Connect").clicked() {
                    self.quick_connect(ctx);
                }
                if ui.button("Add/Update Server").clicked() {
                    self.add_or_update_server(ctx);
                }
            });
            ui.add_space(4.0);
        });

        egui::TopBottomPanel::bottom("status_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (state, color) = if self.connected {
                    ("Connected", egui::Color32::from_rgb(110, 210, 110))
                } else {
                    ("Disconnected", egui::Color32::from_gray(150))
                };
                ui.label(egui::RichText::new(state).color(color).small());
                ui.separator();
                let host = if self.connected_host.is_empty() {
                    "-".to_string()
                } else {
                    self.connected_host.clone()
                };
                ui.label(egui::RichText::new(host).monospace().small());
                ui.separator();
                let db = self.selected_db.as_deref().unwrap_or("-");
                ui.label(egui::RichText::new(format!("Database: {db}")).small());
            });
        });

        if self.show_log_panel {
            self.log_panel(ctx);
        }

        egui::SidePanel::left("servers_panel")
            .resizable(true)
            .default_width(170.0)
            .min_width(140.0)
            .max_width(220.0)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("Servers").strong().size(18.0));
                ui.separator();
                let mut clicked = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, server) in self.servers.iter().enumerate() {
                        let selected = self.selected_server == Some(i);
                        if ui
                            .selectable_label(selected, server.display_name())
                            .clicked()
                        {
                            clicked = Some(i);
                        }
                    }
                });
                if let Some(i) = clicked {
                    self.select_server(i);
                    self.quick_connect(ctx);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("New").clicked() {
                        self.selected_server = None;
                        self.server_name.clear();
                        self.host.clear();
                        self.user.clear();
                        self.password.clear();
                        self.database.clear();
                    }
                    if ui.button("Delete").clicked() {
                        self.delete_selected_server();
                    }
                });
            });

        egui::SidePanel::right("commands_panel")
            .resizable(true)
            .default_width(220.0)
            .min_width(190.0)
            .max_width(260.0)
            .show(ctx, |ui| {
                if self.connected {
                    if self.doc_json.is_some() || self.doc_loading || self.selected_doc.is_some() {
                        ui.label(egui::RichText::new("Document Commands").strong().size(18.0));
                    } else if self.selected_db.is_some() {
                        ui.label(egui::RichText::new("Database Commands").strong().size(18.0));
                    } else {
                        ui.label(egui::RichText::new("Server Commands").strong().size(18.0));
                    }
                } else {
                    ui.label(egui::RichText::new("Commands").strong().size(18.0));
                }
                ui.separator();
                if self.connected {
                    if let Some(db) = self.selected_db.clone() {
                        if ui.button("Create Doc").clicked() {
                            self.new_doc_mode = true;
                            self.new_doc_id_input.clear();
                            self.new_doc_json = Some(serde_json::json!({}));
                            self.selected_doc = None;
                            self.doc_json = None;
                            self.doc_loading = false;
                        }
                        if ui.button("Delete all Docs").clicked() {
                            self.delete_all_open = true;
                        }
                        if ui.button("Purge deleted Docs").clicked() {
                            self.purge_deleted_docs(ctx, &db);
                        }
                        if ui.button("List all Docs").clicked() {
                            self.run_db_command(ctx, &db, "_all_docs");
                        }
                        if ui.button("Access Test").clicked() {
                            self.start_access_test(ctx, &db);
                        }
                    } else {
                    if ui.button("Load DB info").clicked() {
                        self.users_view = false;
                        self.selected_db = None;
                        self.doc_filter.clear();
                        self.doc_list.clear();
                        self.selected_doc = None;
                        self.doc_json = None;
                        self.doc_loading = false;
                        self.action = "get_dbs_info".to_string();
                        self.run_action(ctx);
                    }
                    if ui.button("Check public access").clicked() {
                        self.check_public_dbs(ctx);
                    }
                    if ui.button("List users").clicked() {
                        self.users_view = true;
                        self.show_add_user = false;
                        self.confirm_delete = None;
                        self.load_users(ctx);
                    }
                    if ui.button("Add new User").clicked() {
                        self.users_view = true;
                        self.show_add_user = true;
                        self.editing_user = None;
                        self.user_name_input.clear();
                        self.user_password_input.clear();
                        self.user_roles_input.clear();
                        self.confirm_delete = None;
                        self.read_only_public = false;
                    }
                    if ui.button("Save all Designs").clicked() {
                        self.save_designs(ctx);
                    }
                    if ui.button("Save all Docs").clicked() {
                        self.save_docs(ctx);
                    }
                    if ui.button("Purge deleted Docs").clicked() {
                        self.purge_all_open = true;
                    }
                    }
                }

            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let content_h = ui.available_height().max(120.0);

            ui.group(|ui| {
                ui.set_min_height(content_h);
                ui.set_max_height(content_h);
                ui.horizontal(|ui| {
                    let server_label = if !self.server_name.trim().is_empty() {
                        self.server_name.trim().to_string()
                    } else if !self.connected_host.trim().is_empty() {
                        Self::bare_host(&self.connected_host)
                    } else if !self.host.trim().is_empty() {
                        Self::bare_host(&self.host)
                    } else {
                        "Couchy".to_string()
                    };
                    if self.connected {
                        if ui.link(&server_label).clicked() {
                            self.users_view = false;
                            self.show_add_user = false;
                            self.selected_db = None;
                            self.doc_filter.clear();
                            self.doc_list.clear();
                            self.selected_doc = None;
                            self.doc_json = None;
                            self.doc_loading = false;
                        }
                    } else {
                        ui.label(egui::RichText::new(&server_label).strong());
                    }
                    if self.users_view {
                        ui.label(egui::RichText::new("›").weak());
                        ui.label(egui::RichText::new("Users").strong());
                    } else if let Some(db) = self.selected_db.clone() {
                        let db_label = Self::db_display_label(&db);
                        let doc_mode = self.new_doc_mode
                            || self.selected_doc.is_some()
                            || self.doc_json.is_some()
                            || self.doc_loading;
                        ui.label(egui::RichText::new("›").weak());
                        if doc_mode {
                            if ui.link(&db_label).clicked() {
                                self.selected_doc = None;
                                self.doc_json = None;
                                self.doc_loading = false;
                                self.new_doc_mode = false;
                                self.new_doc_json = None;
                                self.new_doc_id_input.clear();
                            }
                        } else {
                            ui.label(egui::RichText::new(&db_label).strong());
                        }
                        if self.new_doc_mode {
                            ui.label(egui::RichText::new("›").weak());
                            ui.label(egui::RichText::new("Create Doc").strong().monospace());
                        } else if let Some(id) = &self.selected_doc {
                            ui.label(egui::RichText::new("›").weak());
                            ui.label(egui::RichText::new(id).strong().monospace());
                        }
                    }
                });
                ui.separator();
                if self.users_view {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Users").strong().heading());
                        if ui.button("Reload").clicked() {
                            self.load_users(ctx);
                        }
                        if ui.button("Add new User").clicked() {
                            self.show_add_user = true;
                            self.editing_user = None;
                            self.user_name_input.clear();
                            self.user_password_input.clear();
                            self.user_roles_input.clear();
                            self.read_only_public = false;
                        }
                    });
                    if self.show_add_user {
                        let is_edit = self.editing_user.is_some();
                        let key_color = ui.visuals().hyperlink_color;
                        let faint = ui.visuals().faint_bg_color;
                        let title = if is_edit { "Edit user" } else { "New user" };
                        egui::Frame::none()
                            .fill(faint)
                            .rounding(8.0)
                            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(title).heading().strong());
                                        ui.label(egui::RichText::new(if is_edit {
                                            format!("org.couchdb.user:{}", self.user_name_input)
                                        } else {
                                            "org.couchdb.user:<name>".to_string()
                                        }).monospace().weak());
                                    });
                                    ui.add_space(8.0);
                                    ui.separator();
                                    ui.add_space(8.0);
                                    egui::Grid::new("user_form_grid")
                                        .num_columns(2)
                                        .spacing([12.0, 10.0])
                                        .show(ui, |ui| {
                                            ui.label(egui::RichText::new("Username").monospace().strong().color(key_color));
                                            ui.add(
                                                egui::TextEdit::singleline(&mut self.user_name_input)
                                                    .interactive(!is_edit)
                                                    .desired_width(280.0)
                                                    .margin(egui::Margin::symmetric(10.0, 8.0))
                                                    .font(egui::TextStyle::Monospace),
                                            );
                                            ui.end_row();
                                            ui.label(egui::RichText::new("Password").monospace().strong().color(key_color));
                                            ui.add(
                                                egui::TextEdit::singleline(&mut self.user_password_input)
                                                    .password(true)
                                                    .hint_text(if is_edit { "leave empty to keep current" } else { "password" })
                                                    .desired_width(280.0)
                                                    .margin(egui::Margin::symmetric(10.0, 8.0))
                                                    .font(egui::TextStyle::Monospace),
                                            );
                                            ui.end_row();
                                            ui.label(egui::RichText::new("Roles").monospace().strong().color(key_color));
                                            ui.add(
                                                egui::TextEdit::singleline(&mut self.user_roles_input)
                                                    .hint_text("comma separated, e.g. admins, qa")
                                                    .desired_width(360.0)
                                                    .margin(egui::Margin::symmetric(10.0, 8.0))
                                                    .font(egui::TextStyle::Monospace),
                                            );
                                            ui.end_row();
                                            ui.label("");
                                            ui.horizontal(|ui| {
                                                ui.checkbox(&mut self.read_only_public, egui::RichText::new("Read Only Public").monospace().strong());
                                            });
                                            ui.end_row();
                                        });
                                    ui.horizontal(|ui| {
                                        ui.add_space(140.0);
                                        ui.label(egui::RichText::new("opens the user's own database for public read; only this user or admins can edit").weak().small());
                                    });
                                    ui.add_space(8.0);
                                    ui.separator();
                                    ui.add_space(8.0);
                                    ui.horizontal(|ui| {
                                        if ui.button(if is_edit { "Save changes" } else { "Create user" }).clicked() {
                                            self.save_user(ctx);
                                        }
                                        if ui.button("Cancel").clicked() {
                                            self.show_add_user = false;
                                            self.read_only_public = false;
                                        }
                                    });
                                });
                            });
                    }
                    ui.separator();
                    if self.users_loading {
                        ui.label("Loading users...");
                    } else if self.users.is_empty() {
                        ui.label("No users found.");
                    } else {
                        let mut edit_user: Option<UserRow> = None;
                        let mut request_delete: Option<String> = None;
                        let mut cancel_delete = false;
                        let mut do_delete: Option<(String, String)> = None;
                        let confirm_delete = self.confirm_delete.clone();
                        egui::ScrollArea::both()
                            .id_source("users_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                egui::Grid::new("users_grid")
                                    .striped(true)
                                    .num_columns(5)
                                    .spacing([16.0, 6.0])
                                    .show(ui, |ui| {
                                        ui.strong("User");
                                        ui.strong("Roles");
                                        ui.strong("Per-user DB");
                                        ui.strong("Public R/O");
                                        ui.strong("Actions");
                                        ui.end_row();
                                        let yes = egui::RichText::new("yes").color(egui::Color32::from_rgb(110, 210, 110)).strong();
                                        let no = egui::RichText::new("no").color(egui::Color32::from_gray(150));
                                        for row in &self.users {
                                            ui.label(egui::RichText::new(&row.name).monospace().strong());
                                            ui.label(if row.roles.is_empty() {
                                                "-".to_string()
                                            } else {
                                                row.roles.clone()
                                            });
                                            ui.label(if row.peruser_db_exists { yes.clone() } else { no.clone() });
                                            ui.label(if row.public_readonly { yes.clone() } else { no.clone() });
                                            ui.horizontal(|ui| {
                                                if confirm_delete.as_deref() == Some(row.id.as_str()) {
                                                    ui.label(egui::RichText::new("Delete?").strong());
                                                    if ui.button("Yes").clicked() {
                                                        do_delete = Some((row.id.clone(), row.rev.clone()));
                                                    }
                                                    if ui.button("No").clicked() {
                                                        cancel_delete = true;
                                                    }
                                                } else {
                                                    if ui.button("Edit").clicked() {
                                                        edit_user = Some(row.clone());
                                                    }
                                                    if ui.button("Delete").clicked() {
                                                        request_delete = Some(row.id.clone());
                                                    }
                                                }
                                            });
                                            ui.end_row();
                                        }
                                    });
                            });
                        if let Some(row) = edit_user {
                            self.user_name_input = row.name.clone();
                            self.user_password_input.clear();
                            self.user_roles_input = row.roles.clone();
                            self.read_only_public = row.public_readonly;
                            self.editing_user = Some(row);
                            self.show_add_user = true;
                        }
                        if let Some(id) = request_delete {
                            self.confirm_delete = Some(id);
                        }
                        if cancel_delete {
                            self.confirm_delete = None;
                        }
                        if let Some((id, rev)) = do_delete {
                            self.delete_user(ctx, &id, &rev);
                        }
                    }
                } else if let Some(db) = self.selected_db.clone() {
                    if self.new_doc_mode {
                        let mut value = self.new_doc_json.take().unwrap_or(serde_json::json!({}));
                        ui.horizontal(|ui| {
                            ui.label("Document ID:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.new_doc_id_input)
                                    .hint_text("leave empty for auto-generated id")
                                    .desired_width(320.0)
                                    .font(egui::TextStyle::Monospace)
                                    .margin(egui::Margin::symmetric(10.0, 8.0)),
                            );
                            if ui.button("Save").clicked() {
                                self.new_doc_json = Some(value.clone());
                                let id = self.new_doc_id_input.clone();
                                self.create_doc(ctx, &db, &id);
                            }
                        });
                        ui.separator();
                        egui::ScrollArea::both()
                            .id_source("new_doc_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    json_editor(ui, &mut value, &mut self.new_key_input);
                                });
                            });
                        if self.new_doc_mode {
                            self.new_doc_json = Some(value);
                        }
                    } else if self.doc_loading || self.doc_json.is_some() {
                        let mut editor_doc = self.doc_json.take();
                        ui.horizontal(|ui| {
                            if ui.button("Save").clicked() {
                                if let Some(d) = editor_doc.clone() {
                                    self.doc_json = Some(d);
                                    let id = self.selected_doc.clone().unwrap_or_default();
                                    self.save_doc(ctx, &db, &id);
                                }
                            }
                            if ui.button("Reload").clicked() {
                                self.doc_json = None;
                                self.doc_loading = true;
                                let id = self.selected_doc.clone().unwrap_or_default();
                                self.load_doc(ctx, &db, &id);
                                editor_doc = None;
                            }
                        });
                        if self.doc_loading {
                            ui.label("Loading document...");
                        } else if let Some(mut d) = editor_doc {
                            ui.separator();
                            egui::ScrollArea::both()
                                .id_source("doc_editor_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.vertical(|ui| {
                                        json_editor(ui, &mut d, &mut self.new_key_input);
                                    });
                                });
                            self.doc_json = Some(d);
                        }
                    } else {
                    ui.horizontal(|ui| {
                        if ui.button("Reload").clicked() {
                            self.doc_list.clear();
                            self.selected_doc = None;
                            self.doc_json = None;
                            self.doc_loading = false;
                            self.load_docs(ctx, &db);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Find:");
                        ui.add(egui::TextEdit::singleline(&mut self.doc_filter).hint_text("Filter by document id...").desired_width(260.0));
                        if !self.doc_filter.is_empty() {
                            if ui.button("Clear").clicked() {
                            self.doc_filter.clear();
                            }
                        }
                    });
                    if self.doc_list.is_empty() {
                        ui.label("No documents loaded (or still loading)...");
                    } else {
                        let filter = self.doc_filter.trim().to_lowercase();
                        let visible: Vec<&DocRow> = self.doc_list.iter().filter(|d| filter.is_empty() || d.id.to_lowercase().contains(&filter)).collect();
                        ui.label(format!("{} of {} documents", visible.len(), self.doc_list.len()));
                        let mut clicked_doc: Option<String> = None;
                        egui::ScrollArea::both()
                            .id_source("doc_list_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                egui::Grid::new("doc_list_grid")
                                    .striped(true)
                                    .num_columns(2)
                                    .spacing([16.0, 6.0])
                                    .show(ui, |ui| {
                                        ui.strong("Document ID");
                                        ui.strong("Rev");
                                        ui.end_row();
                                        for doc in visible {
                                            let selected = self.selected_doc.as_deref() == Some(doc.id.as_str());
                                            if ui
                                                .selectable_label(selected, doc.id.as_str())
                                                .clicked()
                                            {
                                                clicked_doc = Some(doc.id.clone());
                                            }
                                            ui.label(doc.rev.as_str());
                                            ui.end_row();
                                        }
                                    });
                            });
                        if let Some(id) = clicked_doc {
                            self.selected_doc = Some(id.clone());
                            self.doc_loading = true;
                            self.doc_json = None;
                            self.log_lines
                                .push_str(&format!("\nSelected document: {id}"));
                            self.load_doc(ctx, &db, &id);
                        }
                    }
                    }
                } else if !self.db_infos.is_empty() {
                    ui.label("Databases Info");
                    egui::ScrollArea::both()
                        .id_source("db_info_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Grid::new("db_info_grid")
                                .striped(true)
                                .num_columns(7)
                                .spacing([16.0, 6.0])
                                .show(ui, |ui| {
                                    ui.strong("Database");
                                    ui.strong("Docs");
                                    ui.strong("Deleted");
                                    ui.strong("Data");
                                    ui.strong("Disk");
                                    ui.strong("Public");
                                    ui.strong("Compacting");
                                    ui.end_row();
                                    let mut clicked_db: Option<String> = None;
                                    for r in &self.db_infos {
                                        let label = Self::db_display_label(&r.name);
                                        let selected = self.database == r.name;
                                        if ui.selectable_label(selected, label).clicked() {
                                            clicked_db = Some(r.name.clone());
                                        }
                                        ui.label(
                                            r.doc_count
                                                .map(|v| v.to_string())
                                                .unwrap_or_else(|| "-".to_string()),
                                        );
                                        ui.label(
                                            r.doc_del_count
                                                .map(|v| v.to_string())
                                                .unwrap_or_else(|| "-".to_string()),
                                        );
                                        ui.label(Self::format_bytes(r.data_size));
                                        ui.label(Self::format_bytes(r.disk_size));
                                        match r.public {
                                            Some(true) => ui.label(egui::RichText::new("yes").color(egui::Color32::from_rgb(110, 210, 110)).strong()),
                                            Some(false) => ui.label(egui::RichText::new("no").color(egui::Color32::from_gray(150))),
                                            None => ui.label(egui::RichText::new("...").weak()),
                                        };
                                        ui.label(match r.compact_running {
                                            Some(true) => "yes",
                                            Some(false) => "no",
                                            None => "-",
                                        });
                                        ui.end_row();
                                    }
                                    if let Some(db) = clicked_db {
                                        self.database = db.clone();
                                        self.selected_db = Some(db.clone());
                                        self.doc_list.clear();
                                        self.selected_doc = None;
                                        self.doc_json = None;
                                        self.doc_loading = false;
                                        self.doc_filter.clear();
                                        self.log_lines.push_str(&format!("\nSelected database: {db}"));
                                        self.load_docs(ctx, &db);
                                    }
                                });
                        });
                } else if self.db_list.is_empty() {
                    ui.label("Controls / content area");
                } else {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("Databases ({})", self.db_list.len())).strong());
                        if ui.button("Load DB info").clicked() {
                            self.action = "get_dbs_info".to_string();
                            self.run_action(ctx);
                        }
                    });
                    egui::ScrollArea::vertical().id_source("db_list_scroll").show(ui, |ui| {
                        ui.vertical(|ui| {
                            for db in &self.db_list.clone() {
                                let selected = self.database == *db;
                                let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
                                let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
                                let visuals = ui.style().interact_selectable(&response, selected);
                                ui.painter().rect(rect, visuals.rounding, visuals.bg_fill, visuals.bg_stroke);
                                let text_pos = egui::pos2(
                                    rect.left() + ui.spacing().button_padding.x,
                                    rect.center().y,
                                );
                                let label = Self::db_display_label(db);
                                ui.painter().text(
                                    text_pos,
                                    egui::Align2::LEFT_CENTER,
                                    label,
                                    egui::TextStyle::Button.resolve(ui.style()),
                                    visuals.fg_stroke.color,
                                );
                                if response.clicked() {
                                    self.database = db.clone();
                                    self.selected_db = Some(db.clone());
                                    self.doc_list.clear();
                                    self.selected_doc = None;
                                    self.doc_json = None;
                                    self.doc_loading = false;
                                    self.doc_filter.clear();
                                    self.log_lines.push_str(&format!("\nSelected database: {db}"));
                                    self.load_docs(ctx, db);
                                }
                            }
                        });
                    });
                }
            });
        });
    }
}
