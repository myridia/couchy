use clap::Parser;
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
            viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 1100.0]),
            ..Default::default()
        };
        let _ = eframe::run_native(
            "Couchy",
            native_options,
            Box::new(|cc| Ok(Box::new(MyEguiApp::new(cc)))),
        );
    } else {
        println!("...run console");
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

struct DbInfoRow {
    name: String,
    doc_count: Option<u64>,
    doc_del_count: Option<u64>,
    data_size: Option<u64>,
    disk_size: Option<u64>,
    compact_running: Option<bool>,
}

struct DocRow {
    id: String,
    rev: String,
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

    fn current_connection_config(&self) -> AppConfig {
        ServerConfig {
            name: self.server_name.clone(),
            host: self.host.clone(),
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

    fn add_or_update_server(&mut self) {
        let server = ServerConfig {
            name: if self.server_name.trim().is_empty() {
                if self.host.trim().is_empty() {
                    format!("Server {}", self.servers.len() + 1)
                } else {
                    self.host.clone()
                }
            } else {
                self.server_name.clone()
            },
            host: self.host.clone(),
            user: self.user.clone(),
            password: self.password.clone(),
            database: self.database.clone(),
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

    fn parse_db_infos(data: &serde_json::Value) -> Vec<DbInfoRow> {
        let entries = if let Some(arr) = data.as_array() {
            arr.clone()
        } else if let Some(obj) = data.as_object() {
            obj.iter()
                .map(|(k, v)| serde_json::json!({ "key": k, "info": v }))
                .collect()
        } else {
            Vec::new()
        };

        let mut rows = Vec::new();
        for entry in entries {
            let info = entry.get("info").unwrap_or(&entry);
            let name = info
                .get("db_name")
                .and_then(|v| v.as_str())
                .or_else(|| entry.get("key").and_then(|v| v.as_str()))
                .unwrap_or("?")
                .to_string();
            let doc_count = info.get("doc_count").and_then(|v| v.as_u64());
            let doc_del_count = info.get("doc_del_count").and_then(|v| v.as_u64());
            let data_size = info
                .get("sizes")
                .and_then(|s| s.get("active"))
                .and_then(|v| v.as_u64())
                .or_else(|| info.get("data_size").and_then(|v| v.as_u64()));
            let disk_size = info
                .get("sizes")
                .and_then(|s| s.get("file"))
                .and_then(|v| v.as_u64())
                .or_else(|| info.get("disk_size").and_then(|v| v.as_u64()));
            let compact_running = info.get("compact_running").and_then(|v| v.as_bool());
            rows.push(DbInfoRow {
                name,
                doc_count,
                doc_del_count,
                data_size,
                disk_size,
                compact_running,
            });
        }
        rows
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
                "get_all_dbs" => match couch_rs::Client::new(&config.host, &config.user, &config.password) {
                    Ok(client) => match client.list_dbs().await {
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
                    Err(e) => format!("GET /_all_dbs failed: {e}"),
                },
                "get_dbs_info" => {
                    match couch_rs::Client::new(&config.host, &config.user, &config.password) {
                        Ok(client) => match client.list_dbs().await {
                            Ok(dbs) => {
                                let base = config.host.trim_end_matches('/').to_string();
                                let url = format!("{base}/_dbs_info");
                                let body_json = serde_json::json!({ "keys": dbs.clone() }).to_string();
                                let curl = Self::curl_command(&config.host, &config.user, &config.password, "POST", "/_dbs_info", Some(&body_json));
                                let http = reqwest::Client::new();
                                let req = http.post(&url).json(&serde_json::json!({"keys": dbs}));
                                let req = if !config.user.is_empty() || !config.password.is_empty() {
                                    req.basic_auth(&config.user, Some(&config.password))
                                } else {
                                    req
                                };
                                match req.send().await {
                                    Ok(resp) => match resp.text().await {
                                        Ok(body) => {
                                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                                                let log = if log_results {
                                                    format!(
                                                        "{curl}\nPOST /_dbs_info: {}",
                                                        serde_json::to_string_pretty(&v).unwrap_or_else(|_| body.clone())
                                                    )
                                                } else {
                                                    format!("{curl}\nPOST /_dbs_info: OK")
                                                };
                                                let envelope = serde_json::json!({ "data": v, "log": log.clone() });
                                                format!("DB_INFO|{}", serde_json::to_string(&envelope).unwrap_or(log))
                                            } else {
                                                if log_results {
                                                    format!("{curl}\nPOST /_dbs_info: {body}")
                                                } else {
                                                    format!("{curl}\nPOST /_dbs_info: OK")
                                                }
                                            }
                                        }
                                        Err(e) => format!("GET /_dbs_info failed: {e}"),
                                    },
                                    Err(e) => format!("GET /_dbs_info failed: {e}"),
                                }
                            }
                            Err(e) => format!("GET /_dbs_info failed: {e}"),
                        },
                        Err(e) => format!("GET /_dbs_info failed: {e}"),
                    }
                }
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
            let base = config.host.trim_end_matches('/').to_string();
            let db_enc = db.replace('/', "%2F");
            let url = format!("{base}/{db_enc}/_all_docs");
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
                        Ok(body) => match serde_json::from_str::<serde_json::Value>(&body) {
                            Ok(v) => {
                                let docs: Vec<serde_json::Value> = v
                                    .get("rows")
                                    .and_then(|r| r.as_array())
                                    .cloned()
                                    .unwrap_or_default()
                                    .into_iter()
                                    .map(|row| {
                                        serde_json::json!({
                                            "id": row
                                                .get("id")
                                                .or_else(|| row.get("key"))
                                                .and_then(|x| x.as_str())
                                                .unwrap_or(""),
                                            "rev": row
                                                .get("value")
                                                .and_then(|v| v.get("rev"))
                                                .and_then(|r| r.as_str())
                                                .unwrap_or(""),
                                        })
                                    })
                                    .collect();
                                let count = docs.len();
                                let envelope = serde_json::json!({
                                    "docs": docs,
                                    "log": format!("Loaded {count} document IDs from {db} [{status}]"),
                                });
                                format!(
                                    "DOC_LIST|{}",
                                    serde_json::to_string(&envelope).unwrap_or_default()
                                )
                            }
                            Err(_) => format!("GET /{db}/_all_docs failed [{status}]: {body}"),
                        },
                        Err(e) => format!("GET /{db}/_all_docs failed: {e}"),
                    }
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

    fn quick_connect(&mut self, ctx: &egui::Context) {
        let mut host = self.host.trim().to_string();
        let user = self.user.clone();
        let password = self.password.clone();
        if host.is_empty() {
            self.log_lines.push_str("\nHost is empty.");
            return;
        }
        if !host.starts_with("http://") && !host.starts_with("https://") {
            host = format!("http://{host}");
            self.host = host.clone();
        }
        self.connected = false;
        self.connected_host.clear();
        self.selected_db = None;
        self.doc_filter.clear();
        self.log_lines.push_str(&format!(
            "\n{}",
            Self::curl_command(&host, &user, &password, "GET", "/_all_dbs", None)
        ));
        let pending = self.pending_log.clone();
        let ctx = ctx.clone();
        let host_for_state = host.clone();
        tokio::spawn(async move {
            let msg = match couch_rs::Client::new(&host, &user, &password) {
                Ok(client) => match client.list_dbs().await {
                    Ok(dbs) => format!("CONNECTED_OK|{host_for_state}|Connected to {host} ({} databases)", dbs.len()),
                    Err(e) => format!("CONNECTED_ERR|Connection failed: {host}: {e}"),
                },
                Err(e) => format!("CONNECTED_ERR|Connection failed: {host}: {e}"),
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
        let mut fetch_db_info = false;
        if let Ok(mut pending) = self.pending_log.lock() {
            if !pending.is_empty() {
                for msg in pending.drain(..) {
                    if let Some(rest) = msg.strip_prefix("CONNECTED_OK|") {
                        if let Some((h, m)) = rest.split_once('|') {
                            self.connected = true;
                            self.connected_host = h.to_string();
                            fetch_db_info = true;
                            self.log_lines.push('\n');
                            self.log_lines.push_str(m);
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if let Some(m) = msg.strip_prefix("CONNECTED_ERR|") {
                        self.connected = false;
                        self.connected_host.clear();
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
                                self.db_infos = Self::parse_db_infos(data);
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
                            }
                            if let Some(log) = env.get("log").and_then(|v| v.as_str()) {
                                self.log_lines.push('\n');
                                self.log_lines.push_str(log);
                            }
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else {
                        self.log_lines.push('\n');
                        self.log_lines.push_str(&msg);
                    }
                }
            }
        }
        if fetch_db_info {
            self.action = "get_dbs_info".to_string();
            self.run_action(ctx);
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

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Couchy", |ui| {
                    if ui.button("About Couchy").clicked() {
                        self.window_about_open = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit Couchy").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Views", |ui| {
                    if ui.button("Save all_design").clicked() {
                        self.action = "save_all_design".to_string();
                        self.log_lines.push_str("\nSelected action: save_all_design");
                        ui.close_menu();
                    }
                    if ui.button("Save all Designs").clicked() {
                        self.action = "save_all_server_design".to_string();
                        self.log_lines
                            .push_str("\nSelected action: save_all_server_design");
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
                        .hint_text("http://localhost:5984")
                        .desired_width(220.0),
                );
                ui.label("User:");
                ui.add(egui::TextEdit::singleline(&mut self.user).desired_width(100.0));
                ui.label("Password:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.password)
                        .password(true)
                        .desired_width(100.0),
                );
                ui.label("Database:");
                ui.add(egui::TextEdit::singleline(&mut self.database).desired_width(120.0));
                if ui.button("Quick Connect").clicked() {
                    self.quick_connect(ctx);
                }
                if ui.button("Add/Update Server").clicked() {
                    self.add_or_update_server();
                }
            });
            ui.add_space(4.0);
        });

        egui::SidePanel::left("servers_panel")
            .resizable(true)
            .default_width(220.0)
            .show(ctx, |ui| {
                ui.heading("Servers");
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
            .default_width(320.0)
            .show(ctx, |ui| {
                if self.connected {
                    if self.selected_db.is_some() {
                        ui.heading("Database Commands");
                    } else {
                        ui.heading("Server Commands");
                    }
                } else {
                    ui.heading("Commands");
                }
                ui.separator();
                if self.connected {
                    if let Some(db) = self.selected_db.clone() {
                        if ui.button("Get_all_docs").clicked() {
                            self.run_db_command(ctx, &db, "_all_docs");
                        }
                        if ui.button("Get_all_design_docs").clicked() {
                            self.run_db_command(ctx, &db, "_design_docs");
                        }
                        ui.separator();
                        if ui.button("Back to Server Commands").clicked() {
                            self.selected_db = None;
                            self.doc_filter.clear();
                            self.doc_list.clear();
                            self.selected_doc = None;
                        }
                    } else {
                    if ui.button("Save all Designs").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            let config = self.current_connection_config();
                            let pending = self.pending_log.clone();
                            let ctx2 = ctx.clone();
                            let server_name = if !self.server_name.trim().is_empty() {
                                self.server_name.clone()
                            } else if !self.connected_host.trim().is_empty() {
                                self.connected_host.clone()
                            } else {
                                self.host.clone()
                            };
                            self.action = "save_all_server_design".to_string();
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
                    if ui.button("Save all Docs").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            let config = self.current_connection_config();
                            let pending = self.pending_log.clone();
                            let ctx2 = ctx.clone();
                            let server_name = if !self.server_name.trim().is_empty() {
                                self.server_name.clone()
                            } else if !self.connected_host.trim().is_empty() {
                                self.connected_host.clone()
                            } else {
                                self.host.clone()
                            };
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
                    if ui.button("Get_dbs").clicked() {
                        self.action = "get_dbs_info".to_string();
                        self.run_action(ctx);
                    }
                    }
                }

            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let total_h = ui.available_height();
            let log_h = (total_h * 0.28).max(140.0);
            let content_h = (total_h - log_h - 24.0).max(120.0);

            ui.group(|ui| {
                ui.set_min_height(content_h);
                ui.set_max_height(content_h);
                ui.label("Content");
                ui.separator();
                if let Some(db) = self.selected_db.clone() {
                    ui.horizontal(|ui| {
                        if ui.button("Back").clicked() {
                            self.selected_db = None;
                            self.doc_filter.clear();
                            self.doc_list.clear();
                            self.selected_doc = None;
                            self.doc_filter.clear();
                        }
                        ui.label(format!("Documents in {db}"));
                        if ui.button("Reload").clicked() {
                            self.doc_list.clear();
                            self.selected_doc = None;
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
                            self.log_lines
                                .push_str(&format!("\nSelected document: {id}"));
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
                                .num_columns(6)
                                .spacing([16.0, 6.0])
                                .show(ui, |ui| {
                                    ui.strong("Database");
                                    ui.strong("Docs");
                                    ui.strong("Deleted");
                                    ui.strong("Data");
                                    ui.strong("Disk");
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
                                        self.doc_filter.clear();
                                        self.log_lines.push_str(&format!("\nSelected database: {db}"));
                                        self.load_docs(ctx, &db);
                                    }
                                });
                        });
                } else if self.db_list.is_empty() {
                    ui.label("Controls / content area");
                } else {
                    ui.label("Databases");
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
                                    self.doc_filter.clear();
                                    self.log_lines.push_str(&format!("\nSelected database: {db}"));
                                    self.load_docs(ctx, db);
                                }
                            }
                        });
                    });
                }
            });

            ui.add_space(8.0);
            ui.group(|ui| {
                ui.set_min_height(log_h);
                ui.set_max_height(log_h);
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
}
