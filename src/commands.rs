use crate::config::AppConfig;
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub struct DbInfoRow {
    pub name: String,
    pub doc_count: Option<u64>,
    pub doc_del_count: Option<u64>,
    pub data_size: Option<u64>,
    pub disk_size: Option<u64>,
    pub compact_running: Option<bool>,
    pub public: Option<bool>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct AccessTestResult {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct UserRow {
    pub id: String,
    pub name: String,
    pub rev: String,
    pub roles: String,
    pub doc: Value,
    #[serde(default)]
    pub peruser_db_exists: bool,
    #[serde(default)]
    pub public_readonly: bool,
}

fn base(config: &AppConfig) -> String {
    config.host.trim_end_matches('/').to_string()
}

fn db_path(db: &str) -> String {
    db.replace('/', "%2F")
}

fn auth(req: reqwest::RequestBuilder, user: &str, password: &str) -> reqwest::RequestBuilder {
    if !user.is_empty() || !password.is_empty() {
        req.basic_auth(user, Some(password))
    } else {
        req
    }
}

async fn get_json(
    http: &reqwest::Client,
    base: &str,
    user: &str,
    password: &str,
    path: &str,
) -> (u16, Option<Value>, String) {
    let url = format!("{base}{path}");
    let req = auth(http.get(&url), user, password);
    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            let v = serde_json::from_str::<Value>(&body).ok();
            (status, v, body)
        }
        Err(e) => (0, None, e.to_string()),
    }
}

async fn send_json(
    http: &reqwest::Client,
    method: reqwest::Method,
    base: &str,
    user: &str,
    password: &str,
    path: &str,
    body: &str,
) -> (u16, String) {
    let url = format!("{base}{path}");
    let req = auth(
        http.request(method, &url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(body.to_string()),
        user,
        password,
    );
    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            (status, body)
        }
        Err(e) => (0, e.to_string()),
    }
}

pub async fn connect(config: &AppConfig) -> Result<String, String> {
    let host = config.host.trim().to_string();
    if host.is_empty() {
        return Err("Host is empty.".to_string());
    }
    if !host.starts_with("http://") && !host.starts_with("https://") {
        return Err(format!("Host '{host}' has no scheme (expected http(s)://...)."));
    }
    let db = config.database.trim().to_string();
    if !db.is_empty() {
        let b = base(config);
        let db_enc = db_path(&db);
        let http = reqwest::Client::new();
        let (status, parsed, body) = get_json(&http, &b, &config.user, &config.password, &format!("/{db_enc}")).await;
        if (200..300).contains(&status) {
            let _ = parsed;
            return Ok(format!("Connected to {host} (database {db})"));
        }
        let snippet: String = body.chars().take(200).collect();
        return Err(format!("Connection failed: {host}: database {db} [{status}] {snippet}"));
    }
    match couch_rs::Client::new(&host, &config.user, &config.password) {
        Ok(client) => match client.list_dbs().await {
            Ok(dbs) => Ok(format!("Connected to {host} ({} databases)", dbs.len())),
            Err(e) => Err(format!("Connection failed: {host}: {e}")),
        },
        Err(e) => Err(format!("Connection failed: {host}: {e}")),
    }
}

pub async fn list_db_names(config: &AppConfig) -> Result<Vec<String>, String> {
    let host = config.host.trim().to_string();
    let client = couch_rs::Client::new(&host, &config.user, &config.password).map_err(|e| e.to_string())?;
    client.list_dbs().await.map_err(|e| e.to_string())
}

pub async fn get_dbs_info(config: &AppConfig) -> Result<Value, String> {
    let dbs = list_db_names(config).await?;
    let b = base(config);
    let http = reqwest::Client::new();
    let url = format!("{b}/_dbs_info");
    let req = auth(
        http.post(&url)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "keys": dbs })),
        &config.user,
        &config.password,
    );
    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            match serde_json::from_str::<Value>(&body) {
                Ok(v) => Ok(v),
                Err(e) => Err(format!("Failed to parse /_dbs_info response: {e} [{status}]")),
            }
        }
        Err(e) => Err(format!("POST /_dbs_info failed: {e}")),
    }
}

pub fn parse_db_infos(data: &Value) -> Vec<DbInfoRow> {
    let entries = if let Some(arr) = data.as_array() {
        arr.clone()
    } else if let Some(obj) = data.as_object() {
        obj.iter().map(|(k, v)| serde_json::json!({ "key": k, "info": v })).collect()
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

pub async fn list_docs(
    config: &AppConfig,
    db: &str,
) -> Result<Vec<(String, String)>, String> {
    let b = base(config);
    let db_enc = db_path(db);
    let http = reqwest::Client::new();
    let path = format!("/{db_enc}/_all_docs");
    let (status, parsed, body) = get_json(&http, &b, &config.user, &config.password, &path).await;
    if !(200..300).contains(&status) {
        let snippet: String = body.chars().take(200).collect();
        return Err(format!("_all_docs failed [{status}]: {snippet}"));
    }
    let parsed = parsed.ok_or_else(|| "Unexpected response".to_string())?;
    let rows = parsed
        .get("rows")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    let out = rows
        .into_iter()
        .filter_map(|row| {
            let id = row.get("id").and_then(|i| i.as_str())?.to_string();
            let rev = row
                .get("value")
                .and_then(|v| v.get("rev"))
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .to_string();
            Some((id, rev))
        })
        .collect();
    Ok(out)
}

pub async fn get_doc(config: &AppConfig, db: &str, id: &str) -> Result<Value, String> {
    let b = base(config);
    let path = format!("/{}/{}", db_path(db), db_path(id));
    let http = reqwest::Client::new();
    let (status, parsed, body) = get_json(&http, &b, &config.user, &config.password, &path).await;
    if !(200..300).contains(&status) {
        let snippet: String = body.chars().take(200).collect();
        return Err(format!("GET {path} failed [{status}]: {snippet}"));
    }
    parsed.ok_or_else(|| "Unexpected response".to_string())
}

pub async fn save_doc(config: &AppConfig, db: &str, doc: &Value) -> Result<Value, String> {
    let b = base(config);
    let id = doc.get("_id").and_then(|i| i.as_str()).ok_or_else(|| "Document has no _id".to_string())?;
    let path = format!("/{}/{}", db_path(db), db_path(id));
    let body = serde_json::to_string(doc).map_err(|e| e.to_string())?;
    let (status, body) =
        send_json(&reqwest::Client::new(), reqwest::Method::PUT, &b, &config.user, &config.password, &path, &body).await;
    if !(200..300).contains(&status) {
        let snippet: String = body.chars().take(200).collect();
        if status == 409 {
            return Err(format!("Document already exists with different revision [{status}]"));
        }
        return Err(format!("PUT {path} failed [{status}]: {snippet}"));
    }
    serde_json::from_str(&body).map_err(|e| format!("Failed to parse response: {e}"))
}

pub async fn create_doc(
    config: &AppConfig,
    db: &str,
    id: &str,
    doc: &Value,
) -> Result<Value, String> {
    let b = base(config);
    let id = id.trim();
    let (method, path) = if id.is_empty() {
        (reqwest::Method::POST, format!("/{}", db_path(db)))
    } else {
        (reqwest::Method::PUT, format!("/{}/{}", db_path(db), db_path(id)))
    };
    let body = serde_json::to_string(doc).map_err(|e| e.to_string())?;
    let (status, body) =
        send_json(&reqwest::Client::new(), method, &b, &config.user, &config.password, &path, &body).await;
    if !(200..300).contains(&status) {
        let snippet: String = body.chars().take(200).collect();
        if status == 409 {
            return Err(format!("Document already exists [{status}]"));
        }
        return Err(format!("Create failed [{status}]: {snippet}"));
    }
    serde_json::from_str(&body).map_err(|e| format!("Failed to parse response: {e}"))
}

pub async fn delete_all_docs(
    config: &AppConfig,
    db: &str,
    pending: Option<Arc<Mutex<Vec<String>>>>,
    ctx: Option<eframe::egui::Context>,
) -> Result<String, String> {
    let base = base(config);
    let db_enc = db_path(db);
    let report = |msg: String| {
        println!("{msg}");
        if let Some(p) = &pending {
            if let Ok(mut q) = p.lock() {
                q.push(msg);
            }
        }
        if let Some(c) = &ctx {
            c.request_repaint();
        }
    };
    report(format!("Delete all Docs: starting in {db} ..."));

    let http = reqwest::Client::new();
    let path = format!("/{db_enc}/_all_docs");
    let (status, parsed, body) = get_json(&http, &base, &config.user, &config.password, &path).await;
    if !(200..300).contains(&status) {
        let snippet: String = body.chars().take(200).collect();
        return Err(format!("GET {path} failed [{status}]: {snippet}"));
    }
    let parsed = parsed.ok_or_else(|| "Unexpected response".to_string())?;
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
        return Ok(format!(
            "Delete all Docs: no documents to delete ({internal_count} document(s) starting with '_' were kept)."
        ));
    }
    if internal_count > 0 {
        report(format!("Skipping {internal_count} document(s) starting with '_' (design documents)."));
    }
    report(format!("Found {total} documents in {db}. Deleting..."));

    let url = format!("{base}/{db_enc}/_bulk_docs");
    let mut deleted = 0usize;
    for chunk in rows.chunks(100) {
        let docs: Vec<Value> = chunk
            .iter()
            .map(|(id, rev)| serde_json::json!({ "_id": id, "_rev": rev, "_deleted": true }))
            .collect();
        let payload = serde_json::json!({ "docs": docs });
        report(format!("curl -X POST '{url}' (batch of {})", chunk.len()));
        let req = auth(
            http.post(&url).header("Content-Type", "application/json").json(&payload),
            &config.user,
            &config.password,
        );
        let (status, body) = match req.send().await {
            Ok(resp) => {
                let s = resp.status().as_u16();
                let b = resp.text().await.unwrap_or_default();
                (s, b)
            }
            Err(e) => return Err(format!("_bulk_docs request failed: {e}")),
        };
        if (200..300).contains(&status) {
            deleted += chunk.len();
            report(format!("Deleted {deleted}/{total} documents..."));
        } else {
            let snippet: String = body.chars().take(200).collect();
            return Err(format!("_bulk_docs failed [{status}]: {snippet}"));
        }
    }
    Ok(format!("Delete all Docs finished: {deleted} documents deleted from {db}."))
}

pub async fn purge_db_deleted_docs(
    config: &AppConfig,
    db: &str,
    pending: Option<Arc<Mutex<Vec<String>>>>,
    ctx: Option<eframe::egui::Context>,
) -> Result<String, String> {
    let base = base(config);
    let db_enc = db_path(db);
    let report = |msg: String| {
        println!("{msg}");
        if let Some(p) = &pending {
            if let Ok(mut q) = p.lock() {
                q.push(msg);
            }
        }
        if let Some(c) = &ctx {
            c.request_repaint();
        }
    };

    let http = reqwest::Client::new();
    let (status, parsed, _body) = get_json(&http, &base, &config.user, &config.password, &format!("/{db_enc}/_changes")).await;
    if status != 200 {
        return Err(format!("[{status}] GET _changes failed"));
    }
    let parsed = parsed.ok_or_else(|| "Unexpected response from _changes".to_string())?;

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
        return Ok(format!("{db}: 0 deleted documents to purge"));
    }
    let mut purged = 0usize;
    for chunk in pairs.chunks(99) {
        let mut map = serde_json::Map::new();
        for (id, rev) in chunk {
            map.insert(id.clone(), serde_json::json!([rev]));
        }
        let body = Value::Object(map).to_string();
        let purge_url = format!("{base}/{db_enc}/_purge");
        let (status, body) =
            send_json(&http, reqwest::Method::POST, &base, &config.user, &config.password, &format!("/{db_enc}/_purge"), &body).await;
        if status != 200 && status != 201 && status != 202 {
            let snippet: String = body.chars().take(200).collect();
            return Err(format!("[{status}] POST {purge_url} failed: {snippet}"));
        }
        let v: Value = serde_json::from_str(&body).unwrap_or(serde_json::json!({}));
        let n = v
            .get("purged")
            .and_then(|p| p.as_object())
            .map(|o| o.len())
            .unwrap_or(chunk.len());
        purged += n;
        report(format!("  purged {purged}/{total} deleted documents..."));
    }
    Ok(format!("{db}: purged {purged} deleted documents"))
}

pub async fn purge_server_deleted_docs(
    config: &AppConfig,
    pending: Option<Arc<Mutex<Vec<String>>>>,
    ctx: Option<eframe::egui::Context>,
) -> Result<String, String> {
    let report = |msg: String| {
        println!("{msg}");
        if let Some(p) = &pending {
            if let Ok(mut q) = p.lock() {
                q.push(msg);
            }
        }
        if let Some(c) = &ctx {
            c.request_repaint();
        }
    };
    let dbs = list_db_names(config).await?;
    let total = dbs.len();
    report(format!("Found {total} databases."));
    let mut failures = 0usize;
    for (idx, dbname) in dbs.into_iter().enumerate() {
        let i = idx + 1;
        match purge_db_deleted_docs(config, &dbname, None, None).await {
            Ok(line) => report(format!("[{i}/{total}] {line}")),
            Err(e) => {
                failures += 1;
                report(format!("[{i}/{total}] {dbname}: {e}"));
            }
        }
    }
    Ok(format!(
        "Purge deleted Docs finished: {total} databases processed, {failures} failure(s)."
    ))
}

pub async fn list_users(config: &AppConfig) -> Result<Vec<UserRow>, String> {
    let b = base(config);
    let http = reqwest::Client::new();
    let path = "/_users/_all_docs?include_docs=true";
    let (status, parsed, body) = get_json(&http, &b, &config.user, &config.password, path).await;
    if !(200..300).contains(&status) {
        let snippet: String = body.chars().take(200).collect();
        return Err(format!("GET {path} failed [{status}]: {snippet}"));
    }
    let parsed = parsed.ok_or_else(|| "Unexpected response".to_string())?;
    let rows = parsed
        .get("rows")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    let mut users = Vec::new();
    for row in rows {
        let doc = row.get("doc").cloned().unwrap_or(Value::Null);
        if doc.get("type").and_then(|t| t.as_str()) != Some("user") {
            continue;
        }
        let id = row.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
        let name = doc.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
        let rev = doc.get("_rev").and_then(|r| r.as_str()).unwrap_or("").to_string();
        let roles = doc
            .get("roles")
            .and_then(|r| r.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        users.push(UserRow {
            id,
            name,
            rev,
            roles,
            doc,
            peruser_db_exists: false,
            public_readonly: false,
        });
    }
    Ok(users)
}

pub async fn save_user(
    config: &AppConfig,
    name: &str,
    password: &str,
    roles: Vec<String>,
    existing: Option<Value>,
) -> Result<String, String> {
    if name.trim().is_empty() {
        return Err("Username is empty.".to_string());
    }
    let is_edit = existing.is_some();
    let doc = if let Some(mut d) = existing {
        d["name"] = Value::String(name.to_string());
        d["type"] = Value::String("user".to_string());
        d["roles"] = Value::Array(roles.iter().map(|r| Value::String(r.clone())).collect());
        if !password.is_empty() {
            d["password"] = Value::String(password.to_string());
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
    let b = base(config);
    let path = format!("/_users/org.couchdb.user:{name}");
    let body = serde_json::to_string(&doc).map_err(|e| e.to_string())?;
    let (status, body) = send_json(&reqwest::Client::new(), reqwest::Method::PUT, &b, &config.user, &config.password, &path, &body).await;
    if (200..300).contains(&status) {
        let action = if is_edit { "Updated" } else { "Created" };
        return Ok(format!("{action} {name} [{status}]"));
    }
    let snippet: String = body.chars().take(200).collect();
    Err(format!("PUT {path} failed [{status}]: {snippet}"))
}

pub async fn delete_user(config: &AppConfig, id: &str, rev: &str) -> Result<String, String> {
    let b = base(config);
    let path = format!("/_users/{id}?rev={rev}");
    let http = reqwest::Client::new();
    let req = auth(http.delete(format!("{b}{path}")), &config.user, &config.password);
    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            if (200..300).contains(&status) {
                Ok(format!("Deleted {id} [{status}]"))
            } else {
                let snippet: String = body.chars().take(200).collect();
                Err(format!("DELETE {path} failed [{status}]: {snippet}"))
            }
        }
        Err(e) => Err(format!("DELETE {path} failed: {e}")),
    }
}

pub async fn check_readonly_db(config: &AppConfig, db: &str) -> Result<bool, String> {
    let b = base(config);
    let path = format!("/{}/_design/auth", db_path(db));
    let http = reqwest::Client::new();
    let (status, parsed, _body) = get_json(&http, &b, &config.user, &config.password, &path).await;
    if status == 404 {
        return Ok(false);
    }
    if !(200..300).contains(&status) {
        return Err(format!("GET {path} failed [{status}]"));
    }
    Ok(parsed
        .map(|v| v.get("validate_doc_update").is_some())
        .unwrap_or(false))
}

pub async fn check_public_db(config: &AppConfig, db: &str) -> Result<bool, String> {
    let b = base(config);
    let path = format!("/{}", db_path(db));
    let http = reqwest::Client::new();
    let (status, _parsed, _body) = get_json(&http, &b, "", "", &path).await;
    Ok(status == 200)
}

pub async fn user_peruser_db_exists(config: &AppConfig, name: &str) -> Result<bool, String> {
    let hex: String = name.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    let db = format!("userdb-{hex}");
    let b = base(config);
    let http = reqwest::Client::new();
    let path = format!("/{}", db_path(&db));
    let (status, _parsed, _body) = get_json(&http, &b, &config.user, &config.password, &path).await;
    Ok(status == 200)
}

pub async fn access_test(
    config: &AppConfig,
    db: &str,
    pending: Option<Arc<Mutex<Vec<String>>>>,
    ctx: Option<eframe::egui::Context>,
) -> Result<Vec<AccessTestResult>, String> {
    let base = base(config);
    let db_enc = db_path(db);
    let http = reqwest::Client::new();
    let user = &config.user;
    let password = &config.password;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let send = |msg: String| {
        println!("{msg}");
        if let Some(p) = &pending {
            if let Ok(mut q) = p.lock() {
                q.push(msg);
            }
        }
        if let Some(c) = &ctx {
            c.request_repaint();
        }
    };

    let mut results: Vec<AccessTestResult> = Vec::new();

    let path1 = format!("/{db_enc}/_all_docs?limit=1");
    let req = auth(http.get(format!("{base}{path1}")), user, password);
    let (status, body) = match req.send().await {
        Ok(r) => {
            let s = r.status().as_u16();
            let b = r.text().await.unwrap_or_default();
            (s, b)
        }
        Err(e) => (0, e.to_string()),
    };
    let total = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v.get("total_rows").and_then(|t| t.as_u64()));
    let detail = match total {
        Some(t) => format!("[{status}] total_rows={t}"),
        None => format!("[{status}] {}", body.chars().take(120).collect::<String>()),
    };
    send(format!("Read with credentials: {}", if status == 200 { "OK" } else { "FAILED" }));
    results.push(AccessTestResult {
        name: "Read with credentials".to_string(),
        ok: status == 200,
        detail,
    });

    let req = http.get(format!("{base}{path1}"));
    let (status, body) = match req.send().await {
        Ok(r) => {
            let s = r.status().as_u16();
            let b = r.text().await.unwrap_or_default();
            (s, b)
        }
        Err(e) => (0, e.to_string()),
    };
    let total = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v.get("total_rows").and_then(|t| t.as_u64()));
    let detail = match total {
        Some(t) => format!("[{status}] total_rows={t}"),
        None => format!("[{status}] {}", body.chars().take(120).collect::<String>()),
    };
    send(format!("Read anonymous: {}", if status == 200 { "OK" } else { "FAILED" }));
    results.push(AccessTestResult {
        name: "Read anonymous".to_string(),
        ok: status == 200,
        detail,
    });

    let test_id = format!("couchy_access_test_{ts}");
    let path3 = format!("/{db_enc}/{test_id}");
    let doc3 = serde_json::json!({ "_id": test_id, "type": "couchy_access_test", "created_at": ts });
    let body3 = serde_json::to_string(&doc3).unwrap_or_else(|_| doc3.to_string());
    let (status, body) = send_json(
        &http,
        reqwest::Method::PUT,
        &base,
        user,
        password,
        &path3,
        &body3,
    )
    .await;
    let created = (200..=202).contains(&status);
    if created {
        let rev = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v.get("rev").and_then(|r| r.as_str()).map(|s| s.to_string()));
        if let Some(rev) = rev {
            let dpath = format!("/{db_enc}/{test_id}?rev={rev}");
            let _ = auth(
                http.delete(format!("{base}{dpath}")),
                user,
                password,
            )
            .send()
            .await;
        }
    }
    let detail = format!("[{status}] {}", body.chars().take(120).collect::<String>());
    send(format!("Write with credentials: {}", if created { "OK" } else { "FAILED" }));
    results.push(AccessTestResult {
        name: "Write with credentials".to_string(),
        ok: created,
        detail,
    });

    let test_id2 = format!("couchy_access_test_anon_{ts}");
    let path4 = format!("/{db_enc}/{test_id2}");
    let doc4 = serde_json::json!({ "_id": test_id2, "type": "couchy_access_test", "created_at": ts });
    let body4 = serde_json::to_string(&doc4).unwrap_or_else(|_| doc4.to_string());
    let (status, body) = send_json(
        &http,
        reqwest::Method::PUT,
        &base,
        "",
        "",
        &path4,
        &body4,
    )
    .await;
    let created = (200..=202).contains(&status);
    if created {
        let rev = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v.get("rev").and_then(|r| r.as_str()).map(|s| s.to_string()));
        if let Some(rev) = rev {
            let dpath = format!("/{db_enc}/{test_id2}?rev={rev}");
            let _ = auth(
                http.delete(format!("{base}{dpath}")),
                user,
                password,
            )
            .send()
            .await;
        }
    }
    let detail = format!("[{status}] {}", body.chars().take(120).collect::<String>());
    send(format!("Write anonymous: {}", if created { "OK" } else { "FAILED" }));
    results.push(AccessTestResult {
        name: "Write anonymous".to_string(),
        ok: created,
        detail,
    });

    Ok(results)
}

pub async fn find_user(config: &AppConfig, name: &str) -> Result<Option<UserRow>, String> {
    let users = list_users(config).await?;
    Ok(users.into_iter().find(|u| u.name == name))
}

async fn fetch_rev(
    http: &reqwest::Client,
    base: &str,
    user: &str,
    password: &str,
    path: &str,
) -> Option<String> {
    let (status, parsed, _body) = get_json(http, base, user, password, path).await;
    if !(200..300).contains(&status) {
        return None;
    }
    parsed
        .and_then(|v| v.get("_rev").and_then(|r| r.as_str()).map(|s| s.to_string()))
}

pub async fn setup_readonly_db(
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
        ddoc["_rev"] = Value::String(rev);
    }
    let dbody = serde_json::to_string_pretty(&ddoc).unwrap_or_else(|_| ddoc.to_string());
    logs.push(format!("PUT {dpath}"));
    let (status, body) = send_json(
        &http,
        reqwest::Method::PUT,
        &base,
        &user,
        &password,
        &dpath,
        &dbody,
    )
    .await;
    let snippet: String = body.chars().take(200).collect();
    logs.push(format!("  {db}/_design/auth [{status}] {snippet}"));

    let spath = format!("/{db}/_security");
    let mut sec = serde_json::json!({
        "admins": { "names": [], "roles": [] },
        "members": { "names": [], "roles": [] },
    });
    if let Some(rev) = fetch_rev(&http, &base, &user, &password, &spath).await {
        sec["_rev"] = Value::String(rev);
    }
    let sbody = serde_json::to_string(&sec).unwrap_or_else(|_| sec.to_string());
    logs.push(format!("PUT {spath}"));
    let (status, body) = send_json(
        &http,
        reqwest::Method::PUT,
        &base,
        &user,
        &password,
        &spath,
        &sbody,
    )
    .await;
    let snippet: String = body.chars().take(200).collect();
    logs.push(format!("  {db}/_security [{status}] {snippet}"));

    logs
}
