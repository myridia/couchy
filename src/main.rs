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
    public: Option<bool>,
}

struct AccessTestRow {
    name: String,
    ok: bool,
    detail: String,
}

struct DocRow {
    id: String,
    rev: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct UserRow {
    id: String,
    name: String,
    rev: String,
    roles: String,
    doc: serde_json::Value,
    #[serde(default)]
    peruser_db_exists: bool,
    #[serde(default)]
    public_readonly: bool,
}

fn test_path(database: &str) -> String {
    let db = database.trim();
    if db.is_empty() {
        "/_all_dbs".to_string()
    } else {
        format!("/{}", db.replace('/', "%2F"))
    }
}

async fn fetch_rev(
    http: &reqwest::Client,
    base: &str,
    user: &str,
    password: &str,
    path: &str,
) -> Option<String> {
    let url = format!("{base}{path}");
    let req = http.get(&url);
    let req = if !user.is_empty() || !password.is_empty() {
        req.basic_auth(user, Some(password))
    } else {
        req
    };
    match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            let v = resp.json::<serde_json::Value>().await.ok()?;
            v.get("_rev").and_then(|r| r.as_str()).map(|s| s.to_string())
        }
        _ => None,
    }
}

async fn get_json(
    http: &reqwest::Client,
    base: &str,
    user: &str,
    password: &str,
    path: &str,
) -> (u16, Option<serde_json::Value>) {
    let url = format!("{base}{path}");
    let req = http.get(&url);
    let req = if !user.is_empty() || !password.is_empty() {
        req.basic_auth(user, Some(password))
    } else {
        req
    };
    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.json::<serde_json::Value>().await.ok();
            (status, body)
        }
        Err(_) => (0, None),
    }
}

async fn put_json(
    http: &reqwest::Client,
    base: &str,
    user: &str,
    password: &str,
    path: &str,
    body: &str,
) -> (u16, String) {
    let url = format!("{base}{path}");
    let req = http
        .put(&url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .body(body.to_string());
    let req = if !user.is_empty() || !password.is_empty() {
        req.basic_auth(user, Some(password))
    } else {
        req
    };
    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            (status, body)
        }
        Err(e) => (0, e.to_string()),
    }
}

async fn purge_db_deleted_docs<F: Fn(String)>(
    http: &reqwest::Client,
    base: &str,
    user: &str,
    password: &str,
    dbname: &str,
    send: F,
) -> Result<String, String> {
    let db_enc = dbname.replace('/', "%2F");
    let (status, parsed) = get_json(http, base, user, password, &format!("/{db_enc}/_changes")).await;
    if status != 200 {
        return Err(format!("[{status}] GET _changes failed"));
    }
    let parsed = parsed.ok_or_else(|| "GET _changes failed: unexpected response".to_string())?;

    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(results) = parsed.get("results").and_then(|r| r.as_array()) {
        for row in results {
            let deleted = row.get("deleted").and_then(|d| d.as_bool()).unwrap_or(false);
            if !deleted {
                continue;
            }
            let id = row.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let rev = row
                .get("changes")
                .and_then(|c| c.as_array())
                .and_then(|a| a.first())
                .and_then(|c| c.get("rev").and_then(|r| r.as_str()))
                .unwrap_or("");
            if !id.is_empty() && !rev.is_empty() {
                pairs.push((id.to_string(), rev.to_string()));
            }
        }
    }
    let total = pairs.len();
    if total == 0 {
        return Ok(format!("{dbname}: 0 deleted documents to purge"));
    }
    let mut purged = 0usize;
    for chunk in pairs.chunks(99) {
        let mut map = serde_json::Map::new();
        for (id, rev) in chunk {
            map.insert(id.clone(), serde_json::json!([rev]));
        }
        let body = serde_json::Value::Object(map).to_string();
        let purge_path = format!("/{db_enc}/_purge");
        let resp = http
            .post(format!("{base}{purge_path}"))
            .header("Content-Type", "application/json")
            .basic_auth(user, Some(password))
            .body(body)
            .send()
            .await
            .map_err(|e| format!("POST {purge_path} failed: {e}"))?;
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        if status != 200 && status != 201 && status != 202 {
            let snippet: String = body.chars().take(200).collect();
            return Err(format!("[{status}] POST {purge_path} failed: {snippet}"));
        }
        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::json!({}));
        let n = v
            .get("purged")
            .and_then(|p| p.as_object())
            .map(|o| o.len())
            .unwrap_or(chunk.len());
        purged += n;
        send(format!("  purged {purged}/{total} deleted documents..."));
    }
    Ok(format!("{dbname}: purged {purged} deleted documents"))
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
                public: None,
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
            let base = config.host.trim_end_matches('/').to_string();
            let url = format!("{base}{path}");
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
                            if serde_json::from_str::<serde_json::Value>(&body).is_ok() && (200..300).contains(&status) {
                                format!("DOC|{body}")
                            } else {
                                format!("DOC_ERR|GET {path} [{status}]: {body}")
                            }
                        }
                        Err(e) => format!("DOC_ERR|GET {path} failed: {e}"),
                    }
                }
                Err(e) => format!("DOC_ERR|GET {path} failed: {e}"),
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
            let base = config.host.trim_end_matches('/').to_string();
            let url = format!("{base}{path}");
            let http = reqwest::Client::new();
            let req = http.put(&url).header("Content-Type", "application/json").body(body);
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
                            if (200..300).contains(&status) {
                                if log_results {
                                    format!("DOC_SAVED|{body}|PUT {path} [{status}]")
                                } else {
                                    format!("DOC_SAVED|{body}|PUT {path} [{status}]: OK")
                                }
                            } else {
                                format!("DOC_SAVE_ERR|PUT {path} [{status}]: {body}")
                            }
                        }
                        Err(e) => format!("DOC_SAVE_ERR|PUT {path} failed: {e}"),
                    }
                }
                Err(e) => format!("DOC_SAVE_ERR|PUT {path} failed: {e}"),
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
            let base = config.host.trim_end_matches('/').to_string();
            let url = format!("{base}{path}");
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
                            if (200..300).contains(&status) {
                                match serde_json::from_str::<serde_json::Value>(&body) {
                                    Ok(v) => {
                                        let mut users = Vec::new();
                                        if let Some(rows) = v.get("rows").and_then(|r| r.as_array()) {
                                            for row in rows {
                                                let doc = row.get("doc").cloned().unwrap_or(serde_json::Value::Null);
                                                if doc.get("type").and_then(|t| t.as_str()) != Some("user") {
                                                    continue;
                                                }
                                                let id = row.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                                                let name = doc.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                                                let rev = doc.get("_rev").and_then(|r| r.as_str()).unwrap_or("").to_string();
                                                let roles = doc.get("roles").and_then(|r| r.as_array()).map(|a| {
                                                    a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")
                                                }).unwrap_or_default();
                                                users.push(serde_json::json!({
                                                    "id": id,
                                                    "name": name,
                                                    "rev": rev,
                                                    "roles": roles,
                                                    "doc": doc,
                                                    "peruser_db_exists": false,
                                                    "public_readonly": false,
                                                }));
                                            }
                                        }
                                        let envelope = serde_json::json!({
                                            "users": users,
                                            "log": format!("Loaded {} users [{status}]", users.len()),
                                        });
                                        format!("USERS|{}", serde_json::to_string(&envelope).unwrap_or_default())
                                    }
                                    Err(e) => format!("USERS_ERR|Failed to parse users: {e}"),
                                }
                            } else {
                                format!("USERS_ERR|GET {path} [{status}]: {body}")
                            }
                        }
                        Err(e) => format!("USERS_ERR|GET {path} failed: {e}"),
                    }
                }
                Err(e) => format!("USERS_ERR|GET {path} failed: {e}"),
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
            let base = config.host.trim_end_matches('/').to_string();
            let http = reqwest::Client::new();
            let mut statuses = Vec::new();
            for name in &names {
                let db_enc = name.replace('/', "%2F");
                let path = format!("/{db_enc}");
                let (status, _) = get_json(&http, &base, "", "", &path).await;
                statuses.push(serde_json::json!({
                    "name": name,
                    "public": status == 200,
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
            let base = config.host.trim_end_matches('/').to_string();
            let http = reqwest::Client::new();
            let mut statuses = Vec::new();
            for u in &users {
                let hex: String = u.name.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
                let db = format!("userdb-{hex}");
                let (status, _) = get_json(&http, &base, &config.user, &config.password, &format!("/{db}")).await;
                let peruser = status == 200;
                let mut public_ro = false;
                if peruser {
                    let (dstatus, ddoc) = get_json(
                        &http,
                        &base,
                        &config.user,
                        &config.password,
                        &format!("/{db}/_design/auth"),
                    )
                    .await;
                    public_ro = dstatus == 200
                        && ddoc
                            .as_ref()
                            .map(|v| v.get("validate_doc_update").is_some())
                            .unwrap_or(false);
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

    async fn setup_readonly_db(
        host: String,
        user: String,
        password: String,
        name: String,
    ) -> Vec<String> {
        let mut logs = Vec::new();
        let base = host.trim_end_matches('/').to_string();
        let hex: String = name.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        let db = format!("userdb-{hex}");
        let http = reqwest::Client::new();

        let js_name = name.replace('\\', "\\\\").replace('\'', "\\'");
        let validate = format!(
            "function(newDoc, oldDoc, userCtx) {{ if (userCtx.roles.indexOf('admin') !== -1) {{ return; }} if (userCtx.name === '{js_name}') {{ return; }} throw ({{ forbidden: 'Only the user or admins may edit the database' }}); }}"
        );

        let dpath = format!("/{db}/_design/auth");
        let mut ddoc = serde_json::json!({
            "_id": "_design/auth",
            "language": "javascript",
            "validate_doc_update": validate,
        });
        if let Some(rev) = fetch_rev(&http, &base, &user, &password, &dpath).await {
            ddoc["_rev"] = serde_json::Value::String(rev);
        }
        let dbody = serde_json::to_string_pretty(&ddoc).unwrap_or_else(|_| ddoc.to_string());
        logs.push(Self::curl_command(&host, &user, &password, "PUT", &dpath, Some(&dbody)));
        let (status, body) = put_json(&http, &base, &user, &password, &dpath, &dbody).await;
        let snippet: String = body.chars().take(200).collect();
        logs.push(format!("  {db}/_design/auth [{status}] {snippet}"));

        let spath = format!("/{db}/_security");
        let mut sec = serde_json::json!({
            "admins": { "names": [], "roles": [] },
            "members": { "names": [], "roles": [] },
        });
        if let Some(rev) = fetch_rev(&http, &base, &user, &password, &spath).await {
            sec["_rev"] = serde_json::Value::String(rev);
        }
        let sbody = serde_json::to_string(&sec).unwrap_or_else(|_| sec.to_string());
        logs.push(Self::curl_command(&host, &user, &password, "PUT", &spath, Some(&sbody)));
        let (status, body) = put_json(&http, &base, &user, &password, &spath, &sbody).await;
        let snippet: String = body.chars().take(200).collect();
        logs.push(format!("  {db}/_security [{status}] {snippet}"));

        logs
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
        let is_edit = editing.is_some();
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
            let base = config.host.trim_end_matches('/').to_string();
            let url = format!("{base}{path}");
            let http = reqwest::Client::new();
            let req = http
                .put(&url)
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .body(body);
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
                            if (200..300).contains(&status) {
                                let action = if is_edit { "Updated" } else { "Created" };
                                let mut log = format!("{action} {name} [{status}]");
                                if make_readonly {
                                    let extra = Self::setup_readonly_db(
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
                                format!("USER_SAVED|{body}|{log}")
                            } else {
                                format!("USER_SAVE_ERR|PUT {path} [{status}]: {body}")
                            }
                        }
                        Err(e) => format!("USER_SAVE_ERR|PUT {path} failed: {e}"),
                    }
                }
                Err(e) => format!("USER_SAVE_ERR|PUT {path} failed: {e}"),
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
            let base = config.host.trim_end_matches('/').to_string();
            let url = format!("{base}{path}");
            let http = reqwest::Client::new();
            let req = http.delete(&url);
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
                            if (200..300).contains(&status) {
                                format!("USER_DELETED|{body}|Deleted {id} [{status}]")
                            } else {
                                format!("USER_DELETE_ERR|DELETE {path} [{status}]: {body}")
                            }
                        }
                        Err(e) => format!("USER_DELETE_ERR|DELETE {path} failed: {e}"),
                    }
                }
                Err(e) => format!("USER_DELETE_ERR|DELETE {path} failed: {e}"),
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
        let db_enc = db.replace('/', "%2F");
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let user = config.user.clone();
        let password = config.password.clone();
        let host = config.host.clone();
        tokio::spawn(async move {
            let base = host.trim_end_matches('/').to_string();
            let http = reqwest::Client::new();
            let send = |msg: String| {
                println!("{msg}");
                if let Ok(mut p) = pending.lock() {
                    p.push(msg);
                }
                ctx2.request_repaint();
            };
            let row = |name: &str, ok: bool, detail: String| {
                let row = serde_json::json!({ "name": name, "ok": ok, "detail": detail });
                format!("ACCESS_TEST_ROW|{row}")
            };

            let path1 = format!("/{db_enc}/_all_docs?limit=1");
            send(Self::curl_command(&host, &user, &password, "GET", &path1, None));
            let resp = http
                .get(format!("{base}{path1}"))
                .basic_auth(&user, Some(&password))
                .send()
                .await;
            let (status, body) = match resp {
                Ok(r) => {
                    let s = r.status().as_u16();
                    let b = r.text().await.unwrap_or_default();
                    (s, b)
                }
                Err(e) => (0, e.to_string()),
            };
            let total = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("total_rows").and_then(|t| t.as_u64()));
            let detail = match total {
                Some(t) => format!("[{status}] total_rows={t}"),
                None => format!("[{status}] {}", body.chars().take(120).collect::<String>()),
            };
            send(row("Read with credentials", status == 200, detail));

            let path2 = path1.clone();
            send(Self::curl_command(&host, "", "", "GET", &path2, None));
            let resp = http.get(format!("{base}{path2}")).send().await;
            let (status, body) = match resp {
                Ok(r) => {
                    let s = r.status().as_u16();
                    let b = r.text().await.unwrap_or_default();
                    (s, b)
                }
                Err(e) => (0, e.to_string()),
            };
            let total = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("total_rows").and_then(|t| t.as_u64()));
            let detail = match total {
                Some(t) => format!("[{status}] total_rows={t}"),
                None => format!("[{status}] {}", body.chars().take(120).collect::<String>()),
            };
            send(row("Read anonymous", status == 200, detail));

            let test_id = format!("couchy_access_test_{ts}");
            let path3 = format!("/{db_enc}/{test_id}");
            let doc3 = serde_json::json!({
                "_id": test_id,
                "type": "couchy_access_test",
                "created_at": ts,
            });
            let body3 = serde_json::to_string(&doc3).unwrap_or_else(|_| doc3.to_string());
            send(Self::curl_command(&host, &user, &password, "PUT", &path3, Some(&body3)));
            let resp = http
                .put(format!("{base}{path3}"))
                .header("Content-Type", "application/json")
                .basic_auth(&user, Some(&password))
                .body(body3)
                .send()
                .await;
            let (status, body) = match resp {
                Ok(r) => {
                    let s = r.status().as_u16();
                    let b = r.text().await.unwrap_or_default();
                    (s, b)
                }
                Err(e) => (0, e.to_string()),
            };
            let created = (200..=202).contains(&status);
            if created {
                let rev = serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|v| v.get("rev").and_then(|r| r.as_str()).map(|s| s.to_string()));
                if let Some(rev) = rev {
                    let dpath = format!("/{db_enc}/{test_id}?rev={rev}");
                    let _ = http
                        .delete(format!("{base}{dpath}"))
                        .basic_auth(&user, Some(&password))
                        .send()
                        .await;
                }
            }
            let detail = format!("[{status}] {}", body.chars().take(120).collect::<String>());
            send(row("Write with credentials", created, detail));

            let test_id2 = format!("couchy_access_test_anon_{ts}");
            let path4 = format!("/{db_enc}/{test_id2}");
            let doc4 = serde_json::json!({
                "_id": test_id2,
                "type": "couchy_access_test",
                "created_at": ts,
            });
            let body4 = serde_json::to_string(&doc4).unwrap_or_else(|_| doc4.to_string());
            send(Self::curl_command(&host, "", "", "PUT", &path4, Some(&body4)));
            let resp = http
                .put(format!("{base}{path4}"))
                .header("Content-Type", "application/json")
                .body(body4)
                .send()
                .await;
            let (status, body) = match resp {
                Ok(r) => {
                    let s = r.status().as_u16();
                    let b = r.text().await.unwrap_or_default();
                    (s, b)
                }
                Err(e) => (0, e.to_string()),
            };
            let created = (200..=202).contains(&status);
            if created {
                let rev = serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|v| v.get("rev").and_then(|r| r.as_str()).map(|s| s.to_string()));
                if let Some(rev) = rev {
                    let dpath = format!("/{db_enc}/{test_id2}?rev={rev}");
                    let _ = http
                        .delete(format!("{base}{dpath}"))
                        .basic_auth(&user, Some(&password))
                        .send()
                        .await;
                }
            }
            let detail = format!("[{status}] {}", body.chars().take(120).collect::<String>());
            send(row("Write anonymous", created, detail));

            send("ACCESS_TEST_DONE".to_string());
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
            let base = config.host.trim_end_matches('/').to_string();
            let db_enc = db.replace('/', "%2F");
            let http = reqwest::Client::new();
            let send = |msg: String| {
                println!("{msg}");
                if let Ok(mut p) = pending.lock() {
                    p.push(msg);
                }
                ctx2.request_repaint();
            };

            let path = format!("/{db_enc}/_all_docs");
            send(Self::curl_command(&config.host, &config.user, &config.password, "GET", &path, None));
            let req = http.get(format!("{base}{path}"));
            let req = if !config.user.is_empty() || !config.password.is_empty() {
                req.basic_auth(&config.user, Some(&config.password))
            } else {
                req
            };
            let (status, body) = match req.send().await {
                Ok(resp) => {
                    let s = resp.status().as_u16();
                    let b = resp.text().await.unwrap_or_default();
                    (s, b)
                }
                Err(e) => {
                    send(format!("DEL_ALL_ERR|Failed to list documents: {e}"));
                    return;
                }
            };
            if !(200..300).contains(&status) {
                let snippet: String = body.chars().take(200).collect();
                send(format!("DEL_ALL_ERR|GET {path} failed [{status}]: {snippet}"));
                return;
            }
            let parsed: serde_json::Value = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(e) => {
                    send(format!("DEL_ALL_ERR|Failed to parse _all_docs response: {e}"));
                    return;
                }
            };
            let rows: Vec<(String, String)> = parsed
                .get("rows")
                .and_then(|r| r.as_array())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|row| {
                    let id = row.get("id").and_then(|i| i.as_str())?.to_string();
                    let rev = row
                        .get("value")
                        .and_then(|v| v.get("rev"))
                        .and_then(|r| r.as_str())?
                        .to_string();
                    Some((id, rev))
                })
                .collect();
            let internal_count = rows.iter().filter(|(id, _)| id.starts_with('_')).count();
            let rows: Vec<(String, String)> = rows.into_iter().filter(|(id, _)| !id.starts_with('_')).collect();
            let total = rows.len();
            if total == 0 {
                send(format!(
                    "DEL_ALL_DONE|{db}|Delete all Docs: no documents to delete ({internal_count} documents starting with '_' were kept)."
                ));
                return;
            }
            if internal_count > 0 {
                send(format!("Skipping {internal_count} document(s) starting with '_' (design documents)."));
            }
            send(format!("Found {total} documents in {db}. Deleting..."));

            let bulk_path = format!("/{db_enc}/_bulk_docs");
            let url = format!("{base}{bulk_path}");
            let mut deleted = 0usize;
            for chunk in rows.chunks(100) {
                let docs: Vec<serde_json::Value> = chunk
                    .iter()
                    .map(|(id, rev)| serde_json::json!({ "_id": id, "_rev": rev, "_deleted": true }))
                    .collect();
                let payload = serde_json::json!({ "docs": docs });
                send(format!("curl -X POST '{url}' -H 'Content-Type: application/json'  (batch of {})", chunk.len()));
                let req = http
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .basic_auth(&config.user, Some(&config.password))
                    .json(&payload);
                match req.send().await {
                    Ok(resp) => {
                        let status = resp.status().as_u16();
                        let body = resp.text().await.unwrap_or_default();
                        if (200..300).contains(&status) {
                            deleted += chunk.len();
                            send(format!("Deleted {deleted}/{total} documents..."));
                        } else {
                            let snippet: String = body.chars().take(200).collect();
                            send(format!("DEL_ALL_ERR|_bulk_docs failed [{status}]: {snippet}"));
                            return;
                        }
                    }
                    Err(e) => {
                        send(format!("DEL_ALL_ERR|_bulk_docs request failed: {e}"));
                        return;
                    }
                }
            }
            send(format!(
                "DEL_ALL_DONE|{db}|Delete all Docs finished: {deleted} documents deleted from {db}."
            ));
        });
    }

    fn purge_all_server_dbs(&mut self, ctx: &egui::Context) {
        let config = self.current_connection_config();
        let pending = self.pending_log.clone();
        let ctx2 = ctx.clone();
        self.purge_all_running = true;
        self.log_lines.push_str("\nPurge deleted Docs (all databases) starting...");
        tokio::spawn(async move {
            let send = |msg: String| {
                println!("{msg}");
                if let Ok(mut p) = pending.lock() {
                    p.push(msg);
                }
                ctx2.request_repaint();
            };
            let client = match couch_rs::Client::new(&config.host, &config.user, &config.password) {
                Ok(c) => c,
                Err(e) => {
                    send(format!("DEL_PURGE_ALL_ERR|Failed to connect: {e}"));
                    return;
                }
            };
            let dbs = match client.list_dbs().await {
                Ok(d) => d,
                Err(e) => {
                    send(format!("DEL_PURGE_ALL_ERR|Failed to list databases: {e}"));
                    return;
                }
            };
            let total = dbs.len();
            send(format!("Found {total} databases."));
            let base = config.host.trim_end_matches('/').to_string();
            let http = reqwest::Client::new();
            let user = config.user.clone();
            let password = config.password.clone();
            let host = config.host.clone();
            let mut failures = 0usize;
            for (idx, dbname) in dbs.into_iter().enumerate() {
                let i = idx + 1;
                let db_enc = dbname.replace('/', "%2F");
                send(Self::curl_command(&host, &user, &password, "GET", &format!("/{db_enc}/_changes"), None));
                match purge_db_deleted_docs(&http, &base, &user, &password, &dbname, send.clone()).await {
                    Ok(line) => {
                        send(format!("[{i}/{total}] {line}"));
                    }
                    Err(e) => {
                        failures += 1;
                        send(format!("[{i}/{total}] {dbname}: {e}"));
                    }
                }
            }
            send(format!(
                "DEL_PURGE_ALL_DONE|Purge deleted Docs finished: {total} databases processed, {failures} failure(s)."
            ));
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
        let user = config.user.clone();
        let password = config.password.clone();
        let host = config.host.clone();
        tokio::spawn(async move {
            let send = |msg: String| {
                println!("{msg}");
                if let Ok(mut p) = pending.lock() {
                    p.push(msg);
                }
                ctx2.request_repaint();
            };
            let base = host.trim_end_matches('/').to_string();
            let http = reqwest::Client::new();
            match purge_db_deleted_docs(&http, &base, &user, &password, &db, send.clone()).await {
                Ok(line) => send(format!("DEL_PURGE_OK|{line}")),
                Err(e) => send(format!("DEL_PURGE_ERR|{db}: {e}")),
            }
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
        let base = config.host.trim_end_matches('/').to_string();
        let db_enc = db.replace('/', "%2F");
        let (method, path) = if id.is_empty() {
            ("POST", format!("/{db_enc}"))
        } else {
            ("PUT", format!("/{db_enc}/{}", id.replace('/', "%2F")))
        };
        let curl = Self::curl_command(&config.host, &config.user, &config.password, method, &path, Some(&body));
        self.log_lines.push_str(&format!("\n{curl}"));
        tokio::spawn(async move {
            let http = reqwest::Client::new();
            let url = format!("{base}{path}");
            let req = if method == "POST" {
                http.post(&url)
            } else {
                http.put(&url)
            };
            let req = req
                .header("Content-Type", "application/json")
                .header("Accept", "application/json")
                .basic_auth(&config.user, Some(&config.password))
                .body(body);
            let msg = match req.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    match resp.text().await {
                        Ok(body) => {
                            let snippet: String = body.chars().take(200).collect();
                            if (200..300).contains(&status) {
                                let created_id = serde_json::from_str::<serde_json::Value>(&body)
                                    .ok()
                                    .and_then(|v| v.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()))
                                    .or_else(|| Some(id.clone()))
                                    .unwrap_or_default();
                                format!("DOC_CREATED|{db}|Created document {created_id} [{status}]")
                            } else if status == 409 {
                                format!("DOC_CREATE_ERR|Document already exists [{status}]")
                            } else {
                                format!("DOC_CREATE_ERR|Create failed [{status}]: {snippet}")
                            }
                        }
                        Err(e) => format!("DOC_CREATE_ERR|Create failed: {e}"),
                    }
                }
                Err(e) => format!("DOC_CREATE_ERR|Create failed: {e}"),
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
            host = format!("https://{host}");
        }
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
        self.log_lines.push_str(&format!(
            "\n{}",
            Self::curl_command(&host, &user, &password, "GET", &test_path(&self.database), None)
        ));
        let pending = self.pending_log.clone();
        let ctx = ctx.clone();
        let host_for_state = host.clone();
        let db_test = self.database.trim().to_string();
        tokio::spawn(async move {
            let msg = if !db_test.is_empty() {
                let base = host.trim_end_matches('/').to_string();
                let db_enc = db_test.replace('/', "%2F");
                let url = format!("{base}/{db_enc}");
                let http = reqwest::Client::new();
                let req = http.get(&url);
                let req = if !user.is_empty() || !password.is_empty() {
                    req.basic_auth(&user, Some(&password))
                } else {
                    req
                };
                let (status, body) = match req.send().await {
                    Ok(resp) => {
                        let s = resp.status().as_u16();
                        let b = resp.text().await.unwrap_or_default();
                        (s, b)
                    }
                    Err(e) => (0, e.to_string()),
                };
                if (200..300).contains(&status) {
                    format!("CONNECTED_OK|{host_for_state}|Connected to {host} (database {db_test})")
                } else {
                    let snippet: String = body.chars().take(160).collect();
                    format!("CONNECTED_ERR|Connection failed: {host}: database {db_test} [{status}] {snippet}")
                }
            } else {
                match couch_rs::Client::new(&host, &user, &password) {
                    Ok(client) => match client.list_dbs().await {
                        Ok(dbs) => format!("CONNECTED_OK|{host_for_state}|Connected to {host} ({} databases)", dbs.len()),
                        Err(e) => format!("CONNECTED_ERR|Connection failed: {host}: {e}"),
                    },
                    Err(e) => format!("CONNECTED_ERR|Connection failed: {host}: {e}"),
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
        let mut fetch_db_info = false;
        let mut open_db: Option<String> = None;
        let mut refresh_doc = false;
        let mut refresh_users = false;
        let mut refresh_readonly = false;
        let mut check_public = false;
        let mut create_refresh = false;
        let mut delete_all_refresh: Option<String> = None;
        let mut probe_ok: Option<serde_json::Value> = None;
        if let Ok(mut pending) = self.pending_log.lock() {
            if !pending.is_empty() {
                for msg in pending.drain(..) {
                    if let Some(rest) = msg.strip_prefix("CONNECTED_OK|") {
                        if let Some((h, m)) = rest.split_once('|') {
                            self.connected = true;
                            self.connected_host = h.to_string();
                            let db = self.database.trim().to_string();
                            if !db.is_empty() {
                                self.selected_db = Some(db.clone());
                                self.doc_filter.clear();
                                self.doc_list.clear();
                                self.selected_doc = None;
                                self.doc_json = None;
                                self.doc_loading = false;
                                open_db = Some(db);
                            } else {
                                fetch_db_info = true;
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
                                self.db_infos = Self::parse_db_infos(data);
                                check_public = !self.db_infos.is_empty();
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
                    } else if let Some(rest) = msg.strip_prefix("ACCESS_TEST_ROW|") {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(rest) {
                            self.access_test_results.push(AccessTestRow {
                                name: v.get("name").and_then(|x| x.as_str()).unwrap_or("?").to_string(),
                                ok: v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false),
                                detail: v.get("detail").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                            });
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if let Some(rest) = msg.strip_prefix("PROBE_OK|") {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(rest) {
                            probe_ok = Some(v);
                        } else {
                            self.log_lines.push('\n');
                            self.log_lines.push_str(rest);
                        }
                    } else if let Some(m) = msg.strip_prefix("PROBE_ERR|") {
                        self.log_lines.push('\n');
                        self.log_lines.push_str(m);
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
        if fetch_db_info {
            self.action = "get_dbs_info".to_string();
            self.run_action(ctx);
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
        if check_public {
            self.check_public_dbs(ctx);
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
                    if ui.button("List DBs").clicked() {
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
